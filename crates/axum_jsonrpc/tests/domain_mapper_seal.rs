//! Separate process: sealing must panic on late `register_domain_mapper`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum_jsonrpc::{ErrorObject, error_codes, register_domain_mapper, seal_domain_mappers};
use std::panic::catch_unwind;

#[test]
fn register_after_seal_panics() {
    seal_domain_mappers();
    let result = catch_unwind(|| {
        register_domain_mapper(|_| {
            Some(ErrorObject::new(
                error_codes::INTERNAL_ERROR,
                "should not register",
            ))
        });
    });
    assert!(result.is_err());
}
