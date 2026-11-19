//! Template HTTP / JSON-RPC route group wrappers.
//!
//! JSON-RPC primitives live in `axum_jsonrpc`; depend on that crate directly
//! for reusable RPC code. This crate only provides app-level router tags.
//!
//! Features contribute method tables / route fragments **without** calling
//! `with_state`; `http_server` merges groups then attaches process [`AppState`].
//!
//! 限流：无全站中间件。贵路径在 method 内调用
//! [`app_state::AuthRateLimit::allow`] / [`cache_kit::Cache::allow_sliding_window`]
//!（示范：identity `rate_limit`）。生产全站限流放网关。

use app_state::AppState;
use axum::Router;
use axum_jsonrpc::RpcRouter;

#[derive(Clone)]
pub struct PrivateRouter(pub Router<AppState>);

/// Public HTTP surface contributed via `modrun` groups.
///
/// Typed with [`AppState`] so handlers can extract `State<Db>` / `FromRef`.
///
/// **Body limit：** `http_server` 对整棵 PublicRouter 套 [`PUBLIC_BODY_LIMIT_BYTES`]
///（默认 16 MiB），**不**使用更小的 `BODY_LIMIT_BYTES`（那只约束 RPC / PrivateRouter）。
/// 公开写接口若需要更严上限，在**本路由上**再挂更小的 `DefaultBodyLimit`（勿在外层用
/// 更小 limit 包住更大路由——外层会先截断）。
#[derive(Clone)]
pub struct PublicRouter(pub Router<AppState>);

/// PublicRouter 默认请求体上限（与 fs `PUT /files/presigned` 对齐）。
pub const PUBLIC_BODY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Liveness / readiness probes. Merged outside the busy (timeout) tree.
#[derive(Clone)]
pub struct ProbeRouter(pub Router<AppState>);

/// Anonymous JSON-RPC surface contributed via `modrun` groups (no auth gate).
///
/// Method table typed with [`AppState`]; `http_server` attaches process state
/// via [`RpcRouter::with_state`]. Prefer [`PrivateRpc`] for methods that need
/// a logged-in account; only put anonymous methods here（如 login / refresh）。
#[derive(Clone)]
pub struct PublicRpc(pub RpcRouter<AppState>);

/// Authenticated JSON-RPC surface contributed via `modrun` groups.
///
/// `http_server` wraps each fragment with `before_each(require_authed_account)`
/// before merge. Handlers that need the caller id still take `AuthedAccount`.
#[derive(Clone)]
pub struct PrivateRpc(pub RpcRouter<AppState>);
