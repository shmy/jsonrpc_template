dev:
    cargo watch -x "run -p api"

# Local Postgres + Redis
db-up:
    docker compose up -d

# Fast iteration: fmt check + clippy for one crate
lint crate:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    cargo fmt --all -- --check
    cargo clippy -p {{crate}} --all-targets

check:
    cargo check -p api

# 日常迭代：固定一种命令形态，勿与 --all-features / 全 workspace 来回切（会触发 feature 统一化补建）。
# 叶子改动用 check-fast；要编默认 otlp+blob-fs 组合根再用 check。
check-fast crate="identity":
    cargo check -p {{crate}}

# Workspace 分层：feature 互依 / kit→模板 / infra→contract 例外（scripts/check_layering.py）
layering:
    python3 scripts/check_layering.py

# 重新生成 docs/openrpc-viewer.min.js（/docs 内嵌的压缩产物）。改 viewer 源文件后必须跑。
# 需要 node/npx；terser 钉主版本 5 保证输出稳定。
docs-min:
    #!/usr/bin/env bash
    set -euo pipefail
    npx -y terser@5 docs/openrpc-viewer.js --compress passes=2 --mangle --comments false --ecma 2017 -o docs/openrpc-viewer.min.js
    node --check docs/openrpc-viewer.min.js
    if rg -q "</script" docs/openrpc-viewer.min.js; then
        echo "min.js 含 </script> 字面量，禁止内嵌" >&2
        exit 1
    fi
    ls -l docs/openrpc-viewer.min.js

pre_commit:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    python3 scripts/check_layering.py
    cargo machete
    cargo fixrs
    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features

migration-generate:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    cargo run --bin migrator -- migration generate

migration-apply:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    cargo run --bin migrator -- migration apply

# Drop all tables and re-apply migrations (`migrator migration reset`; auto-confirm).
migration-reset:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    : "${DATABASE_URL:?DATABASE_URL required}"
    # dialoguer needs a TTY; allocate a PTY and answer yes.
    printf 'y\n' | script -q /dev/null cargo run --bin migrator -- migration reset

# Self-contained: migration-reset → ephemeral api (MASTER=true → seed admin) → hurl.
# Needs hurl + DATABASE_URL + Redis (CACHE_REDIS_URL) + JWT_SECRET (≥32 bytes).
# Override with E2E_LISTEN_ADDR / E2E_BASE_URL if 18080 is taken.
# Auth IP rate limits are deliberately raised: e2e hammers login/refresh from one IP.
# 使用 --no-default-features（关 otlp）加快启动；默认 otlp 路径由 CI `cargo check -p api` 覆盖。
e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    command -v hurl >/dev/null || { echo "e2e needs hurl on PATH (https://hurl.dev)"; exit 1; }
    : "${DATABASE_URL:?DATABASE_URL required}"
    : "${JWT_SECRET:?JWT_SECRET required (≥32 bytes)}"
    : "${CACHE_REDIS_URL:?CACHE_REDIS_URL required}"
    listen="${E2E_LISTEN_ADDR:-127.0.0.1:18080}"
    base_url="${E2E_BASE_URL:-http://${listen}}"
    just migration-reset
    # MASTER=true: schema is already applied by migration-reset (re-apply is a no-op)
    # and boots the seed admin used by e2e/seed_admin.hurl.
    # 关 otlp 加快启动；blob-fs 仍需显式打开。
    # BLOB_FS_PRESIGN_SECRET：与 JWT 分离的本地预签名密钥（无 .env 时给固定测试值）。
    # 固定 Redis DB 14；启动前清该库，避免 login IP 窗连坐。
    if command -v redis-cli >/dev/null; then
      redis-cli -n 14 FLUSHDB >/dev/null || true
    else
      # 同一 TCP 连接内 SELECT+FLUSHDB（分连接的 FLUSHDB 只会清 DB 0）。
      {
        printf '*2\r\n$6\r\nSELECT\r\n$2\r\n14\r\n'
        printf '*1\r\n$8\r\nFLUSHDB\r\n'
        sleep 0.1
      } | nc 127.0.0.1 6379 >/dev/null || true
    fi
    MASTER=true \
    SEED_ADMIN_PHONE=13800000777 \
    SEED_ADMIN_PASSWORD=seed-pass-1 \
    DOCS_ENABLED=true \
    RATE_LIMIT_AUTH_PER_SECOND=1000 \
    RATE_LIMIT_AUTH_BURST=2000 \
    RATE_LIMIT_LOGIN_PHONE_MAX_ATTEMPTS=1000 \
    CORS_ORIGINS=http://example.com \
    BLOB_FS_ROOT="${BLOB_FS_ROOT:-data/blob-e2e}" \
    BLOB_FS_DOMAIN="${base_url}/files" \
    BLOB_FS_PRESIGN_SECRET="${BLOB_FS_PRESIGN_SECRET:-e2e-blob-presign-secret-at-least-32b}" \
    CACHE_REDIS_URL="redis://127.0.0.1:6379/14" \
    cargo run -p api --no-default-features --features blob-fs -- --listen-addr "$listen" &
    api_pid=$!
    cleanup() { kill "$api_pid" 2>/dev/null || true; wait "$api_pid" 2>/dev/null || true; }
    trap cleanup EXIT
    for _ in $(seq 1 80); do
        if curl -sf "$base_url/healthz" >/dev/null; then
            break
        fi
        sleep 0.25
    done
    curl -sf "$base_url/healthz" >/dev/null
    # >2MiB fixture：同时大于 BODY_LIMIT_BYTES(1MiB) 与 Axum 提取器默认(~2MiB)，
    # 断言走 PublicRouter 的 PUBLIC_BODY_LIMIT_BYTES(16MiB)。
    dd if=/dev/zero of=e2e/fixtures/over-2m.bin bs=1024 count=2560 status=none
    # 单会话模型：--test 默认并行会互相吊销 access；--jobs 1 串行。
    hurl --test --jobs 1 --variable base_url="$base_url" e2e/*.hurl
