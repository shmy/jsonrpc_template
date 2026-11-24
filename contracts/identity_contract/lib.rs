//! Identity bounded-context persistence models (toasty).
//!
//! Layering: `app/*` / future `ops/*` may depend on this crate only — never on
//! `identity`. This crate must not depend on any surface implementation.

pub mod error;
pub mod models;
pub mod password;
pub mod phone;
pub mod refresh_token;

pub use error::{IdentityError, codes};
pub use models::account::Account;
pub use password::Password;
pub use phone::{is_cn_mobile, mask_for_wire, validate_phone_format};
pub use refresh_token::RefreshToken;
