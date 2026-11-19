use app_state::{AuthRateLimit, LoginPhoneRate};
use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use cache_kit::Cache;
use http_extractors::ClientIp;
use identity_contract::{Account, IdentityError, Password};
use jwt_kit::TokenHelper;
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;
use validify::Validate;

use crate::shared::hashed_password::HashedPassword;
use crate::shared::rate_limit::{AuthFace, enforce_auth_ip, enforce_login_phone};
use crate::shared::token_ops;
use crate::shared::token_response::TokenResponse;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct LoginAccountRequest {
    /// 手机号
    #[schemars(example = &"13888888888")]
    #[validate(
        length(min = 11, max = 11, code = "phone_invalid_length"),
        custom(function = identity_contract::validate_phone_format)
    )]
    pub phone: String,
    /// 密码
    #[schemars(example = &"admin123!")]
    #[validate(
        length(min = 8, max = 64, code = "password_invalid_length"),
        non_control_char(code = "password_control_chars")
    )]
    pub password: Password,
}

/// 登录并颁发访问/刷新令牌
#[rpc("identity.account.login")]
#[tracing::instrument(skip(db, cache, token_helper, request))]
pub async fn login_account(
    State(mut db): State<Db>,
    State(cache): State<Cache>,
    State(token_helper): State<TokenHelper>,
    State(auth_rate): State<AuthRateLimit>,
    State(login_phone_rate): State<LoginPhoneRate>,
    ip: ClientIp,
    ValidatedParams(request): ValidatedParams<LoginAccountRequest>,
) -> Result<TokenResponse> {
    enforce_auth_ip(
        &cache,
        auth_rate,
        AuthFace::Login,
        &ip,
        IdentityError::LoginRateLimited,
    )
    .await?;
    enforce_login_phone(
        &cache,
        login_phone_rate,
        &request.phone,
        IdentityError::LoginRateLimited,
    )
    .await?;

    let Some(account) = Account::filter_by_phone(&request.phone)
        .first()
        .exec(&mut db)
        .await?
    else {
        HashedPassword::equalize_timing().await;
        return Err(IdentityError::AccountInvalidCredentials.into());
    };

    let hashed = HashedPassword::new_unchecked(account.password);
    if hashed.verify(request.password.as_str()).await.is_err() {
        return Err(IdentityError::AccountInvalidCredentials.into());
    }

    let tokens = token_ops::issue_tokens(&cache, &token_helper, &account.id).await?;
    Ok(TokenResponse::bearer(tokens))
}
