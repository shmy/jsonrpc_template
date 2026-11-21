//! Identity RPC 方法表。
//!
//! - **PublicRpc**（匿名）：`login_account` / `refresh_account`
//! - **PrivateRpc**（需登录）：其余全部

pub(crate) mod account_profile;
pub(crate) mod change_password;
pub(crate) mod create_account;
pub(crate) mod delete_account;
pub(crate) mod login_account;
pub(crate) mod logout_account;
pub(crate) mod read_account;
pub(crate) mod refresh_account;
pub(crate) mod search_account;
pub(crate) mod update_account;
