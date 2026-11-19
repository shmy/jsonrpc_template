# AGENTS.md — JSON-RPC Template

给在本仓库工作的编码代理：优先遵循本文，细节以代码与 `README.md` / `TODO.md` 为准。

**快速入口：** 启动/端点 → [`README.md`](README.md)；加业务 → [`docs/feature-template/`](docs/feature-template/)；blob 后端切换 → [`docs/blob-backends.md`](docs/blob-backends.md)；分层 CI → `just layering`。

## 项目是什么

Axum + `modrun` 的 **JSON-RPC 2.0 应用脚手架**。

- **可复用协议核**：`crates/axum_jsonrpc*`（可单独依赖）
- **应用模板**：`bin/` + `features/` + `infra/`（示范如何挂业务）

默认端点：`POST /jsonrpc`、`GET /openrpc.json`、`GET /livez`、`GET /healthz`、`GET /readyz`、（`blob-fs`）`PUT /files/presigned` + `GET /files/{*path}`；上传走 JSON-RPC `file.presign`。

## 分层与依赖方向（必须遵守）

```text
bin/api                    # 组合根：CLI supply + module 注册
  → features/*             # 业务 / 运维面——彼此不互依
  → features/*_contract    # 跨 feature 契约（模型等）；feature 唯一可依赖的兄弟层
  → infra/*                # 应用壳（http_server、persistence、blob、file_attachment、…）
  → crates/axum_jsonrpc*   # 协议核（不要反向依赖 features / 模板 infra）
  → crates/{blob_kit,trace_kit,…}
```

| 目录 | 职责 | 改动频率 |
|------|------|----------|
| `crates/axum_jsonrpc*` | 解析、batch、limits、Handler、Observer、trace_id；`RpcRouter` / `ReadyRpcRouter` | 低（宽依赖，API 求稳） |
| `crates/blob_kit` | OpenDAL 对象存储门面（fs / 腾讯 COS / test-utils） | 低 |
| `infra/app_state` | 进程级 `AppState`（`Db` + `Cache` + `Blob` + `TokenHelper` + `FromRef`） | 中（加资源时扩展） |
| `infra/blob` | OpenDAL `Blob` provide（feature `fs` / `cos`；cos 优先；fs→`config/fs`：`BLOB_FS_ROOT` 有默认，`DOMAIN`/`PRESIGN_SECRET` 必填；cos→`config/cos` 必填 `BLOB_S3_*`） | 低 |
| `infra/file_attachment` | staging claim / commit / attach / detach + **通用绑定编排**（`bind` / `AttachmentWrite` fresh-stale 引用计数协议；跨业务附件实现，勿让 feature 互依；identity avatar 为薄适配示范） | 中 |
| `infra/cache` | Redis `Cache`（modrun provide） | 低 |
| `infra/http_auth` | Bearer 解析、软/强制鉴权中间件、`AuthedAccount`；authn 错误码段（`-32020..-32029`）+ FTL + mapper 自持，由 `http_server` seal 前统一注册（**勿**在 feature `module()` 里再注册） | 低 |
| `infra/http_routing` | `PublicRpc` / `PrivateRpc` / `PublicRouter` / `PrivateRouter` / `ProbeRouter`（无限流中间件） | 低 |
| `infra/http_extractors` | `ClientIp` + `TRUSTED_PROXIES` 解析 | 低 |
| `infra/persistence` | DB 连接；提供进程级 `Db`；注册 `*_contract` 模型；仅 `MASTER=true` 时 apply embedded migrations（默认 `false`） | 中 |
| `infra/http_server` | `Db`+`Cache`+`Blob`+JWT → `AppState`；合并路由、监听、OpenRPC info、中间件、`with_state` | 中 |
| `crates/{authn_kit,jwt_kit,cache_kit}` | 鉴权 key/错误、HS256 JWT、Redis 缓存门面 | 低 |
| `features/file` | `file.presign`、（fs）本地预签名 PUT / 回源、Lifecycle GC（仅 master） | 中 |
| `features/*` | 业务 RPC / HTTP | **高（日常应落这里）** |
| `features/*_contract` | 跨 feature 契约（如 toasty 模型）；**不**依赖任何 `features/*` 实现 | 中 |
| `bin/api` | 组装入口（默认 `otlp`+`blob-fs`；可切 `blob-cos`） | 低 |

**禁止（`just layering` / CI 会拦硬违规）：**

- 让 `axum_jsonrpc` 依赖 `features`、`http_server`、业务类型
- 把 `PublicRpc` / `PrivateRpc` 塞回协议核（它们属于模板侧 `http_routing`）
- feature 依赖 `http_server`（用 `app_state` / `http_routing` / `axum_jsonrpc` / `locale`）
- **feature 依赖另一个 feature 实现 crate**——兄弟业务只能依赖对方的 `*_contract`（例：可用 `identity_contract`，禁止依赖 `identity`）。附件实现走 `infra/file_attachment`（通用绑定编排 `file_attachment::bind` + fresh/stale 时序；identity `avatar_ops` 为薄适配示范，新业务勿复制粘贴它的实现）
- `*_contract` 依赖任何 feature 实现，或互相堆业务逻辑（契约保持薄：**只放 wire 形态（新类型/schema/稳定错误码）**，含业务语义的规则落 feature 实现）。staging 前缀权威在 `blob_kit::STAGING_PREFIX`；`file_contract::STORAGE_DIR_PREFIX` 为镜像，由 `file` 单测锁死相等
- infra 依赖 `features/*_contract`——**仅允许下列例外**（勿再扩大）：
  1. `persistence`：`toasty::models!(…)` **注册**各 `*_contract` 模型（落库壳，不写业务）
  2. `http_auth`：借用 `identity_contract::Id` 作为 `AuthedAccount` 主键（纯类型；出现第二个带自有主键的业务时再泛型化）
  3. `file_attachment`：staging claim 主键借用 `identity_contract::Id`（与上同类）；域错误借用 `file_contract::FileError` / 码段（附件实现与 file 契约对齐，勿让 feature 互依）
- `crates/` 里的工具 kit（`authn_kit` / `jwt_kit` / `cache_kit` / `blob_kit` / `trace_kit`）保持协议与业务无关：不定义 `-320xx` RPC 码段、不依赖 `locale`——码段分配与文案归使用它的 infra / feature 壳层（例：authn 在 `http_auth`；file 在 `file_contract`）。`blob_kit`：Fs 本地 HMAC 在 `local_presign` / `Fs`；门面只提供 `as_fs()`，勿再挂 `verify_local_*`

## AppState 所有权（易错）

1. Feature 用 **`RpcRouter::<AppState>::new()`** 注册方法（**仅方法表**，不要 `with_state`）
2. `persistence::module()` 异步提供**进程级** `Db`；`cache::module()` 提供 Redis `Cache`；`blob::module()` 提供 `Blob`；仅当 **`MASTER=true`** 时 apply embedded migrations、seed 管理员、启动 file staging/attachment GC（`MASTER` 默认 `false`；多实例只开一个 master，或用 `just migration-apply`）
3. `http_server` 将 `Db` + `Cache` + `Blob` + `TokenHelper` 包成 `AppState`（含进程级配额 `AuthRateLimit` / `LoginPhoneRate`，来自 `RATE_LIMIT_*`）；`merge_rpc` 合并 `PublicRpc` +（`PrivateRpc` 经 `before_each(require_authed_account)`）后 **`.with_state(process_state)`** → `ReadyRpcRouter`；`/jsonrpc` 挂软鉴权（有 Bearer 则验签写 `AuthedAccount`/`AuthnError`）；`PrivateRouter` 挂强制鉴权。Body limit：**`BODY_LIMIT_BYTES`（默认 1 MiB）只套 RPC + PrivateRouter**；**PublicRouter 统一 `PUBLIC_BODY_LIMIT_BYTES`（16 MiB）**。公开写接口若要更严，在路由上挂**更小**的 `DefaultBodyLimit`；切勿用更小的外层 limit 包住更大上传路由。限流：无全站中间件；贵路径在 method 内调 `AuthRateLimit::allow` / `LoginPhoneRate::allow` / `Cache::allow_sliding_window`（示范 identity `rate_limit`，配额可配勿硬编码）；生产全站限流放网关

Feature 路由器上的 state 在 merge 时**不会带上**；不要在 feature 里定制「自以为会生效」的 AppState。

共享资源（池、仓储）放进 `AppState`，用 `State<AppState>` 或 `State<T: FromRef<AppState>>` 提取。保持 `app_state` crate **轻量**（features 会依赖它）。

会话（JWT jti / refresh digest）跨多个 Redis key **无事务**：`issue_tokens` 部分写入失败时可能短暂半开会话；下次 login/refresh 会覆盖 subject 指针并清理旧 digest。每账号单会话模型。refresh **fail-closed**，分两段：
- **commit 前**并发 `consume`：仅一人 `take` 成功；败者拿不到 `used_*` 时只得 `-32004`，**不**吊销胜者（见 `token_ops` 单测 `concurrent_refresh_does_not_revoke_winner`）
- **轮换完成后**再用旧 refresh：命中 `used_*` → 吊销该账号全部会话（`-32004`），需重新 login
如需缓解客户端超时重试，可给旧 token 加短宽限期（暂缓）。

Schema 变更：改 `*_contract` 模型 → `just migration-generate` → 提交 `toasty/` → master 启动或 `just migration-apply`。HTTP 集成测试用 `e2e/*.hurl`（`just e2e` 会清库再跑）；勿用 Rust `#[test]` 打 `/jsonrpc`。

## 扩展业务 Feature

抄写骨架：[`docs/feature-template/`](docs/feature-template/)（**非** workspace 成员；默认依赖勿照抄 `identity`）。

### 目录约定

```text
features/<name>/
  lib.rs                 # module()、provide_*、Lifecycle；不写业务细节
  methods/mod.rs         # 有 RPC 时必有；声明各 method 子模块
  methods/<action>.rs    # 单文件 ≈ 一个 RPC（req/resp + handler）
  endpoints/mod.rs       # 仅当有非 RPC HTTP 时
  endpoints/<route>.rs
  shared/mod.rs          # 辅助一律进这里（含 l10n、GC、access、ops…）
  shared/<helper>.rs
  locales/{en-US,zh-CN}/*.ftl
```

- **根目录只留** `lib.rs` + 可选的 `methods/` / `endpoints/` / `shared/` / `locales/`；禁止再在根上堆 `l10n.rs`、`token_ops.rs`、`http_error.rs` 等
- **子模块声明统一** `foo/mod.rs`，不用并列的 `foo.rs` + `foo/`；`methods` / `endpoints` / `shared` 下的子模块一律 `pub(crate) mod`（`lib` 里对三者本身用私有 `mod`）
- **`shared/` 始终存在**（只要有 locale / 跨 method 逻辑）；`l10n` 固定为 `shared/l10n.rs`
- **无 HTTP 的 feature 不建** `endpoints/`；无 RPC 的（如 probe）不建 `methods/`
- method 文件名用 RPC 动作短名（`login_account`、`presign`）；endpoint 文件名用路由语义（`livez`、`file_presigned_put`），不必强加 feature 前缀

### 步骤

1. 新建 `features/<name>`，`#[rpc("domain.action")]` 写 handler
2. `RpcRouter::<AppState>::new().register(foo_rpc)?…`：**默认包进 `PrivateRpc`**（merge 时统一鉴权门）；仅匿名方法（如 login / refresh）进 `PublicRpc`（identity `create` 需登录但恒 `privileged=false`）
3. `bin/api`：`.module(your_feature::module())`——**`persistence` / `cache` / `blob` 以及所有会 `register_ftl` / `register_domain_mapper` 的 feature 必须排在 `http_server` 之前**（`http_server` 在 `register` 时 `seal_bundles` + `seal_domain_mappers`；挂在其后会 panic 或域错误静默变成 `-32603`）；根上需 `init_group::<PublicRpc>()` + `init_group::<PrivateRpc>()`（探针另加 `init_group::<ProbeRouter>()`；HTTP 组另加 Public/Private Router）
4. HTTP 非 RPC：探针走 `ProbeRouter`（见 `features/probe`）；公开 HTTP 走 `PublicRouter`（示范：`blob-fs` 下 `ServeDir` + `is_public_readable_object_path` 守卫 + 不可信扩展 `Content-Disposition: attachment` + 本地预签名 PUT；默认 16 MiB body；Cos 不挂回源；staging GET 无 claim，靠高熵 path）；需登录走 `PrivateRouter`（当前模板无业务 HTTP；可按需扩展）
5. 需要 caller id 时再在 handler 参数加 `http_auth::AuthedAccount`（读 soft 中间件写入的 extensions）。**「要不要登录」由 `PrivateRpc` 分组决定**，勿依赖漏写提取器；勿在 `/jsonrpc` 上硬鉴权（公开方法共用该端点）
6. 需要落库 / 跨 feature 共享类型：放 `features/<name>_contract`；其它 feature **只**依赖该 contract；在 `persistence` 里 `toasty::models!(…)` 注册
7. 有领域错误时：在 feature 的 `module()` 里注册 locale bundle（ftl + domain mapper；示范：`register_identity_locale_bundle` / file 的 `register_file_*`）——**authn 例外**：由 `http_server::register` 调 `register_authn_locale()`，feature 不要重复注册。时机：业务 feature 均须在 `http_server` 之前
8. 持有正式 blob 对象引用的 feature（如 identity avatar）须在 `module()` 调用 `file_attachment::register_attachment_in_use`，供 AttachmentGc 安全网合并；该 module 应排在启动 GC 的 `file` 之前（或至少在进程 `run` 前完成注册）。绑定编排直接用 `file_attachment::bind`（入参 `BindInput::Clear/Set` 显式区分清空；省略态走 `keep_current`；fresh/stale 时序见其模块文档），勿在各 feature 里手写 commit/attach/detach 编排

**默认 Cargo 依赖（叶子 feature）**：`app_state` + `http_routing` + `axum_jsonrpc` + `locale` + 自己的 `*_contract`。按需再加：`http_auth`（只要 `AuthedAccount`）、`http_extractors`（`ClientIp` 等）。**不要**默认照抄 `identity` 整图（它还拉了 `config` / `authn_kit` / `jwt_kit` / `cache_kit`，因为示范会话面）。

跨 feature 复用**实现逻辑**（非模型）：不能依赖兄弟 feature 实现 crate；当第二个 feature 需要同一实现（例：identity 内部 `shared/hashed_password` 的密码哈希），把它提取到 `infra/*` 小 crate 或下沉 contract，禁止直接 `use identity::…`（`shared` 本就 crate-private，也够不到）。

方法命名示范：`identity.account.create`（点分域）。

Handler 可用 Axum 同构提取器：`Params<T>`、`State<…>`、自定义 `FromRequestParts`。

## 错误与 locale

客户端按稳定 **`code` 分支**。号段（勿碰撞）：

| 号段 | 归属 | 说明 |
|------|------|------|
| `-32700` / `-32600..-32603` | JSON-RPC 标准 | parse / invalid / method / params / internal |
| `-32007` | 协议核 `axum_jsonrpc` | body 过大（jsonrpsee 对齐） |
| `-32010` | 协议核 `axum_jsonrpc` | batch 过长（jsonrpsee 对齐） |
| `-32001..-32009`（跳过 `-32007`） | 业务（如 `identity_contract`） | 领域错误 |
| `-32020..-32029` | `http_auth` | authn：缺 token / 无效 / 吊销 / 缓存不可用；**`authn_kit` 不感知 RPC** |
| `-32030..-32039` | `file_contract` | file：attachment（`-32032..-32034`）；`-32035` 仅 test 后端 / 未配置本地 presign；`-32030/31` 已退役勿复用 |

- 基础设施失败（DB 驱动等）→ **`-32603`**（`http_server` 经 `set_internal_error_message` 用 locale 渲染 message）
- **不要**把业务详情塞进 `error.data`（框架仅在错误上注入 `trace_id`）
- **跨 feature 共享**的领域错误放在 `*_contract`：`#[error("snake_case_key")]` + `rpc_code()`；基础设施失败不要包进该枚举
- 套路：
  - Handler / helper：`rootcause::Result`；外库直接 `?`
  - 领域失败：`Err(MyError::….into())`（feature 在 `module()` 注册 `register_domain_mapper` + `locale::register_ftl`，且该 module 须在 `http_server` 之前）
  - FTL：`features/<name>/locales/{en-US,zh-CN}/*.ftl`；`Display` / `l10n_key` 即 Fluent key
  - 临时非域错误可直接 `Result<T, ErrorObject>` / `Err(ErrorObject::new(...))`
  - `http_server` 启动前：`register_authn_locale()` → `seal_bundles` + `seal_domain_mappers`；漏注册 mapper 会把域错误打成 `-32603`；seal 后再 `register_*` 会 panic
- `/jsonrpc` 经 `locale::bind_accept_language`；message 随 `Accept-Language`（如 `en` / `zh-CN` → `en-US` / `zh-CN`）。可选 Cookie `accept_language` **仅语言偏好**，不是会话登录 Cookie
- `set_internal_error_message` / `set_standard_error_message` / `set_invalid_params_renderer` 为**进程级全局钩子**（单组合根假设）；第二 HTTP 入口或并行嵌套第二套路由时勿各写各的

示范：重复手机号 → `-32001` `account_duplicated`（见 `features/identity_contract`）。

## 可观测性

- RPC span：`jsonrpc.call` / `jsonrpc.batch`，字段 `rpc.*`
- 错误响应：`error.data.trace_id`（成功响应不加）
- Metrics：`RpcObserver`；OTLP 由 **`api` feature `otlp`**（默认开）门控
- 本地可关 OTLP：`cargo run -p api --no-default-features --features blob-fs`（或 `blob-cos`）

## 编译与改动策略

- **高频改动放叶子**：`features/*`、必要时 `bin/api`
- **少动宽依赖 crate**：改 `axum_jsonrpc` / `app_state` 公开 API 仍要克制（API 稳定是发布层面的承诺）——但**重编成本已实测不是障碍**：check-only 回路下，touch feature 方法只重检 2 个 crate（≈0.4s），touch 协议核全链 8 个也仅 ≈0.6s；最贵路径是 `axum_jsonrpc_macros`（proc-macro codegen，≈1.7s）
- 依赖尽量 `default-features = false` + 显式 feature（workspace 已如此）
- 日常迭代：`just check-fast <crate>`（默认 `identity`）或 `just check`（`-p api`）；**固定一种命令形态**——`-p` / `--workspace --all-targets` / `--all-features` 来回切会触发 feature 统一化补建（0.2–9s）
- blob 后端只切 `api` 的 `blob-fs` / `blob-cos`（对齐表：`docs/blob-backends.md`）
- 分层：`just layering`（`scripts/check_layering.py`；亦挂在 `pre_commit` / CI）

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
- job_queue / scheduler（file GC 用 Lifecycle 间隔任务，仅 master）、多 cache 后端、双组合根
- MFA / 多设备会话 / OIDC

未完成项与优先级见 **`TODO.md`**。平台脚手架已对齐：CORS/超时/限流、ProbeRouter、CI/deny/compose、seed/`change_password`、blob/file。

## 常用命令

```bash
cp .env.example .env          # 首次；需 Postgres + Redis + JWT_SECRET（≥32）+（blob-fs）BLOB_FS_DOMAIN + BLOB_FS_PRESIGN_SECRET（≥32）
just check-fast identity      # 叶子迭代（固定 -p，勿与 --all-features 交替）
just check                    # cargo check -p api（默认 otlp + blob-fs）
just layering                 # 分层硬违规
cargo run -p api              # 默认 otlp + blob-fs；需 MASTER=true 才 migrate/seed/GC
cargo run -p api --no-default-features --features blob-fs
cargo run -p api --no-default-features --features blob-cos
just migration-generate       # 模型变更后生成 SQL
just migration-apply          # 不经 api 启动、单独 apply
just migration-reset          # `migrator migration reset`
just e2e                      # reset → 临时 api（blob-fs，关 otlp）→ hurl
cargo test --workspace --all-targets   # 含 seal 等 integration
cargo test -p axum_jsonrpc -p identity -p file --lib --features blob-fs
just pre_commit               # layering + machete + fixrs + fmt + clippy（含 --all-features）
just dev                      # cargo watch -x "run -p api"
```

## 改动检查清单

- [ ] 依赖方向未破坏（`just layering`；核 ← 模板 ← feature；feature 之间只经 `*_contract`）
- [ ] 新 RPC：默认进 `PrivateRpc`；仅匿名方法进 `PublicRpc`；`bin/api` 已 `init_group` 且 module 排在 `http_server` 之前
- [ ] 业务错误有稳定 `-320xx`（避开协议 `-32007`/`-32010` 与 authn `-32020..`）：`module()` 内同时 `register_domain_mapper` + `register_ftl`（漏注册会变 `-32603`；挂在 `http_server` 之后会 seal 后注册 panic）；authn 勿在 feature 重复注册
- [ ] 未在成功响应里塞 `trace_id`；未滥用 `error.data` 传业务字段
- [ ] AppState：`persistence` 提供 `Db`，`cache` 提供 Redis，`blob` 提供 `Blob`，`http_server` 包成 `AppState` 后 `with_state`；feature 只用 `RpcRouter::<AppState>::new()`
- [ ] 需 caller id 时用 `AuthedAccount`；登录门槛靠 `PrivateRpc`（勿对 `/jsonrpc` 强制 Bearer）；默认依赖勿照抄 identity 整图
- [ ] 私有 RPC 按 id 操作资源时默认做归属：identity 用 `access::load_for_mutate` / `load_for_delete`（非特权改删他人 / 删特权账号 → `-32009`）；`read`/`search` 可开放全库。配 e2e 越权断言（曾因漏写导致任意账号越权）
- [ ] Schema：经 toasty migration；多实例仅 master apply（或 `just migration-apply`）
- [ ] `cargo test` / clippy 相关包通过

## 文档索引

| 文档 | 用途 |
|------|------|
| `README.md` | 启动、端点、配置、结构 |
| `TODO.md` | 路线图与验收标准 |
| `docs/feature-template/` | 最小 feature 骨架（勿抄 identity 依赖图） |
| `docs/blob-backends.md` | `blob-fs` / `blob-cos` 多 crate 对齐表 |
| `scripts/check_layering.py` | 分层硬违规检查 |
| `crates/trace_kit/README.md` | tracing / OTLP |
| `infra/app_state/lib.rs` | State 所有权注释 |
