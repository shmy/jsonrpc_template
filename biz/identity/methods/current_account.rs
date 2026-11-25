use axum::extract::State;
use axum_jsonrpc::rpc;
use blob_kit::Blob;
use http_auth::AuthedAccount;
use identity_contract::{Account, AccountView};
use rootcause::Result;
use toasty::Db;

use crate::shared::avatar_ops;

/// 获取当前登录账号（PrivateRpc；无参，id 来自 Bearer）
#[rpc("identity.account.current")]
#[tracing::instrument(skip(db, blob), fields(%caller))]
pub async fn current_account(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
    State(blob): State<Blob>,
) -> Result<AccountView> {
    let account = Account::require_by_id(&mut db, caller).await?;
    Ok(avatar_ops::into_view(account, &blob))
}
