use std::sync::Arc;
use std::time::Duration;

/// Dispatch hooks for metrics (OTel stays in the template layer, e.g. `http_server`).
pub trait RpcObserver: Send + Sync + 'static {
    fn on_call(&self, method: &str, error_code: Option<i32>, elapsed: Duration);
    fn on_batch(&self, size: usize);
}

/// Default no-op observer.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRpcObserver;

impl RpcObserver for NoopRpcObserver {
    fn on_call(&self, _method: &str, _error_code: Option<i32>, _elapsed: Duration) {}

    fn on_batch(&self, _size: usize) {}
}

pub(crate) type SharedObserver = Arc<dyn RpcObserver>;

pub(crate) fn default_observer() -> SharedObserver {
    Arc::new(NoopRpcObserver)
}
