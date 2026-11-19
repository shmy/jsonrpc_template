# JSON-RPC Template

基于 Axum + `modrun` 的 JSON-RPC 2.0 应用脚手架。协议核在 `crates/axum_jsonrpc`，OpenRPC 在 `axum_jsonrpc_openrpc`，业务以 feature 模块挂载；自带健康检查与 Bearer 登录示范。

## 快速启动

需要：Rust（edition 2024）、**可达的 Postgres**、**可达的 Redis**（≥ 6.2，会话消费用 `GETDEL`），以及 `.env` 里的 `JWT_SECRET`（≥ 32 字节）。库可为空。未设 `MASTER` 时默认 `false`；`.env.example` 写 `MASTER=true`（migrate + seed + blob GC）。也可先 `just migration-apply`。

```bash
cp .env.example .env
# 按需改 LISTEN_ADDR / RUST_LOG / DATABASE_URL / CACHE_REDIS_URL / JWT_SECRET / MASTER

cargo run -p api
# 或热重载：just dev
# 单独建表 / 升级（不启动 api）：just migration-apply
```

默认监听 `http://0.0.0.0:8080`。多实例时只让一个进程 `MASTER=true`（migrate / seed / GC），其余保持默认 `false`。

## HTTP 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/jsonrpc` | JSON-RPC 2.0（单请求或 batch）；软鉴权：有 `Authorization: Bearer` 则校验并注入 `AuthedAccount` |
| `GET` | `/openrpc.json` | OpenRPC 1.3 文档（随已注册方法生成） |
| `GET` | `/docs` | OpenRPC 文档调试页（自包含 HTML，同源读取 `/openrpc.json`，可直接发调试请求；免 CORS 配置） |
| `GET` | `/livez` | 存活探测（不查依赖），返回 `OK` |
| `GET` | `/healthz` | 存活探测，返回 `OK` |
| `GET` | `/readyz` | 就绪探测：Postgres 可取连接且 Redis `probe` 成功 → `200`，否则 `503` |
| `PUT` | `/files/presigned` | `blob-fs` 本地预签名直传（HMAC query；COS 不挂，客户端 PUT 到 Cos URL） |
| `GET` | `/files/{*path}` | `blob-fs`：`ServeDir` + dated 路径白名单；可信扩展可内联，其余 `Content-Disposition: attachment`（COS 不挂） |

### 健康检查

```bash
curl -s http://127.0.0.1:8080/healthz
# OK

curl -s http://127.0.0.1:8080/readyz
# OK（DB + Redis 可达）；否则 HTTP 503
```

### OpenRPC

```bash
curl -s http://127.0.0.1:8080/openrpc.json | jq .
```

文档含 `info`（title / version / description）与 `servers`（对外 RPC URL），在 `http_server` 组装时直接传入。可用 [OpenRPC Playground](https://playground.open-rpc.org/) 浏览：把 `schemaUrl` 指到可访问的 `/openrpc.json`（本机需公网可达或隧道；也可先下载 JSON 再导入）：

```text
https://playground.open-rpc.org/?schemaUrl=https://example.com/openrpc.json
```

Playground 发起调用时会使用文档里的 `servers[].url`（模板默认 `http://127.0.0.1:8080/jsonrpc`）。

本地可设 `DOCS_ENABLED=true` 后打开 `http://127.0.0.1:8080/docs`——服务端自带的文档调试页（引擎在 `docs/openrpc-viewer.js`，零依赖单文件 JS，`/docs` 内嵌同源托管）：渲染全部方法文档，并支持在线发调试请求（Bearer Token、`Accept-Language`、`trace_id` 展示、一键复制 curl、batch），无需 Playground / 公网隧道。该 JS 也可被任意静态页面 `<script src>` 引入（用 `data-url` 指定 schema 地址），但与 API 不同源时需配置 `CORS_ORIGINS`。迭代 viewer 的上下文与硬性约束见 `docs/README.md`。

### JSON-RPC 示例（identity）

示范方法：`identity.account.*`（create / read / update / delete / search / **login / refresh / logout**；Postgres + toasty；password argon2id；会话在 Redis）。

公开 RPC（`PublicRpc`）：`identity.account.{login,refresh}`。其余方法进 `PrivateRpc`，由 `http_server` 在 merge 时统一 `before_each` 鉴权（缺 token → `-32020`）。`/jsonrpc` 仍为软鉴权中间件；`create` 需登录且恒为 `privileged=false`（管理员仅 seed；模板未做注册配额，adopters 按需收紧）；`read`/`search` 任意登录可查全库（含手机号等字段，密码/`privileged` 不返回）；`update`/`delete` 本人或 `privileged` 可操作他人（`privileged` 账号禁止删除）。

默认 seed（`.env.example` / `cargo run`）：`SEED_ADMIN_PHONE=13888888888`、`SEED_ADMIN_PASSWORD=admin123!`。  
`just e2e` 会注入另一套：`13800000777` / `seed-pass-1`（并先清库）——勿与默认 curl 混用。

```bash
# 登录 → access_token + refresh_token（公开；首个管理员见 SEED_ADMIN_*）
curl -s http://127.0.0.1:8080/jsonrpc \
  -H 'content-type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "identity.account.login",
    "params": {
      "phone": "13888888888",
      "password": "admin123!"
    }
  }'

# 创建账号（需 Bearer；结果恒 privileged=false）
curl -s http://127.0.0.1:8080/jsonrpc \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer <access_token>' \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "identity.account.create",
    "params": {
      "name": "Alice",
      "phone": "13800000001",
      "password": "secret123"
    }
  }'

# 读账号 / 登出等（需 Bearer）
curl -s http://127.0.0.1:8080/jsonrpc \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer <access_token>' \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "identity.account.logout"
  }'
```

业务错误示范（**同一手机号再次 create** → 自定义 `-32001`）。`message` 随 `Accept-Language` 在 `en` / `zh-CN` 间切换。identity 号段：`-32001` 重复、`-32002` 不存在、`-32003` 凭据错误、`-32004` refresh 无效、`-32005` 登录/刷新限流（login 与 auth 文案区分、同码）、`-32006` 新旧密码相同、`-32008` 原密码错误、`-32009` 越权（跳过协议占用的 `-32007`）。会话基础设施错误走 authn 号段（码段与 FTL 由 `http_auth` 在 `http_server` seal 前注册，`authn_kit` 不感知 RPC）：`-32020` access 缺失、`-32021` 无效、`-32022` 已吊销、`-32023` 缓存不可用。协议核另占用 `-32007`（body 过大）、`-32010`（batch 过长）。

客户端按 `code` 分支即可；业务层不传自定义 `data`。框架在**错误**响应上注入 `error.data.trace_id`。成功响应不加 `trace_id`。

Batch 请求传 JSON 数组即可；通知（无 `id`）不返回 body。

### PublicRouter / PrivateRouter / ProbeRouter / PublicRpc / PrivateRpc

- `ProbeRouter`：`/livez` `/healthz` `/readyz`；合并在 timeout 业务树之外
- `PublicRouter`：无需登录的业务 HTTP（示范：`blob-fs` 下 `PUT /files/presigned` + `GET /files/{*path}`）；默认 body 上限 16 MiB（`PUBLIC_BODY_LIMIT_BYTES`）
- `PrivateRouter`：强制 Bearer 中间件（HTTP 401）；当前模板无业务 HTTP，可按需扩展
- `PublicRpc`：匿名 JSON-RPC（login / refresh）
- `PrivateRpc`：需登录的 JSON-RPC；merge 时套 `before_each(require_authed_account)`。示范含 `file.presign`（COS 原生直传；fs 为本服务 HMAC `PUT /files/presigned`，客户端流程一致）。扩展名白名单由 `BLOB_ALLOWED_EXTENSIONS` 配置（默认 webp/jpg/jpeg/png/gif/mp4/webm）；**不**校验文件魔数（Content-Type 由 `mime_guess` 按扩展名解析）

中间件层序（外→内）：OTel（可选）→ CORS（`CORS_ORIGINS` 非空）→ locale → timeout → 软鉴权/`/jsonrpc`。探针在 timeout 业务树之外合并。限流：**无应用全站 IP 窗**（裸跑时公开面不按 IP 限流）；login/refresh 在 method 内 Redis 窗；生产全站限流放网关/WAF。

## 配置

通过环境变量或 CLI（`clap`，启动时加载 `.env`）。`RUST_LOG` 经 `trace_kit::init_tracing` 接到 `EnvFilter`。

| 变量 | 默认 | 说明 |
|------|------|------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | HTTP 监听地址 |
| `RUST_LOG` | `info` | 日志级别 |
| `MASTER` | `false`（未设 env）；`.env.example` 示例为 `true` | `true` 时 migrate / seed / blob GC |
| `DOCS_ENABLED` | `false` | 是否挂载 `GET /docs` 文档调试页 |
| `REQUEST_TIMEOUT` | `60s` | 请求超时（408 + locale） |
| `BODY_LIMIT_BYTES` | `1048576` | RPC / PrivateRouter body 上限 |
| （固定）`PUBLIC_BODY_LIMIT_BYTES` | `16 MiB` | PublicRouter 统一上限（含 `/files/presigned`；见 `http_routing`） |
| `MAX_BATCH_LEN` | `128` | JSON-RPC batch 最大条数 |
| `CORS_ORIGINS` | 空 | 逗号分隔 Origin；空 = 不加 CORS |
| `TRUSTED_PROXIES` | 空 | 可信反代 CIDR/IP；非空才解析 XFF。**反代后必须配置**，否则 login/refresh IP 窗按共享出口连坐 |
| `RATE_LIMIT_AUTH_*` | `5`/`20` | login/refresh Redis IP 滑动窗（identity `rate_limit`） |
| `RATE_LIMIT_LOGIN_PHONE_*` | `5m`/`10` | login 按手机号固定窗（成功也计数；e2e 会调高防跨轮连坐） |
| `OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC；**空串**关闭导出，仍保留 in-process tracer（console + span/`trace_id`） |
| `OTLP_SERVICE_NAME` | `jsonrpc-template` | OTel service.name |
| `OTLP_METADATA` | `{}` | gRPC metadata JSON（如 Authorization） |
| `OTLP_SAMPLER_RATIO` | `0.1` | Trace 采样率 `(0, 1]`（**只影响 trace**，不影响日志） |
| `DATABASE_URL` | （必填） | Postgres 连接串 |
| `DATABASE_MAX_CONNECTIONS` | 见 `.env.example` | 连接池最大连接数（当前 toasty 仅支持此项） |
| `JWT_SECRET` | （必填，≥32 字节） | HS256 签名密钥 |
| `JWT_ISSUER` | `jsonrpc-template` | access token `iss` |
| `JWT_AUDIENCE` | `account` | access token `aud` |
| `ACCESS_TOKEN_TTL` | `15min` | access + jti TTL |
| `REFRESH_TOKEN_TTL` | `30d` | refresh / used 标记 TTL |
| `CACHE_REDIS_URL` | `redis://127.0.0.1:6379` | 会话缓存 |
| `BLOB_FS_ROOT` | `data/blob` | `blob-fs` 本地落盘根 |
| `BLOB_FS_DOMAIN` / `BLOB_FS_PRESIGN_SECRET` | （无默认，feature `blob-fs` **必填**） | 公网前缀、HMAC（≥32；**勿**复用 `JWT_SECRET`） |
| `BLOB_ALLOWED_EXTENSIONS` | `webp,jpg,jpeg,png,gif,mp4,webm` | `file.presign` 扩展名白名单（MIME 由 `mime_guess` 解析） |
| `BLOB_S3_DOMAIN` / `ENDPOINT` / `BUCKET` / `SECRET_ID` / `SECRET_KEY` | （无默认，feature `blob-cos` **必填**） | 腾讯 COS 直传配置 |
| `SEED_ADMIN_PHONE` / `SEED_ADMIN_PASSWORD` | 见 `.env.example` | `MASTER=true` 时种子管理员（phone 已存在则跳过；被普通账号占用时仅告警） |

改端口 / 日志级别 / OTLP / JWT / Redis / Blob 只需改 `.env`，无需改代码。完整示例见 [`.env.example`](.env.example)。

关闭 OTLP：`cargo run -p api --no-default-features --features blob-fs`（仍有 console + rootcause-tracing；**须**保留 `blob-fs` 或改开 `blob-cos`）。生产 COS：`cargo run -p api --no-default-features --features blob-cos`。

## Workspace 结构

```
bin/api                 # 进程入口：组装 module 并启动
crates/
  authn_kit             # 会话 key / AuthnError / Verifier
  axum_jsonrpc          # JSON-RPC 协议核
  axum_jsonrpc_macros   # #[rpc("method.name")] 宏
  axum_jsonrpc_openrpc  # OpenRPC 文档生成与 /openrpc.json
  blob_kit              # OpenDAL 对象存储门面（fs / 腾讯 COS）
  cache_kit             # Redis Cache 门面
  jwt_kit               # HS256 TokenHelper
  trace_kit             # tracing 初始化 / trace_id / OTLP metrics
features/
  file                  # file.presign +（fs）本地 PUT/回源 + Lifecycle GC（仅 master）
  file_contract         # BlobObject 模型与 FileError（-32032..）
  identity              # 示范业务：account CRUD + login/refresh/logout（avatar 经 file_attachment）
  identity_contract     # Account 模型与领域错误
  probe                 # 运维 HTTP：/livez、/healthz、/readyz（ProbeRouter）
infra/
  app_state             # AppState（Db + Cache + Blob + TokenHelper + …）
  blob                  # OpenDAL Operator provide（feature：fs / cos）
  file_attachment       # staging claim / commit / attach / detach
  cache                 # Redis 连接池 provide
  config                # Cli / dotenv / TrustedProxies
  http_auth             # Bearer、软/强制鉴权、AuthedAccount
  http_extractors       # ClientIp（TRUSTED_PROXIES）
  http_routing          # Public/Private/Probe + PublicRpc/PrivateRpc
  http_server           # 组装 AppState、合并路由、中间件、监听
  persistence           # Postgres Db；MASTER 时 migrate
  locale                # Fluent + Accept-Language
bin/migrator            # toasty-cli：migration generate / apply
toasty/                 # embedded migration 产物
```

**扩展业务：** 抄写 [`docs/feature-template/`](docs/feature-template/)（最小依赖），细则与禁令见 [`AGENTS.md`](AGENTS.md)。blob 后端只切 `api` 的 `blob-fs` / `blob-cos`（[`docs/blob-backends.md`](docs/blob-backends.md)）。

## 开发命令

```bash
just db-up                # docker compose：Postgres + Redis
just check-fast identity  # 叶子迭代：cargo check -p <crate>（固定形态，勿与 --all-features 交替）
just check                # cargo check -p api（默认 otlp + blob-fs）
just layering             # feature/infra/crate 分层硬违规
just lint identity        # 单 crate：fmt check + clippy
just pre_commit           # layering + machete + fixrs + fmt + clippy
just migration-generate   # 从模型生成 migration
just migration-apply      # 对 DATABASE_URL apply pending migrations
cargo test --workspace --all-targets  # 单元 + seal 等（不含 HTTP e2e）
cargo run -p api          # 另开终端
just migration-reset      # `migrator migration reset`（清库并重 apply；脚本内自动确认）
just e2e                  # reset → 临时 api → hurl（需 hurl、Postgres、Redis、JWT_SECRET）
```

方法命名：点分域 `identity.account.create`；API 版本取 `api` 包版本写入 OpenRPC `info.version`（不在方法名里嵌版本前缀）。

## 路线图

待办与优先级见 [`TODO.md`](TODO.md)。
