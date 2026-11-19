//! identity FTL + domain mapper 一体注册（须在 `http_server` seal 之前）。

use axum_jsonrpc::{ErrorObject, register_domain_mapper};
use identity_contract::IdentityError;
use locale::translate_current;
use rootcause::Report;

/// 在 `module()` 内调用一次：同时 `register_ftl` + `register_domain_mapper`。
pub fn register_identity_locale_bundle() {
    locale::register_ftl("en-US", include_str!("../locales/en-US/identity.ftl"));
    locale::register_ftl("zh-CN", include_str!("../locales/zh-CN/identity.ftl"));
    register_domain_mapper(map_identity_error);
}

fn map_identity_error(report: &Report) -> Option<ErrorObject> {
    report
        .iter_downcast_context::<IdentityError>()
        .next()
        .map(|err| ErrorObject::new(err.rpc_code(), translate_current(err.l10n_key())))
}

#[cfg(test)]
pub(crate) fn ensure_locales_for_test() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(register_identity_locale_bundle);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{ensure_locales_for_test, map_identity_error};
    use identity_contract::IdentityError;
    use identity_contract::codes::NOT_FOUND;
    use locale::try_translate;
    use rootcause::Report;

    #[test]
    fn identity_error_keys_exist() {
        ensure_locales_for_test();
        for err in [
            IdentityError::AccountNotFound,
            IdentityError::AccountDuplicated,
            IdentityError::AccountInvalidCredentials,
            IdentityError::RefreshTokenInvalid,
            IdentityError::LoginRateLimited,
            IdentityError::AuthRateLimited,
            IdentityError::AccountPasswordUnchanged,
            IdentityError::AccountForbidden,
            IdentityError::IncorrectOriginalPassword,
        ] {
            let key = err.l10n_key();
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

    #[test]
    fn identity_error_maps_to_rpc_code() {
        ensure_locales_for_test();
        let report = Report::from(IdentityError::AccountNotFound);
        let err = map_identity_error(&report).unwrap();
        assert_eq!(err.code, NOT_FOUND);
        assert_eq!(err.message, "Account not found");
    }

    #[test]
    fn identity_error_maps_when_wrapped_in_context() {
        ensure_locales_for_test();
        let report: Report = rootcause::report!(IdentityError::AccountNotFound)
            .context("lookup failed")
            .into();
        let err = map_identity_error(&report).unwrap();
        assert_eq!(err.code, NOT_FOUND);
        assert_eq!(err.message, "Account not found");
    }

    #[test]
    fn identity_error_maps_zh_cn_in_locale_scope() {
        ensure_locales_for_test();
        let report = Report::from(IdentityError::AccountNotFound);
        let err = locale::with_locale("zh-CN", || map_identity_error(&report).unwrap());
        assert_eq!(err.code, NOT_FOUND);
        assert_eq!(err.message, "账号不存在");
    }
}
