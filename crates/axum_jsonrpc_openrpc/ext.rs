use std::collections::BTreeSet;

use axum::Router as AxumRouter;
use axum::body::Bytes;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum_jsonrpc::{ReadyRpcRouter, RpcRouter};

use crate::document::{OpenRpcDocument, OpenRpcInfo, OpenRpcServer, build_document};

const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

/// OpenRPC helpers layered on ready routers without coupling feature crates to docs.
pub trait RpcRouterOpenRpcExt {
    /// Build OpenRPC document from registered methods (includes skeleton entries).
    fn openrpc(&self, info: OpenRpcInfo, servers: Vec<OpenRpcServer>) -> OpenRpcDocument;

    /// Like [`openrpc`](Self::openrpc) but marks auth-gated method names with
    /// `x-auth: "required"`.
    fn openrpc_with_auth(
        &self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> OpenRpcDocument;

    /// Mount `POST /jsonrpc` and `GET /openrpc.json`.
    fn into_axum_openrpc_router(self) -> AxumRouter;

    /// Mount `POST /jsonrpc` and `GET /openrpc.json` with custom info (no servers).
    fn into_axum_openrpc_router_with_info(self, info: OpenRpcInfo) -> AxumRouter;

    /// Mount `POST /jsonrpc` and `GET /openrpc.json` with custom info and servers.
    fn into_axum_openrpc_router_with(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
    ) -> AxumRouter;

    /// Mount `POST /jsonrpc` and `GET /openrpc.json` with custom info, servers and
    /// `x-auth: "required"` markers for the given method names.
    fn into_axum_openrpc_router_with_auth(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> AxumRouter;
}

impl RpcRouterOpenRpcExt for RpcRouter<()> {
    fn openrpc(&self, info: OpenRpcInfo, servers: Vec<OpenRpcServer>) -> OpenRpcDocument {
        self.openrpc_with_auth(info, servers, &BTreeSet::new())
    }

    fn openrpc_with_auth(
        &self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> OpenRpcDocument {
        build_document(info, servers, &self.method_metas(), auth_required)
    }

    fn into_axum_openrpc_router(self) -> AxumRouter {
        self.with_state(()).into_axum_openrpc_router()
    }

    fn into_axum_openrpc_router_with_info(self, info: OpenRpcInfo) -> AxumRouter {
        self.with_state(()).into_axum_openrpc_router_with_info(info)
    }

    fn into_axum_openrpc_router_with(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
    ) -> AxumRouter {
        self.with_state(())
            .into_axum_openrpc_router_with(info, servers)
    }

    fn into_axum_openrpc_router_with_auth(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> AxumRouter {
        self.with_state(())
            .into_axum_openrpc_router_with_auth(info, servers, auth_required)
    }
}

impl<S> RpcRouterOpenRpcExt for ReadyRpcRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn openrpc(&self, info: OpenRpcInfo, servers: Vec<OpenRpcServer>) -> OpenRpcDocument {
        self.openrpc_with_auth(info, servers, &BTreeSet::new())
    }

    fn openrpc_with_auth(
        &self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> OpenRpcDocument {
        build_document(info, servers, &self.method_metas(), auth_required)
    }

    fn into_axum_openrpc_router(self) -> AxumRouter {
        self.into_axum_openrpc_router_with_info(OpenRpcInfo::default())
    }

    fn into_axum_openrpc_router_with_info(self, info: OpenRpcInfo) -> AxumRouter {
        self.into_axum_openrpc_router_with(info, Vec::new())
    }

    fn into_axum_openrpc_router_with(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
    ) -> AxumRouter {
        let doc = self.openrpc(info, servers);
        mount_openrpc(self, doc)
    }

    fn into_axum_openrpc_router_with_auth(
        self,
        info: OpenRpcInfo,
        servers: Vec<OpenRpcServer>,
        auth_required: &BTreeSet<String>,
    ) -> AxumRouter {
        let doc = self.openrpc_with_auth(info, servers, auth_required);
        mount_openrpc(self, doc)
    }
}

fn mount_openrpc<S>(router: ReadyRpcRouter<S>, doc: OpenRpcDocument) -> AxumRouter
where
    S: Clone + Send + Sync + 'static,
{
    let (status, body) = match serde_json::to_vec(&doc) {
        Ok(bytes) => (StatusCode::OK, Bytes::from(bytes)),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Bytes::from_static(br#"{"error":"failed to serialize openrpc document"}"#),
        ),
    };

    router.into_axum_rpc_router().route(
        "/openrpc.json",
        get(move || {
            let body = body.clone();
            async move {
                (
                    status,
                    [
                        (header::CONTENT_TYPE, JSON_CONTENT_TYPE),
                        // Docs regenerate per build; debug tools must not see stale copies.
                        (header::CACHE_CONTROL, "no-store"),
                    ],
                    body,
                )
                    .into_response()
            }
        }),
    )
}
