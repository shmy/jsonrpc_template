//! JSON-RPC 2.0 over HTTP for axum.
//!
//! OpenRPC document generation lives in `axum_jsonrpc_openrpc` so feature crates
//! that only register methods do not rebuild when docs change.
//!
//! # Process-wide singleton hooks
//!
//! [`set_internal_error_message`], [`set_standard_error_message`] and
//! [`set_invalid_params_renderer`] (plus `register_domain_mapper` /
//! `seal_domain_mappers` under the `rootcause` feature) mutate
//! **process-global** state: the core stays `locale`-free by letting the app
//! shell install renderers/mappers at startup instead of threading them
//! through every call. The trade-off: one process runs **one** configuration —
//! two `RpcRouter`s in the same process share the same message renderers and
//! domain mappers, and hooks set after sealing are ignored or panic
//! (fail-loud). Treat the crate as "one app per process"; do not embed it
//! twice with different localization needs.
//!
//! ```ignore
//! use axum_jsonrpc::{Params, RpcRouter, rpc};
//!
//! #[rpc("math.add")]
//! async fn add(Params((a, b)): Params<(i32, i32)>) -> Result<i32, axum_jsonrpc::ErrorObject> {
//!     Ok(a + b)
//! }
//!
//! // POST /jsonrpc only. Add `axum_jsonrpc_openrpc::RpcRouterOpenRpcExt` for /openrpc.json.
//! let app = RpcRouter::new()
//!     .register(add_rpc)?
//!     .into_axum_rpc_router();
//! ```

#[cfg(feature = "rootcause")]
mod domain_mapper;
mod handler;
mod internal_message;
mod invalid_params;
mod meta;
mod observer;
mod params;
mod parse;
mod router;
mod standard_message;
mod trace_id;
mod types;

pub use axum::extract::FromRequestParts;
#[cfg(feature = "rootcause")]
pub use domain_mapper::{DomainReportMapper, register_domain_mapper, seal_domain_mappers};
pub use handler::RpcHandler;
pub use internal_message::set_internal_error_message;
pub use invalid_params::set_invalid_params_renderer;
pub use meta::{MethodMeta, MethodSpec, Registrable};
pub use observer::{NoopRpcObserver, RpcObserver};
pub use params::{
    DeserializeErrorKind, IntoExtractError, InvalidParamsCause, Params, ParamsRejection,
    ValidatedParams,
};
pub use router::{ReadyRpcRouter, RegisterError, RpcRouter, TraceIdFallback};
pub use serde_json::value::RawValue;
pub use standard_message::{StandardErrorKind, set_standard_error_message};
pub use trace_id::{attach_trace_id, resolve_trace_id, resolve_trace_id_with_fallback};
pub use types::{
    DEFAULT_BATCH_CONCURRENCY, ErrorObject, Id, MAX_BATCH_LEN, MAX_BODY_BYTES, Response,
    error_codes,
};

/// Attribute macro: generate `{fn}_rpc` registration handle from the handler signature.
pub use axum_jsonrpc_macros::rpc;

#[cfg(test)]
mod suite;
