//! OpenRPC document generation for [`axum_jsonrpc::RpcRouter`].
//!
//! Kept separate so edits here do not force rebuilds of feature crates that only
//! depend on `axum_jsonrpc` registration APIs.

mod document;
mod ext;

pub use document::{
    OpenRpcComponents, OpenRpcContent, OpenRpcDocument, OpenRpcError, OpenRpcExample,
    OpenRpcExampleParam, OpenRpcExampleResult, OpenRpcInfo, OpenRpcMethod, OpenRpcServer,
    OpenRpcTag, build_document,
};
pub use ext::RpcRouterOpenRpcExt;

#[cfg(test)]
mod router_tests;
