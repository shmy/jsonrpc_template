use axum::extract::State;
use axum_jsonrpc::{Params, rpc};
use cache_kit::Cache;
use http_auth::AuthedAccount;
use identity_contract::Id;
use jwt_kit::TokenHelper;
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;

use crate::shared::access::load_for_delete;
use crate::shared::token_ops;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteAccountRequest {
    /// 账号 ID
    pub id: Id,
}

/// 删除账号（本人或 privileged 删他人；特权账号禁删）
#[rpc("identity.account.delete")]
#[tracing::instrument(skip(db, cache, token_helper, request))]
pub async fn delete_account(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
    State(cache): State<Cache>,
    State(token_helper): State<TokenHelper>,
    Params(request): Params<DeleteAccountRequest>,
) -> Result<()> {
    let account = load_for_delete(&mut db, caller, request.id).await?;
    let old_avatar = account.avatar.clone();
    if let Some(path) = old_avatar.as_deref() {
        file_attachment::detach_column_for_delete(&mut db, path).await?;
    }
    if let Err(err) = account.delete().exec(&mut db).await {
        // 删行失败：回滚刚扣减的旧头像引用（ref_count / tombstone 已被动过），
        // 否则行仍引用的对象计数失真（in-use 安全网虽能兜底防误删，勿依赖）。
        if let Some(path) = old_avatar.as_deref() {
            file_attachment::reattach_best_effort(&mut db, path).await;
        }
        return Err(err.into());
    }
    // 删号已成功：会话吊销失败只降级告警，仍返回 Ok。
    // 运维含义：Redis 故障时遗留 access 在 jti TTL 内仍可通过鉴权门，
    // 但业务读库会得 -32002；勿因此把 delete 打成错误以免客户端重试。
    if let Err(err) = token_ops::revoke_tokens(&cache, &token_helper, request.id).await {
        tracing::warn!(error = %err, account_id = %request.id, "revoke after delete degraded");
    }
    Ok(())
}
