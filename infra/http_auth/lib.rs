#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod extractor;
pub mod locale;
mod middleware;
mod token;

pub use extractor::{AuthedAccount, require_authed_account};
pub use locale::register_authn_locale;
pub use middleware::{account_auth_middleware, optional_account_auth};
