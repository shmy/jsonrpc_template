use authn_kit::AuthnError;
use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::extractor::AuthedAccount;
use crate::locale::authn_unauthorized_body;
use crate::token::{access_token_from_headers, access_token_from_headers_optional};
use account_id::Id;
use app_state::AppState;

/// 验证 Bearer access token 并解析出当前账号。
///
/// 步骤：JWT 验签/过期校验 → Redis 比对当前会话 jti（单会话模型）→ subject 解析为账号 id。
async fn verify_bearer(state: &AppState, token: &str) -> Result<AuthedAccount, AuthnError> {
    let claims = state
        .token_helper()
        .decode_access_token(token)
        .map_err(|_| AuthnError::AccessTokenInvalid)?;

    let stored_jti = match state
        .cache()
        .get::<String>(&authn_kit::access_jti_key(
            state.token_helper().realm(),
            &claims.sub,
        ))
        .await
    {
        Ok(stored) => stored,
        Err(err) => {
            tracing::error!(error = %err, "cache unavailable during token verification");
            return Err(AuthnError::CacheUnavailable);
        }
    };

    match stored_jti {
        Some(ref jti) if jti == &claims.jti => claims
            .sub
            .parse::<u64>()
            .map(Id::from)
            .map(AuthedAccount)
            .map_err(|_| AuthnError::AccessTokenInvalid),
        _ => Err(AuthnError::AccessTokenRevoked),
    }
}

/// `PrivateRouter` 强制鉴权：失败直接 HTTP 401。
pub async fn account_auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match authenticate_required(&state, request, next).await {
        Ok(response) => response,
        Err(err) => (
            StatusCode::UNAUTHORIZED,
            Json(authn_unauthorized_body(&err)),
        )
            .into_response(),
    }
}

async fn authenticate_required(
    state: &AppState,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthnError> {
    let token = access_token_from_headers(request.headers())?;
    let account = verify_bearer(state, token).await?;
    request.extensions_mut().insert(account);
    Ok(next.run(request).await)
}

/// `/jsonrpc` 软鉴权：从不短路 HTTP；成功写 `AuthedAccount`，失败写 `AuthnError`。
pub async fn optional_account_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth = match access_token_from_headers_optional(request.headers()) {
        None => None,
        Some(Err(err)) => Some(Err(err)),
        Some(Ok(token)) => Some(verify_bearer(&state, token).await),
    };
    match auth {
        Some(Ok(account)) => {
            request.extensions_mut().insert(account);
        }
        Some(Err(err)) => {
            request.extensions_mut().insert(err);
        }
        None => {}
    }
    next.run(request).await
}
