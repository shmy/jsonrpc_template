use jiff::civil::Date;
use jiff::{Timestamp, tz::TimeZone};
use tempoid::TempoId;

/// 与 [`blob_kit::STAGING_PREFIX`] 同值（权威在 `blob_kit`；本处为契约侧镜像）。
///
/// 变更时请同步 `blob_kit::STAGING_PREFIX`，并由 `file` 单测断言相等。
pub const STORAGE_DIR_PREFIX: &str = "staging/";

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
}
