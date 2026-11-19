use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use blob_kit::Blob;
use cache_kit::Cache;
use http_auth::AuthedAccount;
use identity_contract::{Account, Id};
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;
use validify::Validate;

use crate::shared::access::load_for_mutate;
use crate::shared::avatar_ops;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct UpdateAccountRequest {
    /// 账号ID
    pub id: Id,
    /// 姓名
    #[validate(length(min = 1, max = 64, code = "name_invalid_length"))]
    pub name: String,
    /// 头像三态：省略=不变；JSON `null`=清空；字符串=替换（staging / 出站 fill_url / 外链）
    #[serde(default)]
    #[validate(length(max = 2048, code = "avatar_invalid_length"))]
    pub avatar: Option<Option<String>>,
    /// 备注
    #[validate(length(max = 512, code = "remark_invalid_length"))]
    pub remark: Option<String>,
}

/// 更新账号（本人或 privileged 改他人）
#[rpc("identity.account.update")]
#[tracing::instrument(skip(db, blob, cache, request))]
pub async fn update_account(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
    State(blob): State<Blob>,
    State(cache): State<Cache>,
    ValidatedParams(request): ValidatedParams<UpdateAccountRequest>,
) -> Result<Account> {
    let mut account = load_for_mutate(&mut db, caller, request.id).await?;
    let old_avatar = account.avatar.clone();
    let avatar = avatar_ops::replace_avatar(
        &mut db,
        &blob,
        &cache,
        caller,
        old_avatar.as_deref(),
        request.avatar,
    )
    .await?;
    if let Err(err) = account
        .update()
        .name(request.name)
        .avatar(avatar.stored)
        .remark(request.remark)
        .exec(&mut db)
        .await
    {
        // 行更新失败：新 attach 的正式对象已无业务行引用，补偿 detach
        // 防 ref_count 悬挂（AttachmentGc 才能回收）。
        // stale_path 延迟到行写入成功后才扣减，此处旧引用未动、无需回滚。
        file_attachment::detach_fresh_best_effort(&mut db, avatar.fresh_path.as_deref()).await;
        return Err(err.into());
    }
    // 行更新成功：此刻才扣减被替换 / 清除的旧对象引用。
    // 失败只 warn：漏减至多造成保守的引用残留（in-use 安全网兜底），不会误删。
    file_attachment::detach_stale(&mut db, avatar.stale_path.as_deref()).await;
    avatar_ops::fill_avatar_for_wire(&blob, &mut account);
    Ok(account)
}
