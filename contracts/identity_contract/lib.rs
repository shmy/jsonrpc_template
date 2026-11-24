//! Identity bounded-context：落库模型 + 出站 DTO。
//!
//! - [`Account`]：toasty 模型（无 `Serialize`）
//! - [`AccountView`]：RPC wire（o2o 自 `Account`；OpenRPC schema 名同 Rust 类型）
//!
//! Layering: `app/*` / future `ops/*` may depend on this crate only — never on
//! `identity`. This crate must not depend on any surface implementation.
//! Avatar 绝对 URL 由 identity 的 `FillAvatar` 填充，不进本 crate。

pub mod account_view;
pub mod error;
pub mod models;
pub mod password;
pub mod phone;
pub mod refresh_token;

pub use account_view::AccountView;
pub use error::{IdentityError, codes};
pub use models::account::Account;
pub use password::Password;
pub use phone::{is_cn_mobile, mask_for_wire, validate_phone_format};
pub use refresh_token::RefreshToken;
