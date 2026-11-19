//! `ServeDir` 前置守卫：dated 路径白名单 + nosniff；不可信扩展强制 attachment。

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use blob_kit::is_public_readable_object_path;

use crate::shared::extensions::is_inline_safe_extension;

/// nest `/files` 后 URI path → 对象 key；校验可读形态，成功响应补安全头。
pub(crate) async fn blob_fs_static_guard(request: Request, next: Next) -> Response {
    let object_path = request.uri().path().trim_start_matches('/');
    if !is_public_readable_object_path(object_path) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let force_attachment = object_path
        .rsplit_once('.')
        .is_none_or(|(_, ext)| !is_inline_safe_extension(ext));

    let mut response = next.run(request).await;
    if !response.status().is_success() {
        return response;
    }

    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if force_attachment {
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_dated_paths() {
        assert!(is_public_readable_object_path("staging/2026/11/a.webp"));
        assert!(is_public_readable_object_path("2026/11/a.webp"));
        assert!(!is_public_readable_object_path("../etc/passwd"));
        assert!(!is_public_readable_object_path("secrets/key.pem"));
        assert!(!is_public_readable_object_path("staging/%2e%2e/x.webp"));
    }

    #[test]
    fn only_allowlisted_exts_inline() {
        assert!(is_inline_safe_extension("webp"));
        assert!(is_inline_safe_extension("mp4"));
        assert!(!is_inline_safe_extension("svg"));
        assert!(!is_inline_safe_extension("html"));
    }
}
