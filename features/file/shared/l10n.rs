use axum_jsonrpc::{ErrorObject, register_domain_mapper};
use file_contract::FileError;
use locale::{register_ftl, translate_current};
use rootcause::Report;

/// 在 `module()` 内调用一次：同时 `register_ftl` + `register_domain_mapper`。
pub fn register_file_locale_bundle() {
    register_ftl("en-US", include_str!("../locales/en-US/file.ftl"));
    register_ftl("zh-CN", include_str!("../locales/zh-CN/file.ftl"));
    register_domain_mapper(map_file_error);
}

fn map_file_error(report: &Report) -> Option<ErrorObject> {
    report
        .iter_downcast_context::<FileError>()
        .next()
        .map(|err| ErrorObject::new(err.rpc_code(), translate_current(err.l10n_key())))
}

#[cfg(test)]
pub(crate) fn ensure_locales_for_test() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(register_file_locale_bundle);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use file_contract::codes::ATTACHMENT_INVALID;

    #[test]
    fn file_error_maps_to_rpc_code() {
        ensure_locales_for_test();
        let report = Report::from(FileError::AttachmentInvalid);
        let err = map_file_error(&report).unwrap();
        assert_eq!(err.code, ATTACHMENT_INVALID);
        assert_eq!(
            err.message,
            "Invalid attachment path; use the staging path returned by the upload API."
        );
    }

    #[test]
    fn blob_and_rate_limit_keys_exist() {
        use locale::try_translate;

        ensure_locales_for_test();
        for key in [
            "file_rate_limited",
            "blob_presign_unsupported",
            "blob_not_staging_path",
            "blob_unsafe_object_path",
            "blob_object_not_found",
        ] {
            assert!(
                try_translate("en-US", key).is_some(),
                "missing en-US key `{key}`"
            );
            assert!(
                try_translate("zh-CN", key).is_some(),
                "missing zh-CN key `{key}`"
            );
        }
    }
}
