use jiff::civil::Date;
use jiff::{Timestamp, tz::TimeZone};
use tempoid::TempoId;

/// 与 [`blob_kit::STAGING_PREFIX`] 同值（权威在 `blob_kit`；本处为契约侧镜像）。
///
/// 变更时请同步 `blob_kit::STAGING_PREFIX`，并由 `file` 单测断言相等。
pub const STORAGE_DIR_PREFIX: &str = "staging/";

/// URL 派生默认最长边（`?s=` 默认档；缓存文件 `_s{DEFAULT}.webp`）。
pub const DEFAULT_DERIVE_SIZE: u32 = 256;

/// 允许的派生最长边白名单（防任意尺寸打满 CPU / 缓存爆炸）。
///
/// `?s=` 语义为**最长边上界**（size）：只缩小、不放大；原图已更短则尺寸不变（仍可能转 WebP）。
pub const DERIVE_SIZES: &[u32] = &[128, 256, 512, 1024];

/// 与 [`DERIVE_SIZES`] 同步的提示串（HTTP 400 等）；增删档位时改一处即可。
pub const DERIVE_SIZES_HINT: &str = "128|256|512|1024";

/// 旧版预生成后缀（job 时代 `*_sm.webp`）；GC 仍会清。
pub const LEGACY_THUMB_SUFFIX: &str = "_sm";
/// 旧 query `?w=` 时代的缓存后缀前缀 `_w{n}`；GC / 识别仍支持。
pub const LEGACY_DERIVE_MARK: &str = "_w";
/// 当前缓存后缀前缀：`_s{n}.webp`。
pub const DERIVE_MARK: &str = "_s";
pub const DERIVE_EXTENSION: &str = "webp";

/// 可自动派生的图片扩展名（小写、无点）；视频等不在此列。
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

/// 是否为可派生缩略图的图片扩展名。
#[must_use]
pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// `s` 是否在 [`DERIVE_SIZES`] 白名单。
#[must_use]
pub fn is_allowed_derive_size(size: u32) -> bool {
    DERIVE_SIZES.contains(&size)
}

/// `?s=` 解析结果：区分「非法档」与「此对象不可派生」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeriveRequest {
    /// 可派生，值为缓存 object key。
    CachePath(String),
    /// `s` 不在白名单 → HTTP 应 400。
    BadSize,
    /// staging / 非图 / 已是派生文件等 → 忽略 `s`，回原图。
    NotApplicable,
}

/// 决议 `object_path` + `s`：非法档 / 不适用 / 缓存 key。
#[must_use]
pub fn resolve_derive_request(object_path: &str, size: u32) -> DeriveRequest {
    if !is_allowed_derive_size(size) {
        return DeriveRequest::BadSize;
    }
    match derived_object_path(object_path, size) {
        Some(path) => DeriveRequest::CachePath(path),
        None => DeriveRequest::NotApplicable,
    }
}

/// 由对象 path + 最长边派生缓存 object key：`…/stem_s{size}.webp`。
///
/// - `staging/` / 非图片 / 已是派生缓存 / 非法 size / 外链 → `None`
/// - `2026/01/abc.jpg` + `256` → `Some("2026/01/abc_s256.webp")`
#[must_use]
pub fn derived_object_path(object_path: &str, size: u32) -> Option<String> {
    if size == 0 || !is_allowed_derive_size(size) {
        return None;
    }
    let (parent, stem, _) = formal_image_parts(object_path)?;
    Some(join_object(
        parent,
        &format!("{stem}{DERIVE_MARK}{size}.{DERIVE_EXTENSION}"),
    ))
}

/// 默认档缓存 path（[`DEFAULT_DERIVE_SIZE`]）。
#[must_use]
pub fn thumb_object_path(object_path: &str) -> Option<String> {
    derived_object_path(object_path, DEFAULT_DERIVE_SIZE)
}

/// GC：原图对应的全部派生缓存 key（当前 `_s*` + 旧 `_w*` + 旧 `_sm.webp`）。
///
/// `staging/` 下不会生成派生，故返回空。
#[must_use]
pub fn derived_cache_paths(object_path: &str) -> Vec<String> {
    let Some((parent, stem, _)) = formal_image_parts(object_path) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(DERIVE_SIZES.len() * 2 + 1);
    for &s in DERIVE_SIZES {
        out.push(join_object(
            parent,
            &format!("{stem}{DERIVE_MARK}{s}.{DERIVE_EXTENSION}"),
        ));
        // 旧 ?w= 时代落盘
        out.push(join_object(
            parent,
            &format!("{stem}{LEGACY_DERIVE_MARK}{s}.{DERIVE_EXTENSION}"),
        ));
    }
    out.push(join_object(
        parent,
        &format!("{stem}{LEGACY_THUMB_SUFFIX}.{DERIVE_EXTENSION}"),
    ));
    out
}

/// 可派生的正式原图：非 staging、非外链、非已派生 stem。
fn formal_image_parts(object_path: &str) -> Option<(Option<&str>, &str, &str)> {
    let path = object_path.trim();
    if path.is_empty()
        || path.starts_with(STORAGE_DIR_PREFIX)
        || path.starts_with("http://")
        || path.starts_with("https://")
    {
        return None;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = file_name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() || stem.contains('.') {
        return None;
    }
    if !is_image_extension(ext) || is_derived_stem(stem, ext) {
        return None;
    }
    let parent = path.rsplit_once('/').map(|(p, _)| p);
    Some((parent, stem, ext))
}

fn is_derived_stem(stem: &str, ext: &str) -> bool {
    if !ext.eq_ignore_ascii_case(DERIVE_EXTENSION) {
        return false;
    }
    if stem.ends_with(LEGACY_THUMB_SUFFIX) {
        return true;
    }
    ends_with_size_mark(stem, DERIVE_MARK) || ends_with_size_mark(stem, LEGACY_DERIVE_MARK)
}

fn ends_with_size_mark(stem: &str, mark: &str) -> bool {
    stem.rsplit_once(mark)
        .is_some_and(|(_, rest)| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn join_object(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(p) if !p.is_empty() => format!("{p}/{name}"),
        _ => name.to_string(),
    }
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
    fn derived_object_path_for_images() {
        assert_eq!(
            derived_object_path("2026/01/abc.jpg", 256).as_deref(),
            Some("2026/01/abc_s256.webp")
        );
        assert_eq!(thumb_object_path("staging/2026/01/abc.PNG"), None);
        assert_eq!(derived_object_path("staging/2026/01/abc.jpg", 256), None);
        assert_eq!(derived_object_path("2026/01/abc_s256.webp", 128), None);
        assert_eq!(derived_object_path("2026/01/abc_w256.webp", 128), None);
        assert_eq!(derived_object_path("2026/01/abc_sm.webp", 256), None);
        assert_eq!(derived_object_path("2026/01/clip.mp4", 256), None);
        assert_eq!(derived_object_path("2026/01/abc.jpg", 999), None);
        assert_eq!(derived_object_path("https://cdn.example/a.jpg", 256), None);
    }

    #[test]
    fn resolve_derive_request_distinguishes_bad_size() {
        assert_eq!(
            resolve_derive_request("2026/01/abc.jpg", 256),
            DeriveRequest::CachePath("2026/01/abc_s256.webp".into())
        );
        assert_eq!(
            resolve_derive_request("2026/01/abc.jpg", 1025),
            DeriveRequest::BadSize
        );
        assert_eq!(
            resolve_derive_request("staging/2026/01/abc.jpg", 256),
            DeriveRequest::NotApplicable
        );
        assert_eq!(
            resolve_derive_request("2026/01/clip.mp4", 256),
            DeriveRequest::NotApplicable
        );
        let expected = DERIVE_SIZES
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("|");
        assert_eq!(DERIVE_SIZES_HINT, expected);
    }

    #[test]
    fn derived_cache_paths_include_s_legacy_w_and_sm() {
        let keys = derived_cache_paths("2026/01/abc.jpg");
        assert!(keys.iter().any(|k| k == "2026/01/abc_s128.webp"));
        assert!(keys.iter().any(|k| k == "2026/01/abc_s256.webp"));
        assert!(keys.iter().any(|k| k == "2026/01/abc_s1024.webp"));
        assert!(keys.iter().any(|k| k == "2026/01/abc_w256.webp"));
        assert!(keys.iter().any(|k| k == "2026/01/abc_sm.webp"));
        assert!(derived_cache_paths("staging/2026/01/abc.jpg").is_empty());
        assert_eq!(
            derived_object_path("2026/01/abc.jpg", 1024).as_deref(),
            Some("2026/01/abc_s1024.webp")
        );
    }
}
