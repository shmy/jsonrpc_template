//! 进程级上传扩展名白名单（来自 `BLOB_ALLOWED_EXTENSIONS`）。

use std::sync::OnceLock;

static ALLOWED: OnceLock<Vec<String>> = OnceLock::new();

/// 在 `file` module 装配时调用（fail-closed：未 init 则全部拒绝）。
pub fn init_from_config(raw: &[String]) {
    let _ = ALLOWED.set(normalize(raw));
}

#[must_use]
pub fn is_allowed(ext: &str) -> bool {
    ALLOWED
        .get()
        .is_some_and(|list| list.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

fn normalize(raw: &[String]) -> Vec<String> {
    raw.iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_dots_and_case() {
        let got = normalize(&[" .WEBP ".into(), "mp4".into(), "".into(), ".".into()]);
        assert_eq!(got, vec!["webp".to_string(), "mp4".to_string()]);
    }
}
