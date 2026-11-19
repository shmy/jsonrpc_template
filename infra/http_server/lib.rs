//! HTTP 组合根：AppState、路由合并、监听与进程级 locale/mapper seal。

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use app_state::{AppState, AuthRateLimit, LoginPhoneRate, SlidingWindowRate};
use axum::Router;
use axum::extract::State;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum_jsonrpc::{ReadyRpcRouter, StandardErrorKind};
use axum_jsonrpc_openrpc::{OpenRpcInfo, OpenRpcServer, RpcRouterOpenRpcExt};
use blob_kit::Blob;
use cache_kit::Cache;
use config::TrustedProxies;
use config::cli::Cli;
use http_auth::{optional_account_auth, register_authn_locale};
use http_routing::{
    PUBLIC_BODY_LIMIT_BYTES, PrivateRouter, PrivateRpc, ProbeRouter, PublicRouter, PublicRpc,
};
use jwt_kit::{TokenHelper, TokenRealm};
use modrun::{Error, Group, Lifecycle, Module, Result, task_with};
use toasty::Db;
use tokio::net::TcpListener;
use tokio::time;
use tracing::info;

mod cors;
mod docs;
mod invalid_params;
mod merge;
#[cfg(feature = "otel")]
mod otel;

pub fn module() -> Module {
    // Process-global hooks（单组合根）：protocol core stays locale-free；
    // 第二 HTTP 入口勿再覆盖这些 setter。
    axum_jsonrpc::set_internal_error_message(|| locale::translate_current("internal_server_error"));
    axum_jsonrpc::set_standard_error_message(|kind| {
        locale::translate_current(match kind {
            StandardErrorKind::ParseError => "parse_error",
            StandardErrorKind::InvalidRequest => "invalid_request",
            StandardErrorKind::MethodNotFound => "method_not_found",
            StandardErrorKind::OversizedRequest => "oversized_request",
            StandardErrorKind::TooBigBatchRequest => "too_big_batch_request",
        })
    });
    axum_jsonrpc::set_invalid_params_renderer(invalid_params::render_invalid_params_cause);
    Module::builder("http_server")
        .provide(provide_app_state)
        // Merge HTTP groups here so `register` stays within modrun's 8-arg invoke limit.
        .provide(provide_http_surfaces)
        .invoke(register)
}

/// Pre-merged HTTP route trees（probe / public / private）。
///
/// - `public`：套 [`http_routing::PUBLIC_BODY_LIMIT_BYTES`]（16 MiB），**不**用更小的
///   `BODY_LIMIT_BYTES`（否则会压死预签名大上传）
/// - `private` + RPC：套 `BODY_LIMIT_BYTES`（默认 1 MiB）
#[derive(Clone)]
struct HttpSurfaces {
    probe: Router,
    public: Router,
    private: Router,
}

fn provide_http_surfaces(
    state: AppState,
    public: Group<PublicRouter>,
    private: Group<PrivateRouter>,
    probes: Group<ProbeRouter>,
) -> HttpSurfaces {
    let public = merge::merge_http(public).with_state(state.clone());
    let private = merge::merge_private(private, state.clone()).with_state(state.clone());
    let probe = merge::merge_probes(probes).with_state(state);
    HttpSurfaces {
        probe,
        public,
        private,
    }
}

fn provide_app_state(db: Db, cache: Cache, blob: Blob, cli: Cli) -> AppState {
    let token_helper = TokenHelper::new(
        TokenRealm::Account,
        cli.jwt.secret.clone(),
        cli.jwt.access_ttl.duration().as_secs(),
        cli.jwt.refresh_ttl.duration().as_secs(),
        cli.jwt.issuer.clone(),
        cli.jwt.audience.clone(),
    );
    AppState::new(
        db,
        cache,
        blob,
        token_helper,
        TrustedProxies::parse(&cli.server.trusted_proxies),
        AuthRateLimit {
            sliding_window: SlidingWindowRate {
                per_second: cli.rate_limit.auth_per_second,
                burst: cli.rate_limit.auth_burst,
            },
        },
        LoginPhoneRate {
            window: cli.rate_limit.login_phone_window.duration(),
            max_attempts: cli.rate_limit.login_phone_max_attempts,
        },
    )
}

/// API 版本（`/openrpc.json` `info.version`）。由组合根（`bin/api`）supply——
/// OpenRPC 描述的是「发布出去的 API」，版本应取自发布该 API 的二进制（`api`），
/// 而非本壳 crate 的 `CARGO_PKG_VERSION`（两者各自演进时会漂移）。
#[derive(Clone, Debug)]
pub struct OpenRpcVersion(pub &'static str);

// 参数由 modrun 按类型逐个注入（Group/State 不可打包），非设计问题
#[allow(clippy::too_many_arguments)]
fn register(
    lc: Lifecycle,
    cli: Cli,
    state: AppState,
    surfaces: HttpSurfaces,
    public_rpc: Group<PublicRpc>,
    private_rpc: Group<PrivateRpc>,
    openrpc_version: OpenRpcVersion,
) -> Result<()> {
    // All feature `module()` bodies have already run; seal before accepting traffic so
    // late register_ftl / register_domain_mapper fail loud instead of being silently dropped.
    // authn（会话基础设施）错误不是业务 feature，由壳层在此统一注册。
    register_authn_locale();
    locale::seal_bundles();
    axum_jsonrpc::seal_domain_mappers();

    let addr = SocketAddr::from_str(&cli.server.listen_addr)
        .map_err(|e| Error::hook(format!("invalid LISTEN_ADDR: {e}")))?;
    let server_url = docs::openrpc_url(addr);
    let request_timeout = cli.server.request_timeout.duration();
    let body_limit = cli.server.body_limit_bytes;
    let cors_origins = cli.server.cors_origins.clone();
    let docs_enabled = cli.server.docs_enabled;
    if !docs_enabled {
        info!("GET /docs disabled (DOCS_ENABLED=false)");
    }
    let HttpSurfaces {
        probe,
        public,
        private,
    } = surfaces;

    let (rpc, auth_required) = merge::merge_rpc(
        state.clone(),
        public_rpc,
        private_rpc,
        body_limit,
        cli.server.max_batch_len,
    )?;

    lc.append(task_with(
        "http.serve",
        move || async move {
            let listener = TcpListener::bind(addr)
                .await
                .map_err(|e| Error::io(format!("bind {addr}"), e))?;
            let bound = listener
                .local_addr()
                .map_err(|e| Error::io(format!("local_addr {addr}"), e))?;
            info!("listening on http://{bound}");
            Ok(listener)
        },
        move |listener, stopped| async move {
            let app = build_app(
                probe,
                public,
                private,
                rpc,
                auth_required,
                state,
                openrpc_version,
                server_url,
                body_limit,
                request_timeout,
                &cors_origins,
                docs_enabled,
            );
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                stopped.await;
                info!("HTTP server draining connections...");
            })
            .await
            .map_err(|e| Error::io(format!("serve {addr}"), e))?;
            Ok(())
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_app(
    probe: Router,
    public: Router,
    private: Router,
    rpc: ReadyRpcRouter<AppState>,
    auth_required: BTreeSet<String>,
    state: AppState,
    openrpc_version: OpenRpcVersion,
    server_url: String,
    body_limit: usize,
    request_timeout: Duration,
    cors_origins: &str,
    docs_enabled: bool,
) -> Router {
    // PublicRouter：统一 PUBLIC_BODY_LIMIT_BYTES（16 MiB），覆盖 Axum 默认 ~2 MiB。
    let public = public.layer(DefaultBodyLimit::max(PUBLIC_BODY_LIMIT_BYTES));

    // RPC + PrivateRouter + /docs：套 BODY_LIMIT_BYTES（默认 1 MiB）。
    let mut body_limited = Router::new().merge(private);
    if docs_enabled {
        body_limited = body_limited.route("/docs", get(docs::docs_page));
    }
    let body_limited = body_limited
        .merge(
            rpc.into_axum_openrpc_router_with_auth(
                OpenRpcInfo {
                    title: "JSON-RPC API".into(),
                    version: openrpc_version.0.into(),
                    description: Some("Public JSON-RPC methods served at POST /jsonrpc".into()),
                },
                vec![OpenRpcServer {
                    name: "local".into(),
                    url: server_url,
                }],
                &auth_required,
            )
            .layer(from_fn_with_state(state, optional_account_auth)),
        )
        .layer(DefaultBodyLimit::max(body_limit));

    let busy = Router::new()
        .merge(public)
        .merge(body_limited)
        .layer(from_fn_with_state(
            request_timeout,
            request_timeout_middleware,
        ));

    // Probes stay outside request-timeout busy tree（与业务路由分组合并）。
    let app = Router::new().merge(probe).merge(busy);
    let app = cors::finish_layers(app, cors_origins);

    #[cfg(feature = "otel")]
    {
        use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
        // Last layer = outermost: extract/propagate W3C Trace Context first.
        app.layer(OtelInResponseLayer)
            .layer(OtelAxumLayer::default())
    }
    #[cfg(not(feature = "otel"))]
    {
        app
    }
}

async fn request_timeout_middleware(
    State(timeout): State<Duration>,
    req: Request,
    next: Next,
) -> Response {
    // NOTE: on timeout the in-flight handler future is *dropped* (same semantics as
    // tower_http TimeoutLayer). Multi-step cache writes (e.g. issue_tokens) can be
    // cut mid-flight, leaving the documented half-open session window; the next
    // login/refresh overwrites the subject pointer. 60 s default makes this rare.
    match time::timeout(timeout, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            locale::translate_current("request_timeout"),
        )
            .into_response(),
    }
}
