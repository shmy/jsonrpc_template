set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set dotenv-load := true

# ── 日常 ──────────────────────────────────────────────

dev:
    cargo watch -x "run -p api"

db-up:
    docker compose up -d

db-down:
    docker compose down

# 客户端生成物对齐 + TS 会话单测（需 bun；OpenRPC 用 .clients/openrpc.json）
clients-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -q -p gen_mobile_clients
    if ! git diff --quiet -- .clients/; then
      echo "error: .clients/ drifted from OpenRPC; commit just gen-clients output" >&2
      git diff --stat -- .clients/ >&2
      exit 1
    fi
    command -v bun >/dev/null || { echo "need bun for session.test.ts" >&2; exit 1; }
    (cd .clients/typescript && bun session.test.ts)

# fmt check + clippy（单 crate）
lint crate:
    cargo fmt --all -- --check
    cargo clippy -p {{crate}} --all-targets

check:
    cargo check -p api

# 叶子迭代；勿与 --all-features / 全 workspace 混用（会触发 feature 统一化）
check-fast crate="identity":
    cargo check -p {{crate}}

layering:
    python3 scripts/check_layering.py

# 拉取 /openrpc.json → .clients/（Swift/Kotlin/Dart/TS；api 需在听；已有缓存可只跑后半段）
gen-clients url="http://127.0.0.1:9000/openrpc.json":
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p .clients
    if curl -fsS --connect-timeout 2 "{{url}}" -o .clients/openrpc.json; then
      echo "fetched {{url}} → .clients/openrpc.json"
    elif [[ -f .clients/openrpc.json ]]; then
      echo "warn: fetch failed; using cached .clients/openrpc.json" >&2
    else
      echo "error: cannot fetch {{url}} and no .clients/openrpc.json" >&2
      exit 1
    fi
    cargo run -p gen_mobile_clients

# 改 docs/openrpc-viewer.js 后重生成 min.js（需 node/npx）
docs-min:
    #!/usr/bin/env bash
    set -euo pipefail
    npx -y terser@5 docs/openrpc-viewer.js --compress passes=2 --mangle --comments false --ecma 2017 -o docs/openrpc-viewer.min.js
    node --check docs/openrpc-viewer.min.js
    if rg -q '</script' docs/openrpc-viewer.min.js; then
      echo 'min.js 含 </script> 字面量，禁止内嵌' >&2
      exit 1
    fi
    ls -l docs/openrpc-viewer.min.js

pre_commit:
    python3 scripts/check_layering.py
    cargo machete
    cargo fixrs
    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features

# ── 迁移 ──────────────────────────────────────────────

migration-generate:
    cargo run --bin migrator -- migration generate

migration-apply:
    cargo run --bin migrator -- migration apply

# 非交互清库：psql DROP SCHEMA + apply（需 psql；不用 toasty dialoguer）
migration-reset:
    #!/usr/bin/env bash
    set -euo pipefail
    log() { printf '%s\n' "$*" >&2; }
    : "${DATABASE_URL:?DATABASE_URL required}"
    command -v psql >/dev/null || { log "need psql (CI: postgresql-client; macOS: brew install libpq)"; exit 1; }
    log "migration-reset: DROP SCHEMA public CASCADE..."
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c 'DROP SCHEMA public CASCADE; CREATE SCHEMA public;'
    migrator="${CARGO_TARGET_DIR:-target}/debug/migrator"
    if [[ ! -x "$migrator" ]]; then
      log "migration-reset: building migrator..."
      cargo build --bin migrator
    fi
    log "migration-reset: apply via $migrator..."
    "$migrator" migration apply
    log "migration-reset: done"

# ── e2e ───────────────────────────────────────────────
# 需 hurl + psql + DATABASE_URL + CACHE_REDIS_URL + JWT_SECRET（≥32）
# E2E_LISTEN_ADDR / E2E_BASE_URL 可覆盖默认 18080；关 otlp，默认 otlp 由 CI check 覆盖

e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    log() { printf '%s\n' "$*" >&2; }
    command -v hurl >/dev/null || { log "need hurl (https://hurl.dev)"; exit 1; }
    : "${DATABASE_URL:?DATABASE_URL required}"
    : "${JWT_SECRET:?JWT_SECRET required (≥32 bytes)}"
    : "${CACHE_REDIS_URL:?CACHE_REDIS_URL required}"
    listen="${E2E_LISTEN_ADDR:-127.0.0.1:18080}"
    base_url="${E2E_BASE_URL:-http://${listen}}"
    api_bin="${CARGO_TARGET_DIR:-target}/debug/api"
    api_log="${TMPDIR:-/tmp}/jsonrpc-e2e-api.log"

    log "e2e: [1/5] migration-reset"
    just migration-reset

    # DB 14 避免 login IP 窗连坐；nc 必须 -N/-w，否则会挂等 Redis 关连接
    log "e2e: [2/5] flush redis DB 14"
    if command -v redis-cli >/dev/null; then
      redis-cli -n 14 FLUSHDB >/dev/null || true
    else
      printf '*2\r\n$6\r\nSELECT\r\n$2\r\n14\r\n*1\r\n$8\r\nFLUSHDB\r\n' \
        | nc -N -w 2 127.0.0.1 6379 >/dev/null || true
    fi

    log "e2e: [3/5] build + start api on $listen"
    cargo build -p api --no-default-features --features blob-fs
    : >"$api_log"
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
      kill "$api_pid" 2>/dev/null || true
      for _ in $(seq 1 50); do kill -0 "$api_pid" 2>/dev/null || break; sleep 0.1; done
      kill -9 "$api_pid" 2>/dev/null || true
      wait "$api_pid" 2>/dev/null || true
    }
    trap cleanup EXIT

    log "e2e: [4/5] wait /healthz (pid=$api_pid)"
    ready=0
    for i in $(seq 1 80); do
      if curl -sf "$base_url/healthz" >/dev/null; then
        ready=1
        log "e2e: api ready (${i}/80)"
        break
      fi
      if ! kill -0 "$api_pid" 2>/dev/null; then
        log "e2e: api exited before /healthz; log:"
        cat "$api_log" >&2 || true
        exit 1
      fi
      if (( i % 10 == 0 )); then
        log "e2e: waiting for /healthz (${i}/80)..."
      fi
      sleep 0.25
    done
    if [[ "$ready" -ne 1 ]]; then
      log "e2e: /healthz not ready after ~20s; log:"
      cat "$api_log" >&2 || true
      exit 1
    fi

    # >2MiB：越过 BODY_LIMIT(1MiB) 与 Axum 默认(~2MiB)，走 PUBLIC_BODY_LIMIT(16MiB)
    dd if=/dev/zero of=e2e/fixtures/over-2m.bin bs=1024 count=2560 status=none
    # 单会话模型：并行会互相吊销 → --jobs 1
    log "e2e: [5/5] hurl --test --jobs 1"
    hurl --test --jobs 1 --variable base_url="$base_url" e2e/*.hurl
    log "e2e: all passed"
