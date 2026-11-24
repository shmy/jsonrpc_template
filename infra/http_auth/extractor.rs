use crate::locale::{authn_rpc_code, authn_unauthorized_body};
use account_id::Id;
use authn_kit::AuthnError;
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum_jsonrpc::{ErrorObject, IntoExtractError};
use locale::translate_current;

/// 已通过鉴权中间件写入 extensions 的当前账号。
#[derive(Clone, Copy, Debug)]
pub struct AuthedAccount(pub Id);

#[derive(Debug)]
pub struct AuthedAccountRejection(AuthnError);

impl From<AuthnError> for AuthedAccountRejection {
    fn from(value: AuthnError) -> Self {
        Self(value)
    }
}

impl IntoExtractError for AuthedAccountRejection {
    fn into_extract_error(self) -> ErrorObject {
        // 码段与 FTL 由壳层自持（见 crate::locale）；authn_kit 保持协议无关
        ErrorObject::new(
            authn_rpc_code(&self.0),
            translate_current(&self.0.to_string()),
        )
    }
}

impl IntoResponse for AuthedAccountRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(authn_unauthorized_body(&self.0)),
        )
            .into_response()
    }
}

impl<S> FromRequestParts<S> for AuthedAccount
where
    S: Send + Sync + 'static,
{
    type Rejection = AuthedAccountRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        account_from_parts(parts)
    }
}

fn account_from_parts(parts: &Parts) -> Result<AuthedAccount, AuthedAccountRejection> {
    if let Some(err) = parts.extensions.get::<AuthnError>().cloned() {
        return Err(AuthedAccountRejection(err));
    }
    let Some(account) = parts.extensions.get::<AuthedAccount>() else {
        return Err(AuthedAccountRejection(AuthnError::AccessTokenMissing));
    };
    Ok(*account)
}

/// `RpcRouter::before_each` gate for private RPC method tables.
///
/// Reads extensions written by soft auth middleware; same codes as [`AuthedAccount`].
pub fn require_authed_account<S>(parts: &mut Parts, _state: &S) -> Result<(), ErrorObject> {
    account_from_parts(parts)
        .map(|_| ())
        .map_err(IntoExtractError::into_extract_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::{codes, ensure_locales_for_test};
    use axum::body::Body;
    use axum::http::Request;

    fn empty_parts() -> Parts {
        Request::builder()
            .body(Body::empty())
            .expect("request")
            .into_parts()
            .0
    }

    #[test]
    fn require_authed_missing_token() {
        ensure_locales_for_test();
        let mut parts = empty_parts();
        let err = require_authed_account(&mut parts, &()).expect_err("missing");
        assert_eq!(err.code, codes::ACCESS_TOKEN_MISSING);
    }

    #[test]
    fn require_authed_propagates_authn_error() {
        ensure_locales_for_test();
        let mut parts = empty_parts();
        parts.extensions.insert(AuthnError::AccessTokenInvalid);
        let err = require_authed_account(&mut parts, &()).expect_err("invalid");
        assert_eq!(err.code, codes::ACCESS_TOKEN_INVALID);
    }

    #[test]
    fn require_authed_ok_with_account() {
        ensure_locales_for_test();
        let mut parts = empty_parts();
        parts
            .extensions
            .insert(AuthedAccount(Id::new_unchecked(42)));
        require_authed_account(&mut parts, &()).expect("ok");
    }
}
