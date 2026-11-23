//! Fluent locale bundles: features `register_ftl` at `module()` time; translate at the edge.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod middleware;

use fluent_bundle::{
    FluentArgs, FluentResource, concurrent::FluentBundle as ConcurrentFluentBundle,
};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use unic_langid::LanguageIdentifier;

pub use middleware::bind_accept_language;

pub const DEFAULT_LOCALE: &str = "en-US";

type Bundle = ConcurrentFluentBundle<FluentResource>;
type FtlEntry = (&'static str, &'static str);

/// Domain FTL fragments contributed via [`register_ftl`] (`Some` until first seal).
static FTL_REGISTRY: Mutex<Option<Vec<FtlEntry>>> = Mutex::new(Some(Vec::new()));
static BUNDLES: OnceLock<HashMap<&'static str, Bundle>> = OnceLock::new();

tokio::task_local! {
    static CURRENT_LOCALE: &'static str;
}

fn registry_lock() -> MutexGuard<'static, Option<Vec<FtlEntry>>> {
    FTL_REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Register a Fluent resource. Call from feature `module()` before [`seal_bundles`].
///
/// # Panics
/// If called after bundles have already been sealed.
pub fn register_ftl(locale: &'static str, ftl: &'static str) {
    let mut guard = registry_lock();
    #[allow(clippy::expect_used)]
    let registry = guard.as_mut().expect(
        "register_ftl after bundles sealed \
         (must happen in Module::module(), before http_server seals)",
    );
    registry.push((locale, ftl));
}

/// Seal Fluent bundles (idempotent). Call from the app shell before serving traffic.
pub fn seal_bundles() {
    let _ = bundles();
}

fn bundles() -> &'static HashMap<&'static str, Bundle> {
    BUNDLES.get_or_init(|| {
        let mut entries = {
            let mut guard = registry_lock();
            guard.take().unwrap_or_default()
        };
        // Shared resources are always contributed at seal time (not via register_ftl).
        entries.push(("en-US", include_str!("locales/en-US/shared.ftl")));
        entries.push(("zh-CN", include_str!("locales/zh-CN/shared.ftl")));

        let mut by_locale: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for (locale, ftl) in entries {
            by_locale.entry(locale).or_default().push(ftl);
        }
        let mut map = HashMap::with_capacity(by_locale.len());
        for (locale, ftls) in by_locale {
            register_locale(&mut map, locale, &ftls);
        }
        map
    })
}

#[allow(
    clippy::expect_used,
    reason = "locale registration is infallible with pre-validated inputs"
)]
fn register_locale(map: &mut HashMap<&'static str, Bundle>, locale: &'static str, ftls: &[&str]) {
    let langid: LanguageIdentifier = locale.parse().expect("invalid locale identifier");
    let mut bundle = ConcurrentFluentBundle::new_concurrent(vec![langid]);
    // RPC / HTTP error messages are plain ASCII/CJK; bidi isolates just pollute clients.
    bundle.set_use_isolating(false);
    for ftl in ftls {
        let resource = FluentResource::try_new((*ftl).to_string()).expect("failed to parse FTL");
        bundle
            .add_resource(resource)
            .expect("failed to add FTL resource");
    }
    map.insert(locale, bundle);
}

/// Locale bound for the current request ([`scope`] / middleware).
#[must_use]
pub fn current_locale() -> &'static str {
    CURRENT_LOCALE
        .try_with(|locale| *locale)
        .unwrap_or(DEFAULT_LOCALE)
}

/// Run `fut` with a bound locale (`en-US` / `zh-CN`).
pub async fn scope<F, R>(locale: &'static str, fut: F) -> R
where
    F: Future<Output = R>,
{
    CURRENT_LOCALE.scope(locale, fut).await
}

/// Sync variant of [`scope`] (unit tests / non-async call sites).
pub fn with_locale<R>(locale: &'static str, f: impl FnOnce() -> R) -> R {
    CURRENT_LOCALE.sync_scope(locale, f)
}

#[must_use]
pub fn translate(locale: &str, key: &str) -> String {
    translate_with_args(locale, key, &[])
}

/// Translate `key` with Fluent variables (`{ $name }` in FTL).
#[must_use]
pub fn translate_with_args(locale: &str, key: &str, args: &[(&str, &str)]) -> String {
    if let Some(text) = try_translate_with_args(locale, key, args) {
        return text;
    }
    if locale != DEFAULT_LOCALE
        && let Some(text) = try_translate_with_args(DEFAULT_LOCALE, key, args)
    {
        return text;
    }
    tracing::warn!(%locale, %key, "missing fluent message; returning key");
    key.to_string()
}

/// Translate using [`current_locale`].
#[must_use]
pub fn translate_current(key: &str) -> String {
    translate(current_locale(), key)
}

/// [`translate_current`] with Fluent args.
#[must_use]
pub fn translate_current_with_args(key: &str, args: &[(&str, &str)]) -> String {
    translate_with_args(current_locale(), key, args)
}

/// Look up `key` in a sealed bundle (no fallback).
#[must_use]
pub fn try_translate(locale: &str, key: &str) -> Option<String> {
    try_translate_with_args(locale, key, &[])
}

/// Look up `key` with Fluent args (no fallback).
#[must_use]
pub fn try_translate_with_args(locale: &str, key: &str, args: &[(&str, &str)]) -> Option<String> {
    let bundle = bundles().get(locale)?;
    let msg = bundle.get_message(key)?;
    let pattern = msg.value()?;
    let mut fluent_args = FluentArgs::new();
    for &(name, value) in args {
        fluent_args.set(name, value);
    }
    let mut errors = Vec::new();
    let value = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
    Some(value.into_owned())
}

/// Map `Accept-Language` to a supported locale id (honours `q=` weights; skips `q=0`).
#[must_use]
pub fn negotiate_locale(header: Option<&str>) -> &'static str {
    let Some(raw) = header else {
        return DEFAULT_LOCALE;
    };

    let mut best: Option<(f32, &'static str)> = None;
    for part in raw.split(',') {
        let mut params = part.split(';');
        let tag = params.next().unwrap_or("").trim();
        if tag.is_empty() {
            continue;
        }

        let mut q = 1.0_f32;
        for param in params {
            let param = param.trim();
            if let Some(value) = param
                .strip_prefix("q=")
                .or_else(|| param.strip_prefix("Q="))
                && let Ok(parsed) = value.trim().parse::<f32>()
            {
                q = parsed;
            }
        }
        // RFC 9110: q=0 means "not acceptable".
        if q <= 0.0 {
            continue;
        }

        let primary = tag.split(['-', '_']).next().unwrap_or("").trim();
        let locale = if primary.eq_ignore_ascii_case("zh") {
            "zh-CN"
        } else if primary.eq_ignore_ascii_case("en") {
            "en-US"
        } else {
            continue;
        };

        match best {
            Some((best_q, _)) if best_q >= q => {}
            _ => best = Some((q, locale)),
        }
    }

    best.map_or(DEFAULT_LOCALE, |(_, locale)| locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_shared_keys() {
        assert_eq!(
            translate("zh-CN", "internal_server_error"),
            "服务器内部错误"
        );
        assert_eq!(
            translate("en-US", "internal_server_error"),
            "Internal server error"
        );
        assert_eq!(
            translate("ja", "internal_server_error"),
            "Internal server error"
        );
        assert_eq!(translate("en-US", "invalid_params"), "Invalid params");
        assert_eq!(translate("zh-CN", "invalid_params"), "无效参数");
        assert_eq!(
            translate_with_args(
                "en-US",
                "invalid_params_missing_field",
                &[("path", "password")]
            ),
            "Missing required field `password`"
        );
        assert_eq!(
            translate_with_args(
                "zh-CN",
                "invalid_params_missing_field",
                &[("path", "password")]
            ),
            "缺少必填字段 `password`"
        );
        assert_eq!(
            translate("en-US", "invalid_params_unknown_field"),
            "Unknown field"
        );
        assert_eq!(
            translate("zh-CN", "invalid_params_unknown_field"),
            "存在未知字段"
        );
        assert_eq!(
            translate_with_args(
                "en-US",
                "invalid_params_unknown_variant",
                &[("path", "mode")]
            ),
            "Unknown variant at `mode`"
        );
        assert_eq!(
            translate_with_args(
                "en-US",
                "invalid_params_duplicate_field",
                &[("path", "password")]
            ),
            "Duplicate field `password`"
        );
        assert_eq!(
            translate("en-US", "invalid_params_shape"),
            "Params must be a JSON array or object"
        );
        assert_eq!(
            translate("zh-CN", "invalid_params_invalid_type_root"),
            "参数类型无效"
        );
        assert_eq!(translate("zh-CN", "parse_error"), "解析错误");
    }

    #[test]
    fn negotiate_accept_language() {
        assert_eq!(negotiate_locale(Some("zh-CN,en;q=0.8")), "zh-CN");
        assert_eq!(negotiate_locale(Some("en-US,en;q=0.9")), "en-US");
        assert_eq!(negotiate_locale(Some("en;q=0.8,zh;q=0.9")), "zh-CN");
        assert_eq!(negotiate_locale(None), "en-US");
        assert_eq!(negotiate_locale(Some("fr,de")), "en-US");
    }

    #[test]
    fn negotiate_skips_q_zero() {
        assert_eq!(negotiate_locale(Some("zh-CN;q=0")), "en-US");
        assert_eq!(negotiate_locale(Some("zh-CN;q=0,en;q=0.5")), "en-US");
        assert_eq!(negotiate_locale(Some("en;q=0,zh;q=0.1")), "zh-CN");
    }
}
