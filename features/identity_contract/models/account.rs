use jiff::Timestamp;
use rootcause::Result;
use schemars::JsonSchema;
use serde::Serialize;
use toasty::{Executor, Model};

use crate::{Id, IdentityError};

#[derive(Debug, Serialize, Model, JsonSchema)]
pub struct Account {
    /// 账号ID
    #[key]
    pub id: Id,
    /// 姓名
    pub name: String,
    /// 手机号
    #[unique]
    pub phone: String,
    #[serde(skip_serializing)]
    /// 密码
    #[schemars(skip)]
    pub password: String,
    /// 特权账号（seed / 运维写入；API create 恒为 false；不序列化到 wire）
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub privileged: bool,
    /// 头像：库内为相对 object path 或外链；RPC 出站由 identity 拼成绝对 URL
    pub avatar: Option<String>,
    /// 备注
    pub remark: Option<String>,
    /// 版本号
    #[serde(skip_serializing)]
    #[schemars(skip)]
    #[version]
    pub version: u64,
    /// 创建时间
    #[auto]
    pub created_at: Timestamp,
    /// 更新时间
    #[auto]
    pub updated_at: Timestamp,
}

impl Account {
    // No `tracing::instrument` here: contract stays tracing-free; every caller
    // (identity methods) already wraps the call in its own span.
    pub async fn require_by_id(executor: &mut dyn Executor, id: Id) -> Result<Self> {
        match Self::filter_by_id(id).first().exec(executor).await? {
            Some(account) => Ok(account),
            None => Err(IdentityError::AccountNotFound.into()),
        }
    }
}
