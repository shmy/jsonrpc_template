//! 上传扩展名 → Content-Type（`mime_guess`），以及 ServeDir 可内联展示的「可信」类型。
//!
//! 上传白名单由运行时 `BLOB_ALLOWED_EXTENSIONS` 配置（见 [`super::allowed_extensions`]）；
//! Content-Type 由扩展名经 `mime_guess` 解析，库不认识则拒签。
//! 不可信扩展（html / svg / js 等）若磁盘上仍有文件，HTTP 回源应加
//! `Content-Disposition: attachment`。

/// 扩展名 → Content-Type（`mime_guess`）；未知扩展返回 `None`。
#[must_use]
pub fn content_type_for_extension(ext: &str) -> Option<String> {
    mime_guess::from_ext(ext)
        .first()
        .map(|mime| mime.essence_str().to_owned())
}

/// 浏览器可安全内联的扩展（固定集合）；其余回源应强制 `attachment`。
/// 与默认上传白名单对齐，但**不**随 `BLOB_ALLOWED_EXTENSIONS` 放开（防 XSS）。
/// 仅 `blob-fs` ServeDir 守卫使用。
#[cfg(feature = "blob-fs")]
const INLINE_SAFE_EXTENSIONS: &[&str] = &["webp", "jpg", "jpeg", "png", "gif", "mp4", "webm"];

/// 浏览器可安全内联的扩展；其余回源应强制 `attachment`。
#[cfg(feature = "blob-fs")]
#[inline]
#[must_use]
pub fn is_inline_safe_extension(ext: &str) -> bool {
    INLINE_SAFE_EXTENSIONS
        .iter()
        .any(|e| ext.eq_ignore_ascii_case(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_and_inline_safe() {
        assert_eq!(
            content_type_for_extension("WEBP").as_deref(),
            Some("image/webp")
        );
        assert_eq!(
            content_type_for_extension("jpg").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            content_type_for_extension("mp4").as_deref(),
            Some("video/mp4")
        );
        assert_eq!(
            content_type_for_extension("svg").as_deref(),
            Some("image/svg+xml")
        );
        assert_eq!(content_type_for_extension("qqrq"), None);
        #[cfg(feature = "blob-fs")]
        {
            assert!(is_inline_safe_extension("png"));
            assert!(is_inline_safe_extension("mp4"));
            assert!(!is_inline_safe_extension("svg"));
            assert!(!is_inline_safe_extension("html"));
        }
    }
}
