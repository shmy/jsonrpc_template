use app_state::AuthRateLimit;
use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use cache_kit::Cache;
use http_extractors::ClientIp;
use identity_contract::{IdentityError, RefreshToken};
use jwt_kit::TokenHelper;
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use validify::Validate;

use crate::shared::rate_limit::{AuthFace, enforce_auth_ip};
use crate::shared::token_ops;
use crate::shared::token_response::TokenResponse;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct RefreshAccountRequest {
    /// 刷新令牌
    #[validate(length(min = 1, max = 128, code = "refresh_token_invalid_length"))]
    pub refresh_token: RefreshToken,
}

/// 刷新令牌并颁发新的访问令牌
#[rpc("identity.account.refresh")]
#[tracing::instrument(skip(cache, token_helper, request))]
pub async fn refresh_account(
    State(cache): State<Cache>,
    State(token_helper): State<TokenHelper>,
    State(auth_rate): State<AuthRateLimit>,
    ip: ClientIp,
    ValidatedParams(request): ValidatedParams<RefreshAccountRequest>,
) -> Result<TokenResponse> {
    enforce_auth_ip(
        &cache,
        auth_rate,
        AuthFace::Refresh,
        &ip,
        IdentityError::AuthRateLimited,
    )
    .await?;

    let tokens =
        token_ops::rotate_refresh(&cache, &token_helper, request.refresh_token.as_str()).await?;
    Ok(TokenResponse::bearer(tokens))
}
