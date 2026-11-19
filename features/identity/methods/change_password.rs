use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use cache_kit::Cache;
use http_auth::AuthedAccount;
use identity_contract::{Account, IdentityError, Password};
use jwt_kit::TokenHelper;
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;
use validify::Validate;

use crate::shared::hashed_password::HashedPassword;
use crate::shared::token_ops;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct ChangePasswordRequest {
    /// 旧密码
    #[validate(
        length(min = 8, max = 64, code = "password_invalid_length"),
        non_control_char(code = "password_control_chars")
    )]
    pub old_password: Password,
    /// 新密码
    #[validate(
        length(min = 8, max = 64, code = "password_invalid_length"),
        non_control_char(code = "password_control_chars")
    )]
    pub new_password: Password,
}

/// 修改调用者的密码并吊销所有会话
#[rpc("identity.account.change_password")]
#[tracing::instrument(skip(db, cache, token_helper, request))]
pub async fn change_password(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
    State(cache): State<Cache>,
    State(token_helper): State<TokenHelper>,
    ValidatedParams(request): ValidatedParams<ChangePasswordRequest>,
) -> Result<()> {
    if request.old_password.as_str() == request.new_password.as_str() {
        return Err(IdentityError::AccountPasswordUnchanged.into());
    }

    let mut account = Account::require_by_id(&mut db, caller).await?;
    let hashed = HashedPassword::new_unchecked(account.password.clone());
    if hashed.verify(request.old_password.as_str()).await.is_err() {
        return Err(IdentityError::IncorrectOriginalPassword.into());
    }

    let new_hashed = HashedPassword::try_new(request.new_password.as_str()).await?;
    account
        .update()
        .password(new_hashed.into_string())
        .exec(&mut db)
        .await?;

    token_ops::revoke_tokens(&cache, &token_helper, caller).await?;
    Ok(())
}
