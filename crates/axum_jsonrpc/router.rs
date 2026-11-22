use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use axum::Router as AxumRouter;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::post;
use futures_util::stream::{self, StreamExt};
use http_body_util::LengthLimitError;
use serde_json::value::RawValue;
use tracing::field::{Empty, display};
use tracing::{Instrument, Span, debug, info_span};

use super::handler::{BeforeEachFn, BeforeEachMethod, ErasedMethod, MethodHandler, RpcHandler};
use super::meta::{MethodMeta, MethodSpec, Registrable};
use super::observer::{RpcObserver, SharedObserver, default_observer};
use super::params::RpcParams;
use super::parse::{ParseCall, TopCall, parse_call, parse_top, validate_call};
use super::trace_id::{ResolvedRequestId, resolve_trace_id_with_fallback, set_request_id_header};
use super::types::{
    Call, DEFAULT_BATCH_CONCURRENCY, ErrorObject, Id, MAX_BATCH_LEN, MAX_BODY_BYTES, Response,
};

/// Optional fallback when request headers have no valid trace id (e.g. current OTel span).
pub type TraceIdFallback = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Error while registering or merging RPC methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    ReservedMethod(String),
    DuplicateMethod(String),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedMethod(name) => {
                write!(f, "method name `{name}` is reserved (rpc.* prefix)")
            }
            Self::DuplicateMethod(name) => {
                write!(f, "method `{name}` is already registered")
            }
        }
    }
}

impl Error for RegisterError {}

/// Method table typed with state `S` (axum-like `Router::<S>` before `with_state`).
///
/// Register / merge here, then [`Self::with_state`] to serve.
#[derive(Clone)]
pub struct RpcRouter<S = ()> {
    methods: HashMap<String, Arc<dyn ErasedMethod<S>>>,
    metas: HashMap<String, MethodMeta>,
    /// Max in-flight calls per batch. See [`Self::batch_concurrency`].
    batch_concurrency: usize,
    /// Max HTTP body bytes. See [`Self::max_body_bytes`].
    max_body_bytes: usize,
    /// Max calls in one batch. See [`Self::max_batch_len`].
    max_batch_len: usize,
    observer: SharedObserver,
    trace_id_fallback: Option<TraceIdFallback>,
    /// Sticky gate: wraps existing methods and every later [`Self::method`] / [`Self::register`].
    /// Not applied to handlers brought in via [`Self::merge`] (they keep their own wrapping).
    before_each: Option<BeforeEachFn<S>>,
    _state: PhantomData<fn() -> S>,
}

/// [`RpcRouter`] after [`RpcRouter::with_state`] — ready to dispatch / mount on axum.
#[derive(Clone)]
pub struct ReadyRpcRouter<S> {
    methods: HashMap<String, Arc<dyn ErasedMethod<S>>>,
    metas: HashMap<String, MethodMeta>,
    state: S,
    batch_concurrency: usize,
    max_body_bytes: usize,
    max_batch_len: usize,
    observer: SharedObserver,
    trace_id_fallback: Option<TraceIdFallback>,
}

impl Default for RpcRouter<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> RpcRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Empty method table typed with `S` (no state value yet).
    pub fn new() -> Self {
        Self {
            methods: HashMap::new(),
            metas: HashMap::new(),
            batch_concurrency: DEFAULT_BATCH_CONCURRENCY,
            max_body_bytes: MAX_BODY_BYTES,
            max_batch_len: MAX_BATCH_LEN,
            observer: default_observer(),
            trace_id_fallback: None,
            before_each: None,
            _state: PhantomData,
        }
    }

    /// Attach process state before serving (axum-style).
    pub fn with_state(self, state: S) -> ReadyRpcRouter<S> {
        ReadyRpcRouter {
            methods: self.methods,
            metas: self.metas,
            state,
            batch_concurrency: self.batch_concurrency,
            max_body_bytes: self.max_body_bytes,
            max_batch_len: self.max_batch_len,
            observer: self.observer,
            trace_id_fallback: self.trace_id_fallback,
        }
    }

    /// Set max in-flight method calls while processing one batch request.
    ///
    /// Spec §6 allows concurrent batch execution. Response order still matches
    /// request order (client correlation uses `id` either way).
    ///
    /// - `1` — serial
    /// - `n > 1` — at most `n` concurrent calls
    /// - `0` — unbounded (up to batch length)
    pub fn batch_concurrency(mut self, n: usize) -> Self {
        self.batch_concurrency = n;
        self
    }

    /// Override HTTP body size limit (default [`MAX_BODY_BYTES`]).
    pub fn max_body_bytes(mut self, n: usize) -> Self {
        self.max_body_bytes = n;
        self
    }

    /// Override max calls allowed in one batch (default [`MAX_BATCH_LEN`]).
    pub fn max_batch_len(mut self, n: usize) -> Self {
        self.max_batch_len = n;
        self
    }

    /// Install a metrics / telemetry observer (default is no-op).
    pub fn observer(mut self, observer: Arc<dyn RpcObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Fallback when headers lack `traceparent` / `x-request-id` (e.g. OTel current span).
    pub fn trace_id_fallback(
        mut self,
        fallback: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.trace_id_fallback = Some(Arc::new(fallback));
        self
    }

    pub fn method<H, T>(
        mut self,
        name: impl Into<String>,
        handler: H,
    ) -> Result<Self, RegisterError>
    where
        H: RpcHandler<T, S> + Send + Sync + 'static,
        T: 'static,
    {
        let name = name.into();
        self.insert_method(name.clone(), handler)?;
        // Skeleton meta so `/openrpc.json` stays in sync with callable methods.
        self.metas.insert(name.clone(), MethodMeta::new(name));
        Ok(self)
    }

    /// Register a `#[rpc]` handle or [`MethodSpec`] (no need to call a helper fn).
    pub fn register(self, method: impl Registrable<S>) -> Result<Self, RegisterError> {
        method.register(self)
    }

    /// Register handler + OpenRPC metadata together.
    pub fn register_spec<H, T>(mut self, spec: MethodSpec<H, T>) -> Result<Self, RegisterError>
    where
        H: RpcHandler<T, S> + Send + Sync + 'static,
        T: 'static,
    {
        let MethodSpec {
            name,
            handler,
            meta,
            ..
        } = spec;
        self.insert_method(name.clone(), handler)?;
        self.metas.insert(name, meta);
        Ok(self)
    }

    fn insert_method<H, T>(&mut self, name: String, handler: H) -> Result<(), RegisterError>
    where
        H: RpcHandler<T, S> + Send + Sync + 'static,
        T: 'static,
    {
        // Spec §4 / §8: names beginning with "rpc." are reserved.
        if name.starts_with("rpc.") {
            return Err(RegisterError::ReservedMethod(name));
        }
        if self.methods.contains_key(&name) {
            return Err(RegisterError::DuplicateMethod(name));
        }
        let mut erased: Arc<dyn ErasedMethod<S>> = Arc::new(MethodHandler {
            handler,
            _marker: PhantomData,
        });
        if let Some(check) = &self.before_each {
            erased = Arc::new(BeforeEachMethod {
                check: Arc::clone(check),
                inner: erased,
            });
        }
        self.methods.insert(name, erased);
        Ok(())
    }

    /// Merge methods from `other`.
    ///
    /// Keeps `self`'s batch concurrency and observer. `trace_id_fallback`
    /// prefers `self`, otherwise takes `other`'s.
    ///
    /// Handlers already wrapped by [`Self::before_each`] keep that gate after merge.
    /// `self`'s sticky [`Self::before_each`] is **not** applied to merged-in methods
    /// (so public + private fragments can share one table).
    pub fn merge(mut self, mut other: Self) -> Result<Self, RegisterError> {
        self.methods.reserve(other.methods.len());
        self.metas.reserve(other.methods.len());

        if self.trace_id_fallback.is_none() {
            self.trace_id_fallback = other.trace_id_fallback.take();
        }

        for (name, handler) in other.methods.drain() {
            if self.methods.contains_key(&name) {
                return Err(RegisterError::DuplicateMethod(name));
            }
            let meta = other
                .metas
                .remove(&name)
                .unwrap_or_else(|| MethodMeta::new(name.clone()));
            self.methods.insert(name.clone(), handler);
            self.metas.insert(name, meta);
        }
        Ok(self)
    }

    /// Run `check` before every method on **this** router (existing + later registers).
    ///
    /// Apply on a private fragment before [`Self::merge`] into a larger table so only
    /// that fragment is gated. Calling again stacks another gate outside the previous.
    /// Merged-in methods are not retroactively wrapped by this router's sticky check.
    pub fn before_each(
        mut self,
        check: impl Fn(&mut Parts, &S) -> Result<(), ErrorObject> + Send + Sync + 'static,
    ) -> Self {
        let check: BeforeEachFn<S> = Arc::new(check);
        for handler in self.methods.values_mut() {
            *handler = Arc::new(BeforeEachMethod {
                check: Arc::clone(&check),
                inner: Arc::clone(handler),
            });
        }
        self.before_each = Some(check);
        self
    }

    /// Registered method names (sorted).
    pub fn method_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.methods.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    /// Registered method metadata (sorted by name). Used by OpenRPC generation.
    pub fn method_metas(&self) -> Vec<MethodMeta> {
        sorted_method_metas(&self.metas)
    }
}

impl RpcRouter<()> {
    /// Convenience for unit state: `with_state(())` then mount `POST /jsonrpc`.
    pub fn into_axum_rpc_router(self) -> AxumRouter {
        self.with_state(()).into_axum_rpc_router()
    }

    #[cfg(test)]
    pub(crate) async fn dispatch_bytes(
        &self,
        parts: Parts,
        bytes: &[u8],
        trace_id: &str,
    ) -> DispatchOutcome {
        self.clone()
            .with_state(())
            .dispatch_bytes(parts, bytes, trace_id)
            .await
    }
}

impl<S> ReadyRpcRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Replace process state. Method table is unchanged.
    pub fn with_state(mut self, state: S) -> Self {
        self.state = state;
        self
    }

    /// Registered method metadata (sorted by name). Used by OpenRPC generation.
    pub fn method_metas(&self) -> Vec<MethodMeta> {
        sorted_method_metas(&self.metas)
    }

    /// Mount `POST /jsonrpc` only.
    ///
    /// For `GET /openrpc.json` as well, use
    /// `axum_jsonrpc_openrpc::RpcRouterOpenRpcExt`.
    pub fn into_axum_rpc_router(self) -> AxumRouter {
        let router = Arc::new(self);

        AxumRouter::new().route(
            "/jsonrpc",
            post({
                let router = Arc::clone(&router);
                move |req: Request| {
                    let router = Arc::clone(&router);
                    async move { router.handle_http(req).await }
                }
            }),
        )
    }

    async fn handle_http(&self, req: Request) -> HttpResponse {
        // Prefer outer-layer [`ResolvedRequestId`] (timeout / body-limit) so mint
        // matches transport errors; otherwise resolve once here.
        let (parts, body) = req.into_parts();
        let trace_id = self.resolve_trace_id_for_parts(&parts);

        if !is_json_content_type(parts.headers.get(header::CONTENT_TYPE)) {
            // jsonrpsee: 415 + plain text.
            return with_request_id(
                (
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "Supplied content type is not allowed. Content-Type: application/json is required\n",
                )
                    .into_response(),
                &trace_id,
            );
        }

        // Span name after parse (`otel.name` can't be renamed post-enter).
        let max_body = self.max_body_bytes;
        let bytes = match to_bytes(body, max_body).await {
            Ok(bytes) => bytes,
            Err(err) if is_length_limit_error(&err) => {
                return self
                    .http_error(&trace_id, 0, || {
                        rpc_error_response(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            ErrorObject::oversized_request(max_body),
                        )
                    })
                    .await;
            }
            Err(err) => {
                debug!(error = %err, "failed to read jsonrpc body");
                return self
                    .http_error(&trace_id, 0, || {
                        rpc_error_response(StatusCode::BAD_REQUEST, ErrorObject::parse_error())
                    })
                    .await;
            }
        };

        let top = match parse_top(&bytes) {
            Ok(top) => top,
            Err(()) => {
                return self
                    .http_error(&trace_id, bytes.len(), || {
                        rpc_error_response(StatusCode::OK, ErrorObject::parse_error())
                    })
                    .await;
            }
        };

        let span = root_span(&top, bytes.len());
        async move {
            Span::current().record("rpc.trace_id", display(&trace_id));
            // Single: this span is the method span. Batch: nested per-call spans.
            with_request_id(
                self.dispatch(parts, top, &trace_id, false)
                    .await
                    .into_response(),
                &trace_id,
            )
        }
        .instrument(span)
        .await
    }

    fn resolve_trace_id_for_parts(&self, parts: &Parts) -> String {
        if let Some(ResolvedRequestId(id)) = parts.extensions.get::<ResolvedRequestId>() {
            return id.clone();
        }
        resolve_trace_id_with_fallback(&parts.headers, || {
            self.trace_id_fallback.as_ref().and_then(|f| f())
        })
    }

    async fn http_error(
        &self,
        trace_id: &str,
        body_bytes: usize,
        make: impl FnOnce() -> HttpResponse,
    ) -> HttpResponse {
        with_http_span(trace_id, body_bytes, async {
            with_request_id(make(), trace_id)
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn dispatch_bytes(
        &self,
        parts: Parts,
        bytes: &[u8],
        trace_id: &str,
    ) -> DispatchOutcome {
        match parse_top(bytes) {
            Ok(top) => self.dispatch(parts, top, trace_id, true).await,
            Err(()) => {
                DispatchOutcome::Single(Response::error(Id::Null, ErrorObject::parse_error()))
            }
        }
    }

    /// `single_nested`: batch calls always get their own `jsonrpc.call` span;
    /// HTTP single-call path reuses the root span as the method span (`false`).
    async fn dispatch(
        &self,
        parts: Parts,
        top: TopCall<'_>,
        trace_id: &str,
        single_nested: bool,
    ) -> DispatchOutcome {
        match top {
            TopCall::Batch(items) => self.dispatch_batch(parts, items, trace_id).await,
            TopCall::Single(call) => self
                .dispatch_one(parts, validate_call(call), trace_id, single_nested)
                .await
                .map_or(DispatchOutcome::Empty, DispatchOutcome::Single),
            TopCall::ScalarRoot => self
                .dispatch_one(
                    parts,
                    ParseCall::Invalid {
                        id: Id::Null,
                        error: ErrorObject::invalid_request(),
                    },
                    trace_id,
                    single_nested,
                )
                .await
                .map_or(DispatchOutcome::Empty, DispatchOutcome::Single),
        }
    }

    async fn dispatch_batch(
        &self,
        parts: Parts,
        items: Vec<&RawValue>,
        trace_id: &str,
    ) -> DispatchOutcome {
        if items.is_empty() {
            return DispatchOutcome::Single(Response::error(
                Id::Null,
                ErrorObject::invalid_request(),
            ));
        }
        if items.len() > self.max_batch_len {
            // jsonrpsee uses implementation-defined -32010 (not -32600).
            return DispatchOutcome::Single(Response::error(
                Id::Null,
                ErrorObject::too_big_batch_request(self.max_batch_len),
            ));
        }

        self.observer.on_batch(items.len());

        let limit = match self.batch_concurrency {
            0 => items.len().max(1),
            n => n,
        };

        // Pre-parse all items up front (sync, bounded by MAX_BATCH_LEN): each
        // `ParseCall` is fully owned, so per-call futures borrow nothing from the body.
        let parsed: Vec<ParseCall> = items.iter().map(|item| parse_call(item.get())).collect();

        // `buffered` caps in-flight futures and preserves input order.
        // Batch only: clone Parts per call; single-call path moves Parts once.
        let results: Vec<Option<Response>> = stream::iter(parsed)
            .map(|parsed| {
                let parts = parts.clone();
                self.dispatch_one(parts, parsed, trace_id, true)
            })
            .buffered(limit)
            .collect()
            .await;

        let responses: Vec<Response> = results.into_iter().flatten().collect();
        if responses.is_empty() {
            DispatchOutcome::Empty
        } else {
            DispatchOutcome::Batch(responses)
        }
    }

    async fn dispatch_one(
        &self,
        parts: Parts,
        parsed: ParseCall,
        trace_id: &str,
        nested_call_span: bool,
    ) -> Option<Response> {
        match parsed {
            ParseCall::Invalid { id, error } => {
                mark_rpc_error(&error);
                Some(Response::error(id, error))
            }
            ParseCall::Dropped => None,
            ParseCall::Notification(call) => {
                let method = call.method.clone();
                let run = async {
                    let started = Instant::now();
                    let err = self.invoke(parts, call).await.err();
                    self.finish_call(&method, err.as_ref(), started);
                };
                if nested_call_span {
                    run.instrument(call_span(&method, "none", trace_id)).await;
                } else {
                    run.await;
                }
                None
            }
            ParseCall::Request { call, id } => {
                let method = call.method.clone();
                let id_label = id_display(&id);
                let run = async {
                    if !nested_call_span {
                        Span::current().record("rpc.id", display(&id_label));
                    }
                    let started = Instant::now();
                    match self.invoke(parts, call).await {
                        Ok(result) => {
                            self.finish_call(&method, None, started);
                            Some(Response::success(id, result))
                        }
                        Err(error) => {
                            self.finish_call(&method, Some(&error), started);
                            Some(Response::error(id, error))
                        }
                    }
                };
                if nested_call_span {
                    run.instrument(call_span(&method, &id_label, trace_id))
                        .await
                } else {
                    run.await
                }
            }
        }
    }

    fn finish_call(&self, method: &str, error: Option<&ErrorObject>, started: Instant) {
        match error {
            Some(err) => mark_rpc_error(err),
            None => {
                Span::current().record("rpc.error_code", 0);
            }
        }
        self.observer
            .on_call(method, error.map(|e| e.code), started.elapsed());
    }

    async fn invoke(&self, parts: Parts, call: Call) -> Result<Box<RawValue>, ErrorObject> {
        let Some(handler) = self.methods.get(&call.method) else {
            return Err(ErrorObject::method_not_found());
        };

        let mut req = Request::from_parts(parts, Body::empty());
        req.extensions_mut().insert(RpcParams(call.params));
        handler.call(req, self.state.clone()).await
    }
}

fn sorted_method_metas(metas: &HashMap<String, MethodMeta>) -> Vec<MethodMeta> {
    let mut metas: Vec<MethodMeta> = metas.values().cloned().collect();
    metas.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    metas
}

fn id_display(id: &Id) -> String {
    match id {
        Id::Number(n) => n.to_string(),
        Id::String(s) => s.clone(),
        Id::Null => "null".into(),
    }
}

async fn with_http_span<F, T>(trace_id: &str, body_bytes: usize, fut: F) -> T
where
    F: Future<Output = T>,
{
    fut.instrument(info_span!(
        "jsonrpc.http",
        otel.name = "jsonrpc.http",
        rpc.trace_id = %trace_id,
        rpc.body_bytes = body_bytes,
        error_message = "jsonrpc http error",
        otel.status_code = "ERROR",
        otel.status_description = "jsonrpc http error",
    ))
    .await
}

fn root_span(top: &TopCall<'_>, body_bytes: usize) -> tracing::Span {
    match top {
        TopCall::Batch(_) => info_span!(
            "jsonrpc.batch",
            otel.name = "jsonrpc.batch",
            rpc.method = "batch",
            rpc.trace_id = Empty,
            rpc.body_bytes = body_bytes,
            rpc.error_code = Empty,
            error_message = Empty,
            otel.status_code = Empty,
            otel.status_description = Empty,
        ),
        TopCall::Single(call) => match call.method_name() {
            Some(method) => info_span!(
                "jsonrpc.call",
                otel.name = %method,
                rpc.method = %method,
                rpc.id = Empty,
                rpc.trace_id = Empty,
                rpc.body_bytes = body_bytes,
                rpc.error_code = Empty,
                error_message = Empty,
                otel.status_code = Empty,
                otel.status_description = Empty,
            ),
            None => info_span!(
                "jsonrpc.http",
                otel.name = "jsonrpc.http",
                rpc.trace_id = Empty,
                rpc.body_bytes = body_bytes,
                error_message = Empty,
                otel.status_code = Empty,
                otel.status_description = Empty,
            ),
        },
        TopCall::ScalarRoot => info_span!(
            "jsonrpc.http",
            otel.name = "jsonrpc.http",
            rpc.trace_id = Empty,
            rpc.body_bytes = body_bytes,
            error_message = Empty,
            otel.status_code = Empty,
            otel.status_description = Empty,
        ),
    }
}

fn call_span(method: &str, id_label: &str, trace_id: &str) -> tracing::Span {
    info_span!(
        "jsonrpc.call",
        otel.name = %method,
        rpc.method = %method,
        rpc.id = %id_label,
        rpc.trace_id = %trace_id,
        rpc.error_code = Empty,
        error_message = Empty,
        otel.status_code = Empty,
        otel.status_description = Empty,
    )
}

fn mark_rpc_error(error: &ErrorObject) {
    let span = Span::current();
    span.record("rpc.error_code", error.code);
    span.record("error_message", error.message.as_str());
    span.record("otel.status_code", "ERROR");
    span.record(
        "otel.status_description",
        format!("{}: {}", error.code, error.message),
    );
}

#[derive(Debug)]
pub(crate) enum DispatchOutcome {
    Empty,
    Single(Response),
    Batch(Vec<Response>),
}

impl IntoResponse for DispatchOutcome {
    fn into_response(self) -> HttpResponse {
        match self {
            // Spec §6: no Response objects → must not return `[]`.
            // jsonrpsee HTTP ACK: 200 + JSON `null` body.
            Self::Empty => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
                Body::from("null"),
            )
                .into_response(),
            Self::Single(response) => rpc_status_response(StatusCode::OK, &response),
            Self::Batch(responses) => rpc_status_response(StatusCode::OK, &responses),
        }
    }
}

/// Echo the resolved correlation id on every `/jsonrpc` HTTP response.
fn with_request_id(mut response: HttpResponse, trace_id: &str) -> HttpResponse {
    set_request_id_header(&mut response, trace_id);
    response
}

/// Response `Content-Type` used by jsonrpsee HTTP.
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

pub(crate) fn is_json_content_type(value: Option<&HeaderValue>) -> bool {
    let Some(raw) = value.and_then(|v| v.to_str().ok()) else {
        return false;
    };

    let mut parts = raw.split(';');
    let Some(mime) = parts.next().map(str::trim) else {
        return false;
    };
    let is_json = mime.eq_ignore_ascii_case("application/json")
        || mime.eq_ignore_ascii_case("application/json-rpc");
    if !is_json {
        return false;
    }

    // Accept bare mime, or a single `charset=utf-8` parameter (jsonrpsee-compatible).
    match parts.next().map(str::trim) {
        None => true,
        Some(param) => {
            let Some((key, val)) = param.split_once('=') else {
                return false;
            };
            key.trim().eq_ignore_ascii_case("charset")
                && val.trim().eq_ignore_ascii_case("utf-8")
                && parts.next().is_none()
        }
    }
}

fn is_length_limit_error(err: &axum::Error) -> bool {
    Error::source(err)
        .map(|source| source.is::<LengthLimitError>())
        .unwrap_or(false)
}

fn rpc_error_response(status: StatusCode, error: ErrorObject) -> HttpResponse {
    rpc_status_response(status, &Response::error(Id::Null, error))
}

fn rpc_status_response(status: StatusCode, body: &impl serde::Serialize) -> HttpResponse {
    match serde_json::to_vec(body) {
        Ok(bytes) => (status, [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)], bytes).into_response(),
        Err(_) => {
            // Prefer the same localized -32603 path as handlers; last resort is ASCII-only.
            // Correlation id is on `x-request-id` (set by caller via `with_request_id`).
            let payload = serde_json::to_vec(&Response::error(
                Id::Null,
                ErrorObject::internal_error(),
            ))
            .unwrap_or_else(|_| {
                br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":null}"#
                    .to_vec()
            });
            (status, [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)], payload).into_response()
        }
    }
}
