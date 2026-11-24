use account_id::Id;
use axum::extract::State;
use axum_jsonrpc::{Params, rpc};
use blob_kit::Blob;
use identity_contract::{Account, AccountView};
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;

use crate::shared::avatar_ops;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadAccountRequest {
    /// 账号ID
    pub id: Id,
}

/// 读取账号（PrivateRpc；任意登录可读）
#[rpc("identity.account.read")]
#[tracing::instrument(skip(db, blob, request))]
pub async fn read_account(
    State(mut db): State<Db>,
    State(blob): State<Blob>,
    Params(request): Params<ReadAccountRequest>,
) -> Result<AccountView> {
    let account = Account::require_by_id(&mut db, request.id).await?;
    Ok(avatar_ops::into_view(account, &blob))
}
