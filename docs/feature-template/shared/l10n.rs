//! FTL + domain mapper 一体注册（须在 `http_server` seal 之前）。
//!
//! 有领域错误时取消注释并改名；校验文案（validify `code`）也放本 feature 的 locales，
//! 勿往 `infra/locale` 的 `shared.ftl` 塞业务 key。

// use axum_jsonrpc::{ErrorObject, register_domain_mapper};
// use locale::{register_ftl, translate_current};
// use rootcause::Report;
// use FEATURE_NAME_contract::FeatureError;

/// 在 `module()` 内调用一次：同时 `register_ftl` + `register_domain_mapper`。
pub fn register_FEATURE_locale_bundle() {
    // register_ftl("en-US", include_str!("../locales/en-US/example.ftl"));
    // register_ftl("zh-CN", include_str!("../locales/zh-CN/example.ftl"));
    // register_domain_mapper(map_feature_error);
}

// fn map_feature_error(report: &Report) -> Option<ErrorObject> {
//     report
//         .iter_downcast_context::<FeatureError>()
//         .next()
//         .map(|err| ErrorObject::new(err.rpc_code(), translate_current(err.l10n_key())))
// }
