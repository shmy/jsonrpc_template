# TODO — JSON-RPC 脚手架待办

基于当前仓库评估：`axum_jsonrpc` 协议核可用，应用壳与运维层已齐。

## P0 — 能跑、能懂

- [x] 编写 `README.md`：快速启动、`POST /jsonrpc` 示例、`GET /openrpc.json`、`GET /healthz`、workspace 结构说明
- [x] 补充 `.env.example`（`LISTEN_ADDR`、`RUST_LOG`、`DATABASE_URL` 及连接池可选项）
- [x] `http_server` 监听地址走 `Cli` / `LISTEN_ADDR`（默认 `0.0.0.0:8080`）
- [x] 挂上 probe：`GET /livez` `/healthz` `/readyz`（经 `ProbeRouter` 合并）

## P1 — 契约与错误

- [x] 约定领域错误 → JSON-RPC `ErrorObject` 映射（避免业务失败一律 `-32603`）
- [x] 在 `identity` 示范至少一个带自定义 `-320xx` 的失败路径
- [x] OpenRPC `info` / `servers` 可配置（title、version、description、base URL）
- [x] 可选：OpenRPC Playground / 静态文档页链接说明

## P2 — 安全与中间件

- [x] 示范认证：`FromRequestParts` 提取 Bearer，并接到 RPC handler
- [ ] ~~常见/breached 密码 denylist（create / change_password）~~ **不做**
- [x] 区分 `PublicRouter` / `PrivateRouter` 的实际用法（或文档说明何时用）
- [x] CORS、请求超时、method 内限流（login/refresh Redis 窗；全站限流放网关）；仅 master 跑 blob GC
- [x] Body/batch 限制保持可配置（`BODY_LIMIT_BYTES` / `MAX_BATCH_LEN`；默认 1 MiB / 128）
- [x] refresh 重放宽限 `REFRESH_GRACE_TTL`（默认 30s）

## P3 — 可观测性

- [x] RPC 级 tracing span：`method`、`id`、耗时、成功/错误码
- [x] RPC 返回关联 id（响应头 `x-request-id`，成功/失败都回）
- [x] 接入 `rootcause-tracing`
- [x] 基础 metrics 钩子（`RpcObserver` + api feature `otlp`）

## P4 — 测试与工程

- [x] `identity` HTTP 集成测试（`e2e/*.hurl`；`just e2e`）
- [x] Postgres schema migration（toasty；仅 `MASTER=true` 或 `just migration-apply`）
- [x] CI：fmt / clippy / test / deny / e2e / clients-check
- [x] 明确 `axum_jsonrpc` 与应用模板的边界
- [x] [`docs/production.md`](docs/production.md)（反代 / 限流 / 多实例；compose 仅本地依赖）

## P5 — 业务示范深化

- [x] `identity.account.*` 接 toasty / 会话 / change_password
- [x] 方法命名与版本策略文档

## 明确不做 / 暂缓

- WebSocket / 订阅（除非产品需要）
- 规范 introspection（`rpc.discover` 等）——已有 OpenRPC 可替代
- TLS 终止（默认交给反向代理）
- 多 cache 后端、双组合根（独立 worker 二进制）
- COS 数据万象 / 独立图片服务（当前 fs 同进程 `?w=`）
- MFA / 多设备会话 / OIDC
- 常见/breached 密码 denylist
- 账号软删 / 独立审计事件表
- 模板默认不收紧：登录即可全库 `read`/`search`、无注册配额的 `create`（产品化时自行加策略）

## 已完成（blob / file / clients）

- [x] `blob_kit` + `infra/blob`（fs / 腾讯 COS）；上传仅 `file.presign`
- [x] （fs）`PUT /files/presigned` + `GET /files/*`（可选 `?w=128|256|512` 派生 WebP 落盘缓存）
- [x] `infra/file_attachment` + identity avatar + Lifecycle GC
- [x] `image_kit` + fs URL 派生缩略图（`*_w{n}.webp` 缓存；旧 `_sm.webp` GC 仍清）
- [x] 多语言客户端生成 + CI `clients-check`（OpenRPC 漂移 + bun session.test）

## 验收标准（起步可用）

1. 新人按 README（空库 + `MASTER=true` seed 管理员）能 login → `identity.account.create` 打通一次 RPC
2. 改端口、日志级别无需改代码
3. 业务错误对客户端可见且稳定（非一律 internal）
4. `/healthz`、`/readyz` 与 `/openrpc.json` 可用
5. CI 绿、核心 crate 测试不回退
