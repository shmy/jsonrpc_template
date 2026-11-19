//! CORS 白名单解析与外层 layer。

use axum::Router;
use axum::http::{HeaderValue, Method, Uri, header};
use axum::middleware::from_fn;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Outer layers shared by probes + busy routes: locale (inner) → CORS (outer).
///
/// CORS must stay outermost so any future response-rebuilding middleware still
/// gets `access-control-*` headers (matches modrun-template discipline).
pub(crate) fn finish_layers(app: Router, cors_origins: &str) -> Router {
    let app = app.layer(from_fn(locale::bind_accept_language));
    match cors_layer(cors_origins) {
        Some(cors) => app.layer(cors),
        None => app,
    }
}

pub(crate) fn cors_layer(origins: &str) -> Option<CorsLayer> {
    let trimmed = origins.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parsed = Vec::new();
    for origin in trimmed.split(',') {
        let origin = origin.trim();
        if origin.is_empty() {
            continue;
        }
        // `HeaderValue` alone accepts junk (e.g. spaces); a typo'd origin must be
        // rejected loudly instead of silently producing a never-matching whitelist.
        match normalize_origin(origin) {
            Some(normalized) => match HeaderValue::from_str(&normalized) {
                Ok(value) => parsed.push(value),
                Err(err) => {
                    tracing::warn!(origin, error = %err, "skip invalid CORS origin");
                }
            },
            None => {
                tracing::warn!(
                    origin,
                    "skip invalid CORS origin (expected http(s)://host[:port])"
                );
            }
        }
    }
    if parsed.is_empty() {
        return None;
    }
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(parsed))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                header::ACCEPT,
                header::ACCEPT_LANGUAGE,
            ]),
    )
}

/// 校验并归一化一个白名单条目为浏览器实际会发送的 Origin 形态。
///
/// 拒绝（`None`）：非 http(s) scheme、缺 host、带 path / query / userinfo——
/// 浏览器 Origin 从不携带这些，这类条目会静默成为「永不匹配」的死白名单。
/// 归一化：
/// - 显式默认端口去掉（http:80 / https:443；浏览器按 Fetch 规范省略默认端口）
/// - host 转小写（`AllowOrigin::list` 按 Header 值字节精确比对，浏览器只发小写 host）
pub(crate) fn normalize_origin(origin: &str) -> Option<String> {
    let uri = Uri::try_from(origin).ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let default_port: u16 = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    // '@' in authority means userinfo ("user@host") — dead entry.
    if uri.authority()?.as_str().contains('@') || uri.host().is_none() {
        return None;
    }
    // "http://host" parses with path "/"; anything longer is dead.
    if uri.path() != "/" || uri.query().is_some() {
        return None;
    }
    let host = uri.host()?.to_ascii_lowercase();
    Some(match uri.port_u16() {
        Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod cors_tests {
    use super::{cors_layer, normalize_origin};

    #[test]
    fn empty_or_whitespace_yields_no_layer() {
        assert!(cors_layer("").is_none());
        assert!(cors_layer("   ").is_none());
        assert!(cors_layer(" , , ").is_none());
    }

    #[test]
    fn invalid_entries_are_skipped() {
        // No scheme / bare host → invalid Origin → skipped; the valid one is kept.
        let layer = cors_layer("http://a.example, not-an-origin");
        assert!(layer.is_some());
        // All entries invalid → no layer at all (never a silently dead whitelist).
        assert!(cors_layer("bad origin 1, example.com, ://nope").is_none());
    }

    #[test]
    fn valid_list_parses() {
        assert!(cors_layer("http://app.example.com").is_some());
        assert!(cors_layer(" http://a.example , https://b.example ").is_some());
    }

    #[test]
    fn normalize_origin_matches_browser_origin_forms() {
        // Exact passthrough.
        assert_eq!(
            normalize_origin("http://a.example").as_deref(),
            Some("http://a.example")
        );
        // Uppercase host is lowercased for byte-exact Header matching.
        assert_eq!(
            normalize_origin("http://A.Example:8080").as_deref(),
            Some("http://a.example:8080")
        );
        // Explicit default ports are dropped (browsers omit them per Fetch spec).
        assert_eq!(
            normalize_origin("http://a.example:80").as_deref(),
            Some("http://a.example")
        );
        assert_eq!(
            normalize_origin("https://a.example:443").as_deref(),
            Some("https://a.example")
        );
        // Non-default port survives.
        assert_eq!(
            normalize_origin("https://a.example:8443").as_deref(),
            Some("https://a.example:8443")
        );
        // IPv6 authority keeps its brackets (real Origin form).
        assert_eq!(
            normalize_origin("http://[::1]:8080").as_deref(),
            Some("http://[::1]:8080")
        );
        assert_eq!(
            normalize_origin("http://[2001:db8::1]").as_deref(),
            Some("http://[2001:db8::1]")
        );
    }

    #[test]
    fn normalize_origin_rejects_dead_entry_forms() {
        for bad in [
            "not-an-origin",
            "example.com",
            "http://a.example/app",
            "http://a.example?x=1",
            "http://user@a.example",
            "ftp://a.example",
            "http://",
        ] {
            assert!(normalize_origin(bad).is_none(), "must reject `{bad}`");
        }
    }

    #[test]
    fn dead_entries_skip_valid_sibling_survives() {
        // Path-bearing entry skipped; the clean sibling survives.
        let layer = cors_layer("http://a.example/app, https://b.example");
        assert!(layer.is_some());
    }
}
