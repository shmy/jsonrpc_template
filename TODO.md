# TODO — JSON-RPC 脚手架待办

基于当前仓库评估：`axum_jsonrpc` 协议核可用，应用壳与运维层仍需补齐。

## P0 — 能跑、能懂

- [x] 编写 `README.md`：快速启动、`POST /jsonrpc` 示例、`GET /openrpc.json`、`GET /healthz`、workspace 结构说明
- [x] 补充 `.env.example`（`LISTEN_ADDR`、`RUST_LOG`、`DATABASE_URL` 及连接池可选项）
- [x] `http_server` 监听地址走 `Cli` / `LISTEN_ADDR`（默认 `0.0.0.0:8080`）
- [x] 挂上 probe：`GET /livez` `/healthz` `/readyz`（经 `ProbeRouter` 合并）

## P1 — 契约与错误

- [x] 约定领域错误 → JSON-RPC `ErrorObject` 映射（避免业务失败一律 `-32603`）
      （`IdentityError` → domain mapper → `ErrorObject`；`locale` FTL + `Accept-Language` 渲染 `message`；契约靠 `code`，不传 `data`）
- [x] 在 `identity` 示范至少一个带自定义 `-320xx` 的失败路径
      （重复手机号 → `-32001`；`en-US` / `zh-CN`；DB 其它失败 → `-32603`）
- [x] OpenRPC `info` / `servers` 可配置（title、version、description、base URL）
      （`http_server` 直接传 `OpenRpcInfo` + `OpenRpcServer`）
- [x] 可选：OpenRPC Playground / 静态文档页链接说明
      （README：`schemaUrl` → `/openrpc.json`；不自建静态页）

## P2 — 安全与中间件

- [x] 示范认证：`FromRequestParts` 提取 Bearer，并接到 RPC handler
      （`PublicRpc`=login/refresh；`logout`/`create`/`change_password`/CRUD 进 `PrivateRpc`+`before_each`（create 恒 privileged=false）；update/delete 本人或 privileged；JWT + Redis；password argon2id）
- [ ] 常见/breached 密码 denylist（create / change_password）
- [x] 区分 `PublicRouter` / `PrivateRouter` 的实际用法（或文档说明何时用）
      （`ProbeRouter`：探针；`PublicRouter` / `PrivateRouter`：其它 HTTP；`PrivateRouter` 强制 `account_auth_middleware`；RPC：`PublicRpc` / `PrivateRpc` + 软鉴权中间件）
- [x] CORS、请求超时、method 内限流（login/refresh Redis 窗；全站限流放网关）；仅 master 跑 blob GC
- [x] Body/batch 限制保持可配置（`BODY_LIMIT_BYTES` / `MAX_BATCH_LEN`；默认 1 MiB / 128）

## P3 — 可观测性

- [x] RPC 级 tracing span：`method`、`id`、耗时、成功/错误码
      （单次：一层 `jsonrpc.call`+`otel.name`=method；batch：`jsonrpc.batch`+子 call；字段 `rpc.*`）
- [x] RPC 返回`trace_id`
      （仅错误：`error.data.trace_id`；`traceparent` / `x-request-id` / mint）
- [x] 接入 `rootcause-tracing`
      （`crates/trace_kit` + `bin/api` `init_tracing`；OTLP 由 api feature `otlp`）
- [x] 基础 metrics 钩子（请求数、错误数、batch 大小）——至少预留接口
      （`RpcObserver` + `trace_kit::metrics::record_rpc_*`，api feature `otlp`）

## P4 — 测试与工程

- [x] `identity` HTTP 集成测试：create 成功 + 重复手机号 `-32001` + `Accept-Language`
      （`e2e/*.hurl`；`just e2e` = `migration reset` + 临时 api + hurl；需 `hurl` + `DATABASE_URL`）
- [x] Postgres schema migration（toasty `embed_migrations`；仅 `MASTER=true` 启动 apply；`bin/migrator` + `just migration-generate` / `migration-apply`；README 已写清）
- [x] CI：`cargo fmt --check`、`clippy -D warnings`、`cargo test --workspace --all-targets`、`cargo check -p api`（默认 features）、`just e2e`
- [x] 明确 `axum_jsonrpc` 与应用模板的边界（可独立复用 vs 示例 app；见 AGENTS / README）

## P5 — 业务示范深化（可选）

- [x] `identity.account.create` 接 toasty / `identity_contract`（非内存桩）
- [x] 补齐 detail / update / delete / search（已有；另补 change_password）
- [x] 参数校验与更多失败用例（找不到账号等；见 e2e）
- [x] 方法命名与版本策略文档（点分域 `identity.account.*`；版本走 OpenRPC info.version / api crate）
- [x] 集成测试统一使用 hurl（不用 Rust `#[test]` 打 HTTP）；`just migration-reset` / `just e2e` 可清库再跑

## 明确不做 / 暂缓

- WebSocket / 订阅（除非产品需要）
- 规范 introspection（`rpc.discover` 等）——已有 OpenRPC 可替代
- TLS 终止（默认交给反向代理）
- job_queue / scheduler（file 的 staging/attachment GC 已用 Lifecycle 间隔任务）、多 cache 后端、双组合根
- MFA / 多设备会话 / OIDC
- 模板默认不收紧：登录即可全库 `read`/`search`、无注册配额的 `create`（产品化时自行加策略）

## 已完成（blob / file）

- [x] `blob_kit` + `infra/blob`（fs / 腾讯 COS）；上传仅 `file.presign`（无 multipart / image_kit）
- [x] （fs）`PUT /files/presigned` + `ServeDir`（dated 白名单 + 不可信 attachment）公有读；`BLOB_FS_PRESIGN_SECRET` 与 JWT 分离
- [x] JSON-RPC `file.presign`（COS 原生；fs 本地 HMAC PUT，客户端同形；不校验内容）
- [x] `infra/file_attachment` + identity avatar + Lifecycle GC（feature 不再互依）

## 验收标准（起步可用）

1. 新人按 README（空库 + `MASTER=true` seed 管理员）能 login → `identity.account.create` 打通一次 RPC
2. 改端口、日志级别无需改代码
3. 业务错误对客户端可见且稳定（非一律 internal）
4. `/healthz`、`/readyz` 与 `/openrpc.json` 可用
5. CI 绿、核心 crate 测试不回退
