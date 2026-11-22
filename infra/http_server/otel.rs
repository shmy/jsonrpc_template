use std::sync::Arc;
use std::time::Duration;

use axum_jsonrpc::{RpcObserver, RpcRouter};
use http_1::HeaderMap;
use trace_kit::metrics::record_rpc_batch;
use trace_kit::metrics::record_rpc_call;

/// Forwards RPC dispatch hooks to `trace_kit::metrics`.
pub(crate) struct OtelRpcObserver;

impl RpcObserver for OtelRpcObserver {
    fn on_call(&self, method: &str, error_code: Option<i32>, elapsed: Duration) {
        record_rpc_call(method, error_code.is_none(), error_code, elapsed);
    }

    fn on_batch(&self, size: usize) {
        record_rpc_batch(size);
    }
}

pub(crate) fn instrument_router<S>(router: RpcRouter<S>) -> RpcRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .observer(Arc::new(OtelRpcObserver))
        // Busy-tree HTTP prefers `ResolvedRequestId` from timeout middleware.
        // Keep this fallback for `/jsonrpc` mounted without that middleware
        // (custom composition / direct `into_axum_rpc_router`).
        .trace_id_fallback(|| trace_kit::extract_trace_id(&HeaderMap::new()))
}
