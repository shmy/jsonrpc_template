//! Identity bounded-context persistence models (toasty).
//!
//! Layering: other `features/*` may depend on this crate only — never on
//! `identity`. This crate must not depend on any feature implementation.

pub mod error;
pub mod id;
pub mod models;
pub mod password;
pub mod phone;
pub mod refresh_token;

pub use error::{IdentityError, codes};
pub use id::Id;
pub use models::account::Account;
pub use password::Password;
pub use phone::{is_cn_mobile, validate_phone_format};
pub use refresh_token::RefreshToken;
