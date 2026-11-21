use axum::extract::State;
use axum_jsonrpc::rpc;
use http_auth::AuthedAccount;
use identity_contract::Account;
use rootcause::Result;
use toasty::Db;

/// 获取账号个人资料（PrivateRpc；仅允许当前账号访问）
#[rpc("identity.account.profile")]
#[tracing::instrument(skip(db), fields(%caller))]
pub async fn account_profile(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
) -> Result<Account> {
    let account = Account::require_by_id(&mut db, caller).await?;
    Ok(account)
}
