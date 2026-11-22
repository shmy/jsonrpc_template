use jiff::civil::Date;
use jiff::{Timestamp, tz::TimeZone};
use tempoid::TempoId;

/// 与 [`blob_kit::STAGING_PREFIX`] 同值（权威在 `blob_kit`；本处为契约侧镜像）。
///
/// 变更时请同步 `blob_kit::STAGING_PREFIX`，并由 `file` 单测断言相等。
pub const STORAGE_DIR_PREFIX: &str = "staging/";

/// 缩略图文件名后缀（固定 WebP）：`YYYY/MM/<stem>_sm.webp`。
pub const THUMB_SUFFIX: &str = "_sm";
pub const THUMB_EXTENSION: &str = "webp";

/// 可自动生成缩略图的图片扩展名（小写、无点）；视频等不在此列。
const IMAGE_EXTENSIONS: &[&str] = &["webp", "jpg", "jpeg", "png", "gif"];

#[inline(always)]
fn random_file_name() -> String {
    TempoId::generate().to_string()
}

/// `staging/YYYY/MM/<tempoId>.<ext>`（UTC 年月）。
pub fn staging_dated_file_path(extension: &str) -> String {
    let zdt = Timestamp::now().to_zoned(TimeZone::UTC);
    format!(
        "{}{:04}/{:02}/{}.{}",
        STORAGE_DIR_PREFIX,
        zdt.year(),
        zdt.month(),
        random_file_name(),
        extension
    )
}

/// 是否为可生成缩略图的图片扩展名。
#[must_use]
pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// 由正式（或 staging）对象 path 派生缩略图 object key。
///
/// - 非图片 / 已是 `*_sm.webp` / 无法解析扩展名 → `None`
/// - `2026/01/abc.jpg` → `Some("2026/01/abc_sm.webp")`
/// - 外链 URL 原样不派生（调用方应先 strip domain）
#[must_use]
pub fn thumb_object_path(object_path: &str) -> Option<String> {
    let path = object_path.trim();
    if path.is_empty() || path.starts_with("http://") || path.starts_with("https://") {
        return None;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = file_name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() || stem.contains('.') {
        return None;
    }
    if stem.ends_with(THUMB_SUFFIX) && ext.eq_ignore_ascii_case(THUMB_EXTENSION) {
        return None;
    }
    if !is_image_extension(ext) {
        return None;
    }
    let parent = path.rsplit_once('/').map(|(p, _)| p);
    let thumb_name = format!("{stem}{THUMB_SUFFIX}.{THUMB_EXTENSION}");
    Some(match parent {
        Some(p) if !p.is_empty() => format!("{p}/{thumb_name}"),
        _ => thumb_name,
    })
}

/// 解析 staging 路径中的月份（`staging/YYYY/MM/...`）；非该结构返回 `None`。
/// 返回该月 1 日，供保留期比较。
pub fn staging_dir_date(path: &str) -> Option<Date> {
    let rest = path
        .trim_end_matches('/')
        .strip_prefix(STORAGE_DIR_PREFIX)?;
    let mut parts = rest.split('/');
    let year: i16 = parts.next()?.trim().parse().ok()?;
    let month: i8 = parts.next()?.trim().parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Date::new(year, month, 1).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn staging_path_has_prefix_and_extension() {
        let path = staging_dated_file_path("webp");
        assert!(path.starts_with("staging/"));
        assert!(path.ends_with(".webp"));
        let rest = path.strip_prefix("staging/").unwrap();
        let parts: Vec<_> = rest.split('/').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn staging_dir_date_parses_month() {
        assert_eq!(
            staging_dir_date("staging/2026/01/a.webp"),
            Some(Date::new(2026, 1, 1).unwrap())
        );
        assert_eq!(staging_dir_date("staging/2026/"), None);
        assert_eq!(staging_dir_date("2026/01/a.webp"), None);
    }

    #[test]
    fn thumb_object_path_for_images() {
        assert_eq!(
            thumb_object_path("2026/01/abc.jpg").as_deref(),
            Some("2026/01/abc_sm.webp")
        );
        assert_eq!(
            thumb_object_path("staging/2026/01/abc.PNG").as_deref(),
            Some("staging/2026/01/abc_sm.webp")
        );
        assert_eq!(thumb_object_path("2026/01/abc_sm.webp"), None);
        assert_eq!(thumb_object_path("2026/01/clip.mp4"), None);
        assert_eq!(thumb_object_path("https://cdn.example/a.jpg"), None);
    }
}
