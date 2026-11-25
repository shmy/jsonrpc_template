use axum::extract::State;
use axum_jsonrpc::rpc;
use cache_kit::Cache;
use http_auth::AuthedAccount;
use jwt_kit::TokenHelper;
use rootcause::Result;

use crate::shared::token_ops;

/// 登出并吊销当前会话
#[rpc("identity.account.logout")]
#[tracing::instrument(skip(cache, token_helper))]
pub async fn logout_account(
    AuthedAccount(account_id): AuthedAccount,
    State(cache): State<Cache>,
    State(token_helper): State<TokenHelper>,
) -> Result<()> {
    token_ops::revoke_tokens(&cache, &token_helper, account_id).await?;
    Ok(())
}
