use axum::extract::State;
use axum_jsonrpc::rpc;
use http_auth::AuthedAccount;
use identity_contract::Account;
use rootcause::Result;
use toasty::Db;

/// 获取当前登录账号（PrivateRpc；无参，id 来自 Bearer）
#[rpc("identity.account.current")]
#[tracing::instrument(skip(db), fields(%caller))]
pub async fn current_account(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
) -> Result<Account> {
    let account = Account::require_by_id(&mut db, caller).await?;
    Ok(account)
}
