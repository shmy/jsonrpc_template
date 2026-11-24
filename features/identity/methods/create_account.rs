use account_id::Id;
use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use blob_kit::Blob;
use cache_kit::Cache;
use http_auth::AuthedAccount;
use identity_contract::{Account, IdentityError, Password};
use rootcause::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use toasty::Db;
use validify::Validate;

use crate::shared::avatar_ops;
use crate::shared::hashed_password::HashedPassword;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct CreateAccountRequest {
    /// 姓名
    #[schemars(example = &"Alice")]
    #[validate(length(min = 1, max = 64, code = "name_invalid_length"))]
    pub name: String,
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
    /// 头像（staging path / 本站公网 URL / 外链；出站为绝对 URL；不可挂他人正式 path）
    #[schemars(example = &"staging/2026/01/abc.webp")]
    #[validate(length(max = 2048, code = "avatar_invalid_length"))]
    pub avatar: Option<String>,
    /// 备注
    #[schemars(example = &"remark")]
    #[validate(length(max = 512, code = "remark_invalid_length"))]
    pub remark: Option<String>,
}

/// 创建账号（PrivateRpc；写入恒 `privileged=false`）
#[rpc("identity.account.create")]
#[tracing::instrument(skip(db, blob, cache, request), fields(%caller))]
pub async fn create_account(
    AuthedAccount(caller): AuthedAccount,
    State(mut db): State<Db>,
    State(blob): State<Blob>,
    State(cache): State<Cache>,
    ValidatedParams(request): ValidatedParams<CreateAccountRequest>,
) -> Result<Account> {
    let password = HashedPassword::try_new(request.password.as_str()).await?;
    let avatar =
        avatar_ops::resolve_avatar_for_write(&mut db, &blob, &cache, caller, request.avatar)
            .await?;
    let Some(mut account) = Account::upsert_by_phone(&request.phone)
        .name(request.name)
        .password(password.into_string())
        .privileged(false)
        .avatar(avatar.stored)
        .remark(request.remark)
        .on_create(|a| a.id(Id::default()))
        .or_ignore()
        .exec(&mut db)
        .await?
    else {
        // 手机号冲突：仅补偿本次新 attach 的正式对象
        //（外链 / no-op 分支无 blob 引用，误 detach 会错减 ref_count）。
        file_attachment::detach_fresh_best_effort(&mut db, avatar.fresh_path.as_deref()).await;
        return Err(IdentityError::AccountDuplicated.into());
    };
    avatar_ops::fill_avatar_for_wire(&blob, &mut account);
    Ok(account)
}
