use jiff::Timestamp;
use rootcause::Result;
use toasty::{Executor, Model};

use account_id::Id;

use crate::IdentityError;

/// 账号落库模型（toasty）。出站请用 [`crate::AccountView`]。
#[derive(Debug, Model)]
pub struct Account {
    /// 账号ID
    #[key]
    pub id: Id,
    /// 姓名
    pub name: String,
    /// 手机号（库内明文；wire 经 [`crate::AccountView`] 脱敏）
    #[unique]
    pub phone: String,
    /// 密码哈希
    pub password: String,
    /// 特权账号（seed / 运维写入；API create 恒为 false）
    pub privileged: bool,
    /// 头像：库内为相对 object path 或外链；出站绝对 URL 由 identity 填充
    pub avatar: Option<String>,
    /// 备注
    pub remark: Option<String>,
    /// 版本号
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
