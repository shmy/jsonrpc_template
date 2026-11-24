# AGENTS.md — JSON-RPC Template

给在本仓库工作的编码代理：优先遵循本文，细节以代码与 `README.md` / `TODO.md` 为准。

**快速入口：** 启动/端点 → [`README.md`](README.md)；加业务 → [`docs/app-template/`](docs/app-template/)（`just new-app <name>`）；blob 后端切换 → [`docs/blob-backends.md`](docs/blob-backends.md)；分层 CI → `just layering`。

## 项目是什么

Axum + `modrun` 的 **JSON-RPC 2.0 应用脚手架**。

- **可复用协议核**：`crates/axum_jsonrpc*`（可单独依赖）
- **应用模板**：`bin/app` + `app/` + `contracts/` + `infra/`（预留 `ops/` 与 `bin/ops`，尚未建）

默认端点：`POST /jsonrpc`、`GET /openrpc.json`、`GET /livez`、`GET /healthz`、`GET /readyz`、（`blob-fs`）`PUT /files/presigned` + `GET /files/{*path}`（可选 `?s=128|256|512|1024`）；上传走 JSON-RPC `file.presign`。

## 分层与依赖方向（必须遵守）

```text
bin/app                    # C 端组合根：CLI supply + module 注册
  → app/*                  # C 端表面——彼此不互依；勿依赖未来 ops/*
  → contracts/*            # 跨表面契约（模型等）；app/ops 唯一可依赖的兄弟层
  → infra/*                # 应用壳（http_server、persistence、blob、file_attachment、…）
  → crates/axum_jsonrpc*   # 协议核（不要反向依赖 app/ops / 模板 infra）
  → crates/{account_id,blob_kit,trace_kit,…}
# 预留：bin/ops → ops/* → contracts/* → infra → crates（同纪律；目录尚未建）
```

| 目录 | 职责 | 改动频率 |
|------|------|----------|
| `crates/axum_jsonrpc*` | 解析、batch、limits、Handler、Observer、trace_id；`RpcRouter` / `ReadyRpcRouter` | 低（宽依赖，API 求稳） |
| `crates/blob_kit` | OpenDAL 对象存储门面（fs / 腾讯 COS / test-utils） | 低 |
| `infra/app_state` | 进程级 `AppState`（`Db` + `Cache` + `Blob` + `TokenHelper` + `TrustedProxies` + `AuthRateLimit` / `LoginPhoneRate` + `FromRef`） | 中（加资源时扩展） |
| `infra/blob` | OpenDAL `Blob` provide（feature `fs` / `cos`；cos 优先；fs→`config/fs`：`BLOB_FS_ROOT` 有默认，`DOMAIN`/`PRESIGN_SECRET` 必填；cos→`config/cos` 必填 `BLOB_S3_*`） | 低 |
| `infra/file_attachment` | staging claim / commit / attach / detach + **通用绑定编排**（`bind` / `AttachmentWrite` fresh-stale 引用计数协议；跨业务附件实现，勿让表面互依；identity avatar 为薄适配示范）；缩略图 URL 约定 `?s=`（见 `thumb_public_url`；**仅正式 path**，`staging/` 禁止派生）；purge 清派生缓存 | 中 |
| `crates/image_kit` | 纯图像派生（decode → max-edge → webp）；fs `GET ?s=` 内 `spawn_blocking` 调用 | 低 |
| `infra/cache` | Redis `Cache`（modrun provide） | 低 |
| `infra/http_auth` | Bearer 解析、软/强制鉴权中间件、`AuthedAccount`；authn 错误码段（`-32020..-32029`）+ FTL + mapper 自持，由 `http_server` seal 前统一注册（**勿**在表面 `module()` 里再注册） | 低 |
| `infra/http_routing` | `PublicRpc` / `PrivateRpc` / `PublicRouter` / `PrivateRouter` / `ProbeRouter`（无限流中间件） | 低 |
| `infra/http_extractors` | `ClientIp` + `TRUSTED_PROXIES` 解析 | 低 |
| `infra/persistence` | DB 连接；提供进程级 `Db`；注册 `*_contract` 模型；仅 `MASTER=true` 时 apply embedded migrations（默认 `false`） | 中 |
| `infra/http_server` | `Db`+`Cache`+`Blob`+JWT → `AppState`；合并路由、监听、OpenRPC info、中间件、`with_state` | 中 |
| `crates/account_id` | 账号主键 `Id`（线格式 string / 内存 u64）；壳层与业务直接依赖，contract 不 re-export | 低 |
| `crates/{authn_kit,jwt_kit,cache_kit}` | 鉴权 key/错误、HS256 JWT、Redis 缓存门面 | 低 |
| `app/file` | `file.presign`、（fs）本地预签名 PUT / 回源 / `?s=`（`shared/image_derive` + `endpoints/file_get`）、Lifecycle GC（仅 master） | 中 |
| `app/*` | C 端业务 RPC / HTTP | **高（日常应落这里）** |
| `ops/*` | 管理端表面（**预留**，尚未建目录） | — |
| `contracts/*` | 跨表面契约（toasty 模型等）；**不**依赖 app/ops 实现 | 中 |
| `bin/app` | C 端组装入口（默认 `otlp`+`blob-fs`；可切 `blob-cos`） | 低 |
| `bin/migrator` | toasty migration CLI（generate / apply；不经 app 启动） | 低 |
| `bin/gen_mobile_clients` | 从 OpenRPC 生成移动端客户端 | 低 |

**禁止（`just layering` / CI 会拦硬违规）：**

- 让 `axum_jsonrpc` 依赖 `app`/`ops`、`http_server`、业务类型
- 把 `PublicRpc` / `PrivateRpc` 塞回协议核（它们属于模板侧 `http_routing`）
- app/ops 依赖 `http_server`（用 `app_state` / `http_routing` / `axum_jsonrpc` / `locale`）
- **app/ops 表面实现互依禁止**——只能依赖 `contracts/*`（例：可用 `identity_contract`，禁止依赖 `identity`）。附件实现走 `infra/file_attachment`（通用绑定编排 `file_attachment::bind` + fresh/stale 时序；identity `avatar_ops` 为薄适配示范，新业务勿复制粘贴它的实现）
- `contracts/*` 依赖任何 app/ops 实现，或互相堆业务逻辑（契约保持薄：**wire 形态（新类型/schema/稳定错误码）+ 跨表面共享的纯校验/路径约定**；含副作用的编排与 IO 落表面或 infra）。staging 前缀权威在 `blob_kit::STAGING_PREFIX`；`file_contract::STORAGE_DIR_PREFIX` 为镜像，由 `file` 单测锁死相等
- infra 依赖 `contracts/*`——**仅允许下列例外**（冻结；新增须改 `scripts/check_layering.py` + 本表并写明理由）：
  1. `persistence`：`toasty::models!(…)` **注册**各 `*_contract` 模型（落库壳，不写业务）
  2. `file_attachment`：域错误 / 路径约定借用 `file_contract`（附件实现与 file 契约对齐，勿让表面互依）
- 账号主键在 `crates/account_id`（`Id`）；`identity_contract` **不** re-export。`http_auth` / `file_attachment` / identity 模型与方法 **直接依赖 `account_id`**。出现**第二个自有主键主体**时再考虑泛型 `AuthedAccount` / subject 注册表——现在不做。
- `crates/` 里的工具 kit（`account_id` / `authn_kit` / `jwt_kit` / `cache_kit` / `blob_kit` / `image_kit` / `trace_kit`）保持协议与业务无关：不定义 `-320xx` RPC 码段、不依赖 `locale`——码段分配与文案归使用它的 infra / 表面壳层（例：authn 在 `http_auth`；file 在 `file_contract`）。`blob_kit`：Fs 本地 HMAC 在 `local_presign` / `Fs`；门面只提供 `as_fs()`，勿再挂 `verify_local_*`

## AppState 所有权（易错）

1. 表面用 **`RpcRouter::<AppState>::new()`** 注册方法（**仅方法表**，不要 `with_state`）
2. `persistence::module()` 异步提供**进程级** `Db`；`cache::module()` 提供 Redis `Cache`；`blob::module()` 提供 `Blob`；仅当 **`MASTER=true`** 时 apply embedded migrations、seed 管理员、启动 file staging/attachment GC（`MASTER` 默认 `false`；多实例只开一个 master，或用 `just migration-apply`）
3. `http_server` 将 `Db` + `Cache` + `Blob` + `TokenHelper` + `TrustedProxies` + 限流配额包成 `AppState`（`AuthRateLimit` / `LoginPhoneRate`，来自 `RATE_LIMIT_*`）；`merge_rpc` 合并 `PublicRpc` +（`PrivateRpc` 经 `before_each(require_authed_account)`）后 **`.with_state(process_state)`** → `ReadyRpcRouter`；`/jsonrpc` 挂软鉴权（有 Bearer 则验签写 `AuthedAccount`/`AuthnError`）；`PrivateRouter` 挂强制鉴权（当前无贡献路由，预留）。Body limit：**`BODY_LIMIT_BYTES`（默认 1 MiB）只套 RPC + PrivateRouter**；**PublicRouter 统一 `PUBLIC_BODY_LIMIT_BYTES`（16 MiB）**。公开写接口若要更严，在路由上挂**更小**的 `DefaultBodyLimit`；切勿用更小的外层 limit 包住更大上传路由。限流：无全站中间件；贵路径在 method 内调 `AuthRateLimit::allow` / `LoginPhoneRate::allow` / `Cache::allow_sliding_window`（示范 identity `rate_limit`，配额可配勿硬编码）；生产全站限流放网关

表面路由器上的 state 在 merge 时**不会带上**；不要在表面里定制「自以为会生效」的 AppState。

共享资源（池、仓储）放进 `AppState`，用 `State<AppState>` 或 `State<T: FromRef<AppState>>` 提取。保持 `app_state` crate **轻量**（app/ops 会依赖它）。

会话（JWT jti / refresh digest）跨多个 Redis key **无事务**：`issue_tokens` 部分写入失败时可能短暂半开会话；下次 login/refresh 会覆盖 subject 指针并清理旧 digest。每账号单会话模型。refresh **fail-closed**，分两段：
- **commit 前**并发 `consume`：仅一人 `take` 成功；败者拿不到 `used_*` 时只得 `-32004`，**不**吊销胜者（见 `token_ops` 单测 `concurrent_refresh_does_not_revoke_winner`）
- **轮换完成后**再用旧 refresh：命中 `used_*` → 吊销该账号全部会话（`-32004`），需重新 login
旧 refresh 重放宽限：`REFRESH_GRACE_TTL`（默认 `30s`；`0` 关闭）。宽限内重放返回缓存的新 pair，不吊销；过期后仍 fail-closed 吊销。

Schema 变更：改 `*_contract` 模型 → `just migration-generate` → 提交 `toasty/` → master 启动或 `just migration-apply`。HTTP 集成测试用 `e2e/*.hurl`（`just e2e` 会清库再跑）；勿用 Rust `#[test]` 打 `/jsonrpc`。

## 扩展 app 表面

**接线清单（权威、一屏）：** [`docs/app-template/README.md`](docs/app-template/README.md)。  
脚手架：`just new-app <name>`（复制模板并打印下一步）。默认依赖勿照抄 `identity`。管理面将来用 `ops/` + `bin/ops`（尚未建）。

### 决策树（何时加什么）

```text
需要落库 / 跨表面共享类型？
  → contracts/<name>_contract + persistence models!（仅 contract，勿依赖兄弟表面实现）
需要当前登录账号？
  → http_auth::AuthedAccount；登录门槛靠进 PrivateRpc（勿对 /jsonrpc 硬鉴权）
匿名 RPC（login/refresh）？
  → PublicRpc；其余默认 PrivateRpc
领域错误？
  → locale::register_locale_bundle（FTL+mapper 成对；module 在 http_server 前）
持有正式 blob 引用？
  → file_attachment::bind + register_attachment_in_use（排在 file 前）
非 RPC HTTP？
  → ProbeRouter / PublicRouter / PrivateRouter（见 probe、file）
跨表面复用实现（非模型）？
  → 抽 infra/* 或下沉 contract；禁止 use 兄弟表面实现
```

### 目录约定

```text
app/<name>/
  lib.rs                 # module()、provide_*、Lifecycle；不写业务细节
  methods/mod.rs         # 有 RPC 时必有；声明各 method 子模块
  methods/<action>.rs    # 单文件 ≈ 一个 RPC（req/resp + handler）
  endpoints/mod.rs       # 仅当有非 RPC HTTP 时
  endpoints/<route>.rs
  shared/mod.rs          # 辅助一律进这里（含 l10n、GC、access、ops…）
  shared/<helper>.rs
  locales/{en-US,zh-CN}/*.ftl
```

- **根目录只留** `lib.rs` + 可选的 `methods/` / `endpoints/` / `shared/` / `locales/`
- **子模块声明统一** `foo/mod.rs`；`methods` / `endpoints` / `shared` 下子模块 `pub(crate) mod`
- **`shared/l10n.rs`** 固定；无 HTTP 不建 `endpoints/`；无 RPC 不建 `methods/`
- 方法名点分域：`identity.account.create`；Handler 可用 `Params` / `State` / `FromRequestParts`

**默认 Cargo 依赖**：`app_state` + `http_routing` + `axum_jsonrpc` + `locale` + 自己的 `*_contract`。按需：`http_auth`、`http_extractors`。

## 错误与 locale

客户端按稳定 **`code` 分支**。号段（勿碰撞）：

| 号段 | 归属 | 说明 |
|------|------|------|
| `-32700` / `-32600..-32603` | JSON-RPC 标准 | parse / invalid / method / params / internal |
| `-32007` | 协议核 `axum_jsonrpc` | body 过大（jsonrpsee 对齐） |
| `-32010` | 协议核 `axum_jsonrpc` | batch 过长（jsonrpsee 对齐） |
| `-32001..-32009`（跳过 `-32007`） | 业务（如 `identity_contract`） | 领域错误 |
| `-32020..-32029` | `http_auth` | authn：缺 token / 无效 / 吊销 / 缓存不可用；**`authn_kit` 不感知 RPC** |
| `-32030..-32039` | `file_contract` | file：attachment（`-32032..-32034`）；`-32035` 仅 test 后端 / 未配置本地 presign；`-32036` rate limited；`-32030/31` 已退役勿复用 |

- 基础设施失败（DB 驱动等）→ **`-32603`**（`http_server` 经 `set_internal_error_message` 用 locale 渲染 message）
- **不要**把业务详情塞进 `error.data`（关联 id 走响应头 `x-request-id`，成功/失败都回）
- **跨表面共享**的领域错误放在 `*_contract`：`#[error("snake_case_key")]` + `rpc_code()`；基础设施失败不要包进该枚举
- 套路：
  - Handler / helper：`rootcause::Result`；外库直接 `?`
  - 领域失败：`Err(MyError::….into())`（表面在 `module()` 同步段用 `locale::register_locale_bundle` 成对注册 FTL+mapper；`check_module_order.py` 要求注册 locale 的表面排在 `http_server` 之前；seal 在 `http_server` invoke）
  - FTL：`app/<name>/locales/{en-US,zh-CN}/*.ftl`；`Display` / `l10n_key` 即 Fluent key
  - 临时非域错误可直接 `Result<T, ErrorObject>` / `Err(ErrorObject::new(...))`
  - `http_server` 启动前：`register_authn_locale()` → `seal_bundles` + `seal_domain_mappers`；漏注册 mapper 会把域错误打成 `-32603`；seal 后再 `register_*` 会 panic
- `/jsonrpc` 经 `locale::bind_accept_language`；message 随 `Accept-Language`（如 `en` / `zh-CN` → `en-US` / `zh-CN`）。可选 Cookie `accept_language` **仅语言偏好**，不是会话登录 Cookie
- `set_internal_error_message` / `set_standard_error_message` / `set_invalid_params_renderer` 为**进程级全局钩子**（单组合根假设）；第二 HTTP 入口或并行嵌套第二套路由时勿各写各的

示范：重复手机号 → `-32001` `account_duplicated`（见 `contracts/identity_contract`）。

## 可观测性

- RPC span：`jsonrpc.call` / `jsonrpc.batch`，字段 `rpc.*`
- 关联 id：响应头 `x-request-id`（协议核 `/jsonrpc` 全路径；模板 busy 树 timeout 中间件预解析 `ResolvedRequestId`，覆盖 408 / 外层 413 / PublicRouter，探针除外；入站 `traceparent` → `x-request-id` → 归一化 fallback → mint）。自定义组合根若不用该中间件，需自行插入 `ResolvedRequestId`，或依赖 `RpcRouter::trace_id_fallback`（otel 下 `instrument_router` 已装）
- 破坏性：无 `attach_trace_id`；勿再读 `error.data.trace_id`；`-32007` 的 `data` 为字符串限额说明
- Metrics：`RpcObserver`；OTLP 由 **`app` feature `otlp`**（默认开）门控
- 本地可关 OTLP：`cargo run -p app --no-default-features --features blob-fs`（或 `blob-cos`）

## 编译与改动策略

- **高频改动放叶子**：`app/*`、必要时 `bin/app`
- **少动宽依赖 crate**：改 `axum_jsonrpc` / `app_state` 公开 API 仍要克制（API 稳定是发布层面的承诺）——但**重编成本已实测不是障碍**：check-only 回路下，touch 表面方法只重检 2 个 crate（≈0.4s），touch 协议核全链 8 个也仅 ≈0.6s；最贵路径是 `axum_jsonrpc_macros`（proc-macro codegen，≈1.7s）
- 依赖尽量 `default-features = false` + 显式 feature（workspace 已如此）
- 日常迭代：`just check-fast <crate>`（默认 `identity`）或 `just check`（`-p app`）；**固定一种命令形态**——`-p` / `--workspace --all-targets` / `--all-features` 来回切会触发 feature 统一化补建（0.2–9s）
- blob 后端只切 `app` 的 `blob-fs` / `blob-cos`（对齐表：`docs/blob-backends.md`；workspace 已对 `app_state` 等关 default-features，cos 构建不拉 fs）
- 分层：`just layering`（`scripts/check_layering.py` + `check_module_order.py`；亦挂在 `pre_commit` / CI）

## 代码规范

- Edition **2024**；workspace clippy：`unwrap` / `expect` / `panic` / `indexing_slicing` / `dbg_macro` **deny**
- 测试里若需 `expect`，模块级 `#[allow(clippy::expect_used, …)]`
- 新依赖进 `[workspace.dependencies]`，crate 用 `{ workspace = true }`
- 不提交密钥；`.env` 已 gitignore，改配置示例只动 `.env.example`
- 用户要求提交时：Conventional Commits，**中文**说明（如 `feat(app_state): 引入 AppState 与 PublicRpc`）

## 明确不做 / 暂缓

除非产品明确要求，否则不要主动做：

- WebSocket / 订阅
- `rpc.discover` 类 introspection（已有 OpenRPC）
- 进程内 TLS（交给反向代理）
- Cookie session / Web 表单登录（当前为 Bearer JWT + opaque refresh；locale 的 `accept_language` Cookie 只做语言偏好）
- 泛型化 `AppState` 里的 `TokenHelper`（当前固定 `TokenRealm::Account` 单 realm）——出现第二个 token realm 时再引入注册表 / 多实例，勿提前设计
- 多 cache 后端、双组合根（独立 `bin/worker` 可后续再拆）
- COS 数据万象 URL 分叉 / 独立图片服务（imgproxy 等）；当前 fs 同进程 `?s=` 派生
- MFA / 多设备会话 / OIDC
- 常见/breached 密码 denylist

未完成项与优先级见 **`TODO.md`**。平台脚手架已对齐：CORS/超时/限流、ProbeRouter、CI/deny/compose、seed/`change_password`、blob/file、`REFRESH_GRACE_TTL`。

## 常用命令

```bash
cp .env.example .env          # 首次；需 Postgres + Redis + JWT_SECRET（≥32）+（blob-fs）BLOB_FS_DOMAIN + BLOB_FS_PRESIGN_SECRET（≥32）
just db-up                    # compose：仅 postgres + redis（app 本地跑）
just check-fast identity      # 叶子迭代（固定 -p，勿与 --all-features 交替）
just check                    # cargo check -p app（默认 otlp + blob-fs）
just layering                 # 分层硬违规
cargo run -p app              # 默认 otlp + blob-fs；需 MASTER=true 才 migrate/seed/GC
cargo run -p app --no-default-features --features blob-fs
cargo run -p app --no-default-features --features blob-cos
just migration-generate       # 模型变更后生成 SQL
just migration-apply          # 不经 app 启动、单独 apply
just migration-reset          # psql DROP SCHEMA + `migrator migration apply`
just e2e                      # reset → 临时 app（blob-fs，关 otlp）→ hurl
just clients-check            # 生成物对齐 + bun session.test.ts
cargo test --workspace --all-targets   # 含 seal 等 integration
cargo test -p axum_jsonrpc -p identity -p file --lib --features blob-fs
just pre_commit               # layering + machete + fixrs + fmt + clippy（含 --all-features）
just dev                      # cargo watch -x "run -p app"
```

## 改动检查清单

- [ ] 依赖方向未破坏（`just layering`；核 ← 模板 ← app/ops；表面之间只经 `contracts/*`）
- [ ] 新 RPC：默认进 `PrivateRpc`；仅匿名方法进 `PublicRpc`；`bin/app` 已 `init_group` 且 module 排在 `http_server` 之前
- [ ] 业务错误有稳定 `-320xx`（避开协议 `-32007`/`-32010` 与 authn `-32020..`）：`module()` 内 `locale::register_locale_bundle`（漏 mapper 会变 `-32603`；挂在 `http_server` 之后会 seal 后注册 panic）；authn 勿在表面重复注册
- [ ] `/jsonrpc` 响应带 `x-request-id`；未滥用 `error.data` 传业务字段 / 未再往 body 塞 `trace_id`
- [ ] AppState：`persistence` 提供 `Db`，`cache` 提供 Redis，`blob` 提供 `Blob`，`http_server` 包成 `AppState` 后 `with_state`；表面只用 `RpcRouter::<AppState>::new()`
- [ ] 需 caller id 时用 `AuthedAccount`；登录门槛靠 `PrivateRpc`（勿对 `/jsonrpc` 强制 Bearer）；默认依赖勿照抄 identity 整图
- [ ] 私有 RPC 按 id 操作资源时默认做归属：identity 用 `access::load_for_mutate` / `load_for_delete`（非特权改删他人 / 删特权账号 → `-32009`）；`read`/`search` 可开放全库。配 e2e 越权断言（曾因漏写导致任意账号越权）
- [ ] Schema：经 toasty migration；多实例仅 master apply（或 `just migration-apply`）
- [ ] `cargo test` / clippy 相关包通过

## 文档索引

| 文档 | 用途 |
|------|------|
| `README.md` | 启动、端点、配置、结构 |
| `TODO.md` | 路线图与验收标准 |
| `docs/app-template/` | 最小 app 表面骨架（勿抄 identity 依赖图） |
| `docs/blob-backends.md` | `blob-fs` / `blob-cos` 多 crate 对齐表 |
| `scripts/check_layering.py` | 分层硬违规检查 |
| `crates/trace_kit/README.md` | tracing / OTLP |
| `infra/app_state/lib.rs` | State 所有权注释 |
