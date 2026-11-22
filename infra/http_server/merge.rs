//! 合并 Public/Private/Probe HTTP 与 Public/Private RPC 组。

use std::collections::BTreeSet;

use app_state::AppState;
use axum::Router;
use axum::middleware::from_fn_with_state;
use axum_jsonrpc::{ReadyRpcRouter, RpcRouter};
use http_auth::{account_auth_middleware, require_authed_account};
use http_routing::{PrivateRouter, PrivateRpc, ProbeRouter, PublicRouter, PublicRpc};
use modrun::{Error, Group, Result};

pub(crate) fn merge_probes(routers: Group<ProbeRouter>) -> Router<AppState> {
    routers
        .into_iter()
        .fold(Router::<AppState>::new(), |acc, ProbeRouter(router)| {
            acc.merge(router)
        })
}

pub(crate) fn merge_http(routers: Group<PublicRouter>) -> Router<AppState> {
    routers
        .into_iter()
        .fold(Router::<AppState>::new(), |acc, PublicRouter(router)| {
            acc.merge(router)
        })
}

pub(crate) fn merge_private(routers: Group<PrivateRouter>, state: AppState) -> Router<AppState> {
    let fragments: Vec<_> = routers.into_iter().collect();
    if fragments.is_empty() {
        return Router::<AppState>::new();
    }
    fragments
        .into_iter()
        .fold(Router::<AppState>::new(), |acc, PrivateRouter(router)| {
            acc.merge(router)
        })
        .route_layer(from_fn_with_state(state, account_auth_middleware))
}

pub(crate) fn merge_rpc(
    state: AppState,
    public: Group<PublicRpc>,
    private: Group<PrivateRpc>,
    max_body_bytes: usize,
    max_batch_len: usize,
) -> Result<(ReadyRpcRouter<AppState>, BTreeSet<String>)> {
    let mut merged = RpcRouter::<AppState>::new()
        .max_body_bytes(max_body_bytes)
        .max_batch_len(max_batch_len);
    #[cfg(feature = "otel")]
    {
        // 顺序契约：必须在所有 merge **之前**装 observer / trace_id_fallback——
        // merge 保留 self 侧的两者，merge 之后再装不会作用于已合入的 fragment。
        // Busy 树 HTTP 优先 timeout 中间件的 ResolvedRequestId；fallback 留给无该中间件的挂载方式。
        merged = crate::otel::instrument_router(merged);
    }
    for PublicRpc(router) in public {
        merged = merged
            .merge(router)
            .map_err(|e| Error::hook(e.to_string()))?;
    }
    // Private fragments gate with `before_each(require_authed_account)`; remember their
    // names so /openrpc.json can flag them via `x-auth: "required"`.
    let mut auth_required = BTreeSet::new();
    for PrivateRpc(router) in private {
        for meta in router.method_metas() {
            auth_required.insert(meta.name.clone());
        }
        merged = merged
            .merge(router.before_each(require_authed_account))
            .map_err(|e| Error::hook(e.to_string()))?;
    }
    Ok((merged.with_state(state), auth_required))
}
