//! 本地文件系统后端（feature `fs`）：opendal `services::Fs`，适合开发/单实例环境。
//!
//! - **写/删**：落盘到 `root` 目录，路径即相对 key。
//! - **URL**：`domain` 为公网前缀，`fill_public_url` / `strip_public_url_prefix` 与 Cos 语义一致。
//! - **presign**：签发指向本服务的 HMAC PUT URL（`presign_put_base` + `presign_secret`），
//!   与 COS 直传同形，便于客户端一套逻辑切换后端。

use std::{
    borrow::Cow,
    fmt::Debug,
    io::{Read, Seek},
    sync::Arc,
    time::Duration,
};

use futures_util::Stream;
use opendal::{Operator, layers::LoggingLayer, services};
use rootcause::Result;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::local_presign;
use crate::shared::{BackendCore, ListedEntry};
use std::fmt;
use tokio::fs::create_dir_all;

pub struct FsConfig<'a> {
    /// 本地存储根目录（不存在则自动创建）。
    pub root: &'a str,
    /// 公网访问前缀（`fill_public_url` 拼接用）。
    pub domain: &'a str,
    /// 本地预签名 PUT 基址（例 `http://localhost:8080/files/presigned`）。
    pub presign_put_base: &'a str,
    /// HMAC 密钥（通常复用 `JWT_SECRET`）。
    pub presign_secret: &'a [u8],
}

#[derive(Clone)]
pub struct Fs {
    core: BackendCore,
    presign_put_base: String,
    presign_secret: Arc<[u8]>,
}

impl Debug for Fs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fs").finish()
    }
}

impl Fs {
    pub async fn try_new<'a>(config: FsConfig<'a>) -> Result<Self> {
        create_dir_all(config.root).await?;
        let op = Operator::new(services::Fs::default().root(config.root))?
            .layer(LoggingLayer::default());
        Ok(Self {
            core: BackendCore {
                op,
                domain: config.domain.to_string(),
            },
            presign_put_base: config.presign_put_base.trim_end_matches('/').to_string(),
            presign_secret: Arc::from(config.presign_secret.to_vec().into_boxed_slice()),
        })
    }

    /// 供 HTTP 处理端校验本地预签名 query（与 [`Self::presign_*`] 成对）。
    #[inline]
    pub fn verify_presign_put(
        &self,
        path: &str,
        exp: u64,
        content_type: &str,
        sig: &str,
    ) -> Result<()> {
        local_presign::verify_put_query(&self.presign_secret, path, exp, content_type, sig)
    }
}

impl Fs {
    #[inline]
    pub fn fill_public_url<'a>(&self, path: &'a str) -> Cow<'a, str> {
        self.core.fill_public_url(path)
    }

    #[inline]
    pub fn fill_public_url_optional(&self, path: Option<String>) -> Option<String> {
        self.core.fill_public_url_optional(path)
    }

    #[inline]
    pub fn strip_public_url_prefix<'a>(&self, path: &'a str) -> Cow<'a, str> {
        self.core.strip_public_url_prefix(path)
    }

    #[inline]
    pub fn strip_public_url_prefix_optional(&self, path: Option<String>) -> Option<String> {
        self.core.strip_public_url_prefix_optional(path)
    }
}

impl Fs {
    #[tracing::instrument(skip(stream))]
    pub async fn write_stream(
        &self,
        path: impl AsRef<str> + Debug,
        stream: impl Stream<Item = Result<ReaderStream<File>>> + Unpin,
    ) -> Result<u64> {
        self.core.write_stream(path, stream).await
    }

    #[tracing::instrument(skip(reader))]
    pub async fn write(
        &self,
        path: impl AsRef<str> + Debug,
        reader: impl Read + Seek,
    ) -> Result<u64> {
        self.core.write(path, reader).await
    }

    #[tracing::instrument]
    pub async fn delete_many(&self, paths: Vec<String>) -> Result<()> {
        self.core.delete_many(paths).await
    }

    #[tracing::instrument]
    pub async fn mv(&self, from: &str, to: &str) -> Result<()> {
        self.core.mv(from, to).await
    }

    #[tracing::instrument]
    pub async fn mv_out_of_staging(&self, path: &str) -> Result<String> {
        self.core.mv_out_of_staging(path).await
    }

    #[tracing::instrument]
    pub async fn list_children(&self, prefix: &str) -> Result<Vec<ListedEntry>> {
        self.core.list_children(prefix).await
    }

    #[tracing::instrument]
    pub async fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.core.read(path).await?.map(|out| out.bytes))
    }

    #[tracing::instrument]
    pub async fn presign_upload_url(
        &self,
        path: impl AsRef<str> + Debug,
        content_type: &str,
        expire: Duration,
    ) -> Result<String> {
        local_presign::mint_put_url(
            &self.presign_put_base,
            &self.presign_secret,
            path.as_ref(),
            content_type,
            expire,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    async fn test_backend() -> (Fs, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let fs = Fs::try_new(FsConfig {
            root: dir.path().to_str().expect("utf-8 temp path"),
            domain: "http://test.local/files",
            presign_put_base: "http://test.local/files/presigned",
            presign_secret: b"test-presign-secret-at-least-32b!!",
        })
        .await
        .expect("create fs backend");
        (fs, dir)
    }

    #[tokio::test]
    async fn test_write_read_delete_roundtrip() {
        let (fs, _dir) = test_backend().await;
        let path = "2024/12/test.txt";
        let n = fs
            .write(path, Cursor::new("it's working"))
            .await
            .expect("write");
        assert_eq!(n, 12);

        let meta = fs.core.op.stat(path).await.expect("stat");
        assert_eq!(meta.content_length(), 12);

        fs.delete_many(vec![path.to_string()])
            .await
            .expect("delete");
        assert!(fs.core.op.stat(path).await.is_err());
    }

    #[tokio::test]
    async fn test_fill_public_url_and_strip_public_url_prefix() {
        let (fs, _dir) = test_backend().await;
        assert_eq!(
            fs.fill_public_url("https://example.com/test.txt"),
            "https://example.com/test.txt"
        );
        assert_eq!(
            fs.fill_public_url("assets/icons/logo.png"),
            "http://test.local/files/assets/icons/logo.png"
        );
        assert_eq!(
            fs.strip_public_url_prefix("http://test.local/files/assets/icons/logo.png"),
            "assets/icons/logo.png"
        );
    }

    #[tokio::test]
    async fn test_presign_mints_local_put_url() {
        let (fs, _dir) = test_backend().await;
        let url = fs
            .presign_upload_url(
                "staging/2026/01/a.webp",
                "image/webp",
                Duration::from_secs(600),
            )
            .await
            .expect("presign");
        assert!(url.starts_with("http://test.local/files/presigned?"));
        assert!(url.contains("path=staging"));
        assert!(url.contains("ct=image%2Fwebp") || url.contains("ct=image/webp"));
    }
}
