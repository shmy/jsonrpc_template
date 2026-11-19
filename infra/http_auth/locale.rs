//! authn 错误的 RPC 码段、FTL 与 domain mapper（壳层自持）。
//!
//! 会话基础设施错误（token 缺失/无效/吊销、缓存不可用）不属于任何业务 feature。
//! 分层原则：`authn_kit`（crates）只定义错误语义、保持协议无关——不感知
//! JSON-RPC 码段，也不感知 locale；码段分配（`-32020..-32029`）与文案
//! 归使用它的 infra 壳层，即本 crate。
//!
//! **为何从 `-32020` 起：** 协议核 `axum_jsonrpc` 已占用 jsonrpsee 对齐码
//! `-32007`（body 过大）与 `-32010`（batch 过长）；authn 不得与之碰撞，
//! 以便客户端可单靠 `code` 分支。
//!
//! `http_auth` 不是 modrun module、没有独立注册时机，故由 `http_server`
//! 在 seal 之前统一调用 [`register_authn_locale`]（先注册后 seal 的顺序
//! 在同一函数内天然成立）。**不要**在 feature `module()` 里再注册 authn FTL/mapper。

use authn_kit::AuthnError;
use axum_jsonrpc::{ErrorObject, register_domain_mapper};
use locale::translate_current;
use rootcause::Report;
use serde_json::{Value, json};

/// JSON-RPC codes for authn errors。
///
/// - identity 等业务：`-32001..-32009`（跳过协议占用的 `-32007`）
/// - 协议核保留：`-32007`、`-32010`（见 `axum_jsonrpc::error_codes`）
/// - 本号段：`-32020..-32029`；业务 feature 不得复用
pub mod codes {
    pub const ACCESS_TOKEN_MISSING: i32 = -32020;
    pub const ACCESS_TOKEN_INVALID: i32 = -32021;
    pub const ACCESS_TOKEN_REVOKED: i32 = -32022;
    pub const CACHE_UNAVAILABLE: i32 = -32023;
}

pub(crate) fn authn_rpc_code(err: &AuthnError) -> i32 {
    match err {
        AuthnError::AccessTokenMissing => codes::ACCESS_TOKEN_MISSING,
        AuthnError::AccessTokenInvalid => codes::ACCESS_TOKEN_INVALID,
        AuthnError::AccessTokenRevoked => codes::ACCESS_TOKEN_REVOKED,
        AuthnError::CacheUnavailable => codes::CACHE_UNAVAILABLE,
    }
}

/// Fluent key：`AuthnError` 的 snake_case `Display`（authn_kit 自身无 l10n 概念）。
fn authn_l10n_key(err: &AuthnError) -> String {
    err.to_string()
}

/// HTTP 401 body：稳定 `error` key + 本地化 `message` + RPC 对齐的 `code`。
pub(crate) fn authn_unauthorized_body(err: &AuthnError) -> Value {
    let key = authn_l10n_key(err);
    json!({
        "code": authn_rpc_code(err),
        "error": key,
        "message": translate_current(&key),
    })
}

/// 注册 authn FTL（en-US / zh-CN）与 domain mapper；`http_server` seal 前调用。
pub fn register_authn_locale() {
    locale::register_ftl("en-US", include_str!("locales/en-US/http_auth.ftl"));
    locale::register_ftl("zh-CN", include_str!("locales/zh-CN/http_auth.ftl"));
    register_domain_mapper(map_authn_error);
}

fn map_authn_error(report: &Report) -> Option<ErrorObject> {
    report
        .iter_downcast_context::<AuthnError>()
        .next()
        .map(|err| ErrorObject::new(authn_rpc_code(err), translate_current(&authn_l10n_key(err))))
}

#[cfg(test)]
pub(crate) fn ensure_locales_for_test() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(register_authn_locale);
}

#[cfg(test)]
mod tests {
    use super::*;
    use locale::try_translate;

    #[test]
    fn authn_error_keys_exist_in_both_locales() {
        ensure_locales_for_test();
        for err in [
            AuthnError::AccessTokenMissing,
            AuthnError::AccessTokenInvalid,
            AuthnError::AccessTokenRevoked,
            AuthnError::CacheUnavailable,
        ] {
            let key = err.to_string();
            assert!(
                try_translate("en-US", &key).is_some(),
                "missing en-US key `{key}`"
            );
            assert!(
                try_translate("zh-CN", &key).is_some(),
                "missing zh-CN key `{key}`"
            );
        }
    }

    #[test]
    fn authn_error_maps_to_rpc_code() {
        ensure_locales_for_test();
        let report = Report::from(AuthnError::AccessTokenRevoked);
        let err = map_authn_error(&report).unwrap();
        assert_eq!(err.code, codes::ACCESS_TOKEN_REVOKED);
        assert_eq!(err.message, "Access token has been revoked");
        let zh = locale::with_locale("zh-CN", || map_authn_error(&report).unwrap());
        assert_eq!(zh.message, "访问令牌已吊销");
    }

    #[test]
    fn unauthorized_body_keeps_stable_error_key() {
        ensure_locales_for_test();
        let body = authn_unauthorized_body(&AuthnError::AccessTokenMissing);
        assert_eq!(body["code"], codes::ACCESS_TOKEN_MISSING);
        assert_eq!(body["error"], "access_token_missing");
        assert_eq!(body["message"], "Access token is missing");
        let zh = locale::with_locale("zh-CN", || {
            authn_unauthorized_body(&AuthnError::AccessTokenMissing)
        });
        assert_eq!(zh["error"], "access_token_missing");
        assert_eq!(zh["message"], "缺少访问令牌");
    }

    #[test]
    fn authn_codes_avoid_protocol_reserved() {
        // axum_jsonrpc::error_codes::{OVERSIZED_REQUEST, TOO_BIG_BATCH_REQUEST}
        const PROTOCOL_RESERVED: [i32; 2] = [-32007, -32010];
        for code in [
            codes::ACCESS_TOKEN_MISSING,
            codes::ACCESS_TOKEN_INVALID,
            codes::ACCESS_TOKEN_REVOKED,
            codes::CACHE_UNAVAILABLE,
        ] {
            assert!(
                (-32029..=-32020).contains(&code),
                "{code} must stay in authn band -32020..-32029"
            );
            assert!(
                !PROTOCOL_RESERVED.contains(&code),
                "{code} collides with protocol-reserved code"
            );
        }
    }
}
