//! 契约侧 staging 前缀与 `blob_kit` 权威常量对齐（防双源漂移）。

#[cfg(test)]
mod tests {
    #[test]
    fn staging_prefix_matches_blob_kit() {
        assert_eq!(
            file_contract::STORAGE_DIR_PREFIX,
            blob_kit::STAGING_PREFIX,
            "file_contract::STORAGE_DIR_PREFIX must stay equal to blob_kit::STAGING_PREFIX"
        );
    }
}
