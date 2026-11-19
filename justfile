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

# Drop all tables and re-apply migrations（非交互：psql DROP SCHEMA + apply）。
# 不用 toasty `migration reset`：其 dialoguer Confirm 要 TTY，script/pty 伪终端在 CI 易挂死。
migration-reset:
    #!/usr/bin/env bash
    set -euo pipefail
    set -a && [ -f .env ] && source .env
    : "${DATABASE_URL:?DATABASE_URL required}"
    if ! command -v psql >/dev/null; then
        echo "migration-reset needs psql on PATH (CI: postgresql-client; macOS: brew install libpq && brew link --force libpq)" >&2
        exit 1
    fi
    echo "migration-reset: DROP SCHEMA public CASCADE..."
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c 'DROP SCHEMA public CASCADE; CREATE SCHEMA public;'
    migrator="${CARGO_TARGET_DIR:-target}/debug/migrator"
    if [[ ! -x "$migrator" ]]; then
        echo "migration-reset: building migrator..."
        cargo build --bin migrator
    fi
    echo "migration-reset: applying migrations..."
    "$migrator" migration apply
    echo "migration-reset: done"

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
    # 注意：Ubuntu openbsd-nc 默认会在 EOF 后一直等对端关连接 → CI 无 redis-cli 时会挂死；
    # 必须 -N/-w，或直接装 redis-tools。
    if command -v redis-cli >/dev/null; then
      redis-cli -n 14 FLUSHDB >/dev/null || true
    else
      # 同一 TCP 连接内 SELECT+FLUSHDB（分连接的 FLUSHDB 只会清 DB 0）。
      {
        printf '*2\r\n$6\r\nSELECT\r\n$2\r\n14\r\n'
        printf '*1\r\n$8\r\nFLUSHDB\r\n'
      } | nc -N -w 2 127.0.0.1 6379 >/dev/null || true
    fi
    # 先前台 build（CI 无 TTY 时后台 cargo run 会全缓冲且编译远超 healthz 等待窗），
    # 再跑 debug 二进制，避免 wait 卡在还在编译的 cargo 上十几分钟无日志。
    echo "e2e: building api (blob-fs, no otlp)..."
    cargo build -p api --no-default-features --features blob-fs
    api_bin="${CARGO_TARGET_DIR:-target}/debug/api"
    api_log="${TMPDIR:-/tmp}/jsonrpc-e2e-api.log"
    : >"$api_log"
    echo "e2e: starting $api_bin on $listen (log: $api_log)"
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
    "$api_bin" --listen-addr "$listen" >>"$api_log" 2>&1 &
    api_pid=$!
    cleanup() {
        if kill -0 "$api_pid" 2>/dev/null; then
            kill "$api_pid" 2>/dev/null || true
            for _ in $(seq 1 50); do
                kill -0 "$api_pid" 2>/dev/null || break
                sleep 0.1
            done
            kill -9 "$api_pid" 2>/dev/null || true
            wait "$api_pid" 2>/dev/null || true
        fi
    }
    trap cleanup EXIT
    ready=0
    for i in $(seq 1 80); do
        if curl -sf "$base_url/healthz" >/dev/null; then
            ready=1
            echo "e2e: api ready (${i}/80)"
            break
        fi
        if ! kill -0 "$api_pid" 2>/dev/null; then
            echo "e2e: api exited before /healthz; log:" >&2
            cat "$api_log" >&2 || true
            exit 1
        fi
        if (( i % 10 == 0 )); then
            echo "e2e: waiting for /healthz (${i}/80)..."
        fi
        sleep 0.25
    done
    if [[ "$ready" -ne 1 ]]; then
        echo "e2e: /healthz not ready after ~20s; log:" >&2
        cat "$api_log" >&2 || true
        exit 1
    fi
    # >2MiB fixture：同时大于 BODY_LIMIT_BYTES(1MiB) 与 Axum 提取器默认(~2MiB)，
    # 断言走 PublicRouter 的 PUBLIC_BODY_LIMIT_BYTES(16MiB)。
    dd if=/dev/zero of=e2e/fixtures/over-2m.bin bs=1024 count=2560 status=none
    # 单会话模型：--test 默认并行会互相吊销 access；--jobs 1 串行。
    echo "e2e: running hurl..."
    hurl --test --jobs 1 --variable base_url="$base_url" e2e/*.hurl
