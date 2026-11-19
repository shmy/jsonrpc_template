use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use serde::Serialize;
use serde_json::value::RawValue;
use tracing::error;

use super::params::IntoExtractError;
use super::types::ErrorObject;

type BoxMethodFuture = Pin<Box<dyn Future<Output = Result<Box<RawValue>, ErrorObject>> + Send>>;

/// Sync gate run before a method handler (e.g. require auth extensions).
pub type BeforeEachFn<S> =
    Arc<dyn Fn(&mut Parts, &S) -> Result<(), ErrorObject> + Send + Sync + 'static>;

/// Async RPC method handler, modeled after axum's `Handler`.
pub trait RpcHandler<T, S>: Clone + Send + Sync + Sized + 'static {
    fn call(self, req: Request, state: S) -> BoxMethodFuture;
}

/// Dynamic `Report` → domain mappers → `-32603`.
#[cfg(feature = "rootcause")]
impl From<rootcause::Report> for ErrorObject {
    fn from(report: rootcause::Report) -> Self {
        if let Some(err) = crate::domain_mapper::try_domain_mappers(&report) {
            tracing::debug!(error = %report, "jsonrpc application error");
            return err;
        }
        error!(error = %report, "jsonrpc handler failed");
        ErrorObject::internal_error()
    }
}

trait IntoMethodResult {
    fn into_method_result(self) -> Result<Box<RawValue>, ErrorObject>;
}

impl<T, E> IntoMethodResult for Result<T, E>
where
    T: Serialize,
    E: Into<ErrorObject>,
{
    fn into_method_result(self) -> Result<Box<RawValue>, ErrorObject> {
        match self {
            // 直接序列化为 raw JSON 文本，省掉中间 Value 树。
            Ok(value) => serde_json::to_string(&value)
                .map_err(|err| {
                    error!(error = %err, "jsonrpc result serialize failed");
                    ErrorObject::internal_error()
                })
                .and_then(|s| {
                    RawValue::from_string(s).map_err(|err| {
                        error!(error = %err, "jsonrpc result encode failed");
                        ErrorObject::internal_error()
                    })
                }),
            Err(err) => Err(err.into()),
        }
    }
}

macro_rules! impl_rpc_handler {
    ( $($ty:ident),* $(,)? ) => {
        #[allow(non_snake_case, unused_mut, unused_variables)]
        impl<F, Fut, S, Res, $($ty,)*> RpcHandler<($($ty,)*), S> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send + 'static,
            Res: IntoMethodResult,
            S: Send + Sync + 'static,
            $( $ty: FromRequestParts<S> + Send, )*
            $( <$ty as FromRequestParts<S>>::Rejection: IntoExtractError + Send, )*
        {
            fn call(self, req: Request, state: S) -> BoxMethodFuture {
                Box::pin(async move {
                    let (mut parts, _body) = req.into_parts();
                    $(
                        let $ty = $ty::from_request_parts(&mut parts, &state)
                            .await
                            .map_err(IntoExtractError::into_extract_error)?;
                    )*
                    self($($ty,)*).await.into_method_result()
                })
            }
        }
    };
}

impl_rpc_handler!();
impl_rpc_handler!(T1);
impl_rpc_handler!(T1, T2);
impl_rpc_handler!(T1, T2, T3);
impl_rpc_handler!(T1, T2, T3, T4);
impl_rpc_handler!(T1, T2, T3, T4, T5);
impl_rpc_handler!(T1, T2, T3, T4, T5, T6);
impl_rpc_handler!(T1, T2, T3, T4, T5, T6, T7);
impl_rpc_handler!(T1, T2, T3, T4, T5, T6, T7, T8);

pub(crate) trait ErasedMethod<S>: Send + Sync {
    fn call(&self, req: Request, state: S) -> BoxMethodFuture;
}

pub(crate) struct MethodHandler<H, T, S> {
    pub(crate) handler: H,
    pub(crate) _marker: PhantomData<fn() -> (T, S)>,
}

impl<H, T, S> ErasedMethod<S> for MethodHandler<H, T, S>
where
    H: RpcHandler<T, S> + Send + Sync + 'static,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    fn call(&self, req: Request, state: S) -> BoxMethodFuture {
        self.handler.clone().call(req, state)
    }
}

/// Runs [`BeforeEachFn`] then delegates to the inner method.
pub(crate) struct BeforeEachMethod<S> {
    pub(crate) check: BeforeEachFn<S>,
    pub(crate) inner: Arc<dyn ErasedMethod<S>>,
}

impl<S> ErasedMethod<S> for BeforeEachMethod<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn call(&self, req: Request, state: S) -> BoxMethodFuture {
        let check = Arc::clone(&self.check);
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let (mut parts, body) = req.into_parts();
            check(&mut parts, &state)?;
            inner.call(Request::from_parts(parts, body), state).await
        })
    }
}
