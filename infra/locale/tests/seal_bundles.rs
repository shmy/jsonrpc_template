//! Separate process: sealing must panic on late `register_ftl`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use locale::{register_ftl, seal_bundles};
use std::panic::catch_unwind;

#[test]
fn register_ftl_after_seal_panics() {
    seal_bundles();
    let result = catch_unwind(|| {
        register_ftl("en-US", "orphan_key = orphan");
    });
    assert!(result.is_err());
}
