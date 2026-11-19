//! Axum middleware: bind locale from cookie or `Accept-Language`.
//!
//! Cookie `accept_language` is **language preference only** — not a session /
//! auth cookie. Login remains Bearer JWT (+ opaque refresh).

use axum::extract::Request;
use axum::http::header::{ACCEPT_LANGUAGE, COOKIE};
use axum::middleware::Next;
use axum::response::Response;

use crate::{negotiate_locale, scope};
use axum::http::HeaderMap;

const LANGUAGE_COOKIE_NAME: &str = "accept_language";

/// Bind negotiated locale for the rest of the request.
///
/// Preference: cookie `accept_language`（仅语言）→ `Accept-Language` → default.
pub async fn bind_accept_language(req: Request, next: Next) -> Response {
    let cookie_lang = language_from_cookie(req.headers());
    let header = req
        .headers()
        .get(ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let locale = negotiate_locale(cookie_lang.as_deref().or(header));
    scope(locale, next.run(req)).await
}

fn language_from_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        let Some(value) = part.strip_prefix(LANGUAGE_COOKIE_NAME) else {
            continue;
        };
        let Some(value) = value.strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header::ACCEPT_LANGUAGE, header::COOKIE};
    use axum::middleware::from_fn;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::bind_accept_language;
    use crate::current_locale;

    #[tokio::test]
    async fn middleware_binds_zh() {
        let app = Router::new()
            .route("/t", get(|| async { current_locale() }))
            .layer(from_fn(bind_accept_language));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/t")
                    .header(ACCEPT_LANGUAGE, "zh-CN,en;q=0.8")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"zh-CN");
    }

    #[tokio::test]
    async fn cookie_overrides_accept_language() {
        let app = Router::new()
            .route("/t", get(|| async { current_locale() }))
            .layer(from_fn(bind_accept_language));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/t")
                    .header(ACCEPT_LANGUAGE, "en-US")
                    .header(COOKIE, "accept_language=zh-CN")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"zh-CN");
    }

    #[tokio::test]
    async fn without_middleware_defaults_outside_scope() {
        let app = Router::new().route("/t", get(|| async { current_locale() }));

        let response = app
            .oneshot(Request::builder().uri("/t").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"en-US");
    }
}
