//! 本地 fs 回源根目录（请求间共享）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `BLOB_FS_ROOT` 句柄：挂在 `Extension`，供 GET /files 使用。
#[derive(Clone)]
pub(crate) struct BlobFsRoot {
    root: Arc<PathBuf>,
}

impl BlobFsRoot {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
        }
    }

    #[must_use]
    pub(crate) fn as_path(&self) -> &Path {
        self.root.as_ref()
    }
}
