//! Separate process: `register_locale_bundle` contributes FTL before seal.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use locale::{register_locale_bundle, seal_bundles, try_translate};

#[test]
fn register_locale_bundle_contributes_ftl_and_mapper() {
    register_locale_bundle(&[("en-US", "bundle_probe_key = probe-ok")], |_| None);
    seal_bundles();
    assert_eq!(
        try_translate("en-US", "bundle_probe_key").as_deref(),
        Some("probe-ok")
    );
}
