//! 统一对象存储后端：`Blob` 枚举 + 方法门面（模式与 `cache_kit::Cache` 一致）。
//!
//! 编译期按 feature 装配（可并存；`infra/blob`：`cos` 优先，仅 fs 独占时落盘）：
//! - `Cos`：feature `cos`（**默认**），腾讯云 COS / S3 兼容对象存储
//! - `Fs`：feature `fs`，本地文件系统（开发/单实例环境）
//! - `TestBlob`：feature `test-utils`，opendal Memory 内存后端（集成测试）
//!
//! 无 trait / 无 `dyn`：`Blob` 内部 match 派发，方法签名稳定。
//! 后端差异仅在构造（`try_new` 装配 Operator）；方法面（写 / 删 / URL 映射 / presign）三端对齐：
//! - **Cos**：对象存储原生 `presign_write`
//! - **Fs**：HMAC 签名的本服务 `PUT` URL（与 Cos 同形，便于切换）
//! - **TestBlob**：返回 `BlobError::PresignUnsupported`

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

use std::fmt;
#[cfg(feature = "cos")]
mod cos;
mod error;
#[cfg(feature = "fs")]
mod fs;
#[cfg(feature = "fs")]
pub mod local_presign;
#[cfg(feature = "test-utils")]
mod memory;
mod shared;

pub use shared::{
    ListedEntry, STAGING_PREFIX, is_public_readable_object_path, is_safe_object_path,
    is_staging_object_path, resolve_staging_move_paths, strip_staging_prefix,
};

pub use error::BlobError;

#[cfg(feature = "test-utils")]
use crate::memory::TestBlob;
use std::{
    borrow::Cow,
    fmt::Debug,
    io::{Read, Seek},
    time::Duration,
};

use futures_util::Stream;
use rootcause::Result;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

#[cfg(feature = "cos")]
pub use cos::{Cos, CosConfig};
#[cfg(feature = "fs")]
pub use fs::{Fs, FsConfig};

#[cfg(not(any(feature = "cos", feature = "fs", feature = "test-utils")))]
compile_error!("blob_kit requires feature \"cos\", \"fs\", or \"test-utils\"");

/// 对象存储后端句柄：克隆共享、方法即 API。
#[derive(Clone)]
pub enum Blob {
    #[cfg(feature = "cos")]
    Cos(Cos),
    #[cfg(feature = "fs")]
    Fs(Fs),
    #[cfg(feature = "test-utils")]
    Test(TestBlob),
}

impl Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blob").finish()
    }
}

impl Blob {
    /// 各后端构造器独立命名：同名 `try_new` 在 feature 并集下会因方法重名冲突（Rust 无重载），
    /// 拆名后 cos 与 fs 可并存（与 kv 的 `try_new_pg/redb/redis` 同构）。
    #[cfg(feature = "cos")]
    pub async fn try_new_cos<'a>(config: CosConfig<'a>) -> Result<Self> {
        Ok(Self::Cos(Cos::try_new(config).await?))
    }

    #[cfg(feature = "fs")]
    pub async fn try_new_fs<'a>(config: FsConfig<'a>) -> Result<Self> {
        Ok(Self::Fs(Fs::try_new(config).await?))
    }

    /// 测试用内存后端（opendal Memory）：不落盘、不依赖外部服务。
    #[cfg(feature = "test-utils")]
    pub async fn new_for_test() -> Result<Self> {
        Ok(Self::Test(TestBlob::new_for_test().await?))
    }

    /// 拼接公网访问 URL：已是 http(s) 绝对地址则原样返回，否则 `{domain}/{path}`。
    #[inline]
    pub fn fill_public_url<'a>(&self, path: &'a str) -> Cow<'a, str> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.fill_public_url(path),
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.fill_public_url(path),
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.fill_public_url(path),
        }
    }

    #[inline]
    pub fn fill_public_url_optional(&self, path: Option<String>) -> Option<String> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.fill_public_url_optional(path),
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.fill_public_url_optional(path),
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.fill_public_url_optional(path),
        }
    }

    /// 从公网 URL 还原存储路径：剥离 `{domain}/` 前缀；非本站 URL 原样返回。
    #[inline]
    pub fn strip_public_url_prefix<'a>(&self, path: &'a str) -> Cow<'a, str> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.strip_public_url_prefix(path),
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.strip_public_url_prefix(path),
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.strip_public_url_prefix(path),
        }
    }

    #[inline]
    pub fn strip_public_url_prefix_optional(&self, path: Option<String>) -> Option<String> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.strip_public_url_prefix_optional(path),
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.strip_public_url_prefix_optional(path),
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.strip_public_url_prefix_optional(path),
        }
    }

    /// 去掉 `staging/` 前缀；非 staging 原样返回（与 fill / strip URL 同组）。
    #[inline]
    pub fn strip_staging_prefix(path: &str) -> &str {
        strip_staging_prefix(path)
    }

    /// 流式写入，返回写入字节数。fs / test 后端与 cos 行为一致。
    #[tracing::instrument(skip(stream))]
    pub async fn write_stream(
        &self,
        path: impl AsRef<str> + Debug,
        stream: impl Stream<Item = Result<ReaderStream<File>>> + Unpin,
    ) -> Result<u64> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.write_stream(path, stream).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.write_stream(path, stream).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.write_stream(path, stream).await,
        }
    }

    /// 整块写入（`Read + Seek`，如 `Cursor` / `NamedTempFile`），返回写入字节数。
    ///
    /// 会把整个对象读进内存（模板 body 上限内的小对象路径）；
    /// 大文件请用 [`Self::write_stream`]。
    #[tracing::instrument(skip(reader))]
    pub async fn write(
        &self,
        path: impl AsRef<str> + Debug,
        reader: impl Read + Seek,
    ) -> Result<u64> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.write(path, reader).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.write(path, reader).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.write(path, reader).await,
        }
    }

    /// 批量删除（空列表安全）。
    #[tracing::instrument]
    pub async fn delete_many(&self, paths: Vec<String>) -> Result<()> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.delete_many(paths).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.delete_many(paths).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.delete_many(paths).await,
        }
    }

    /// 移动对象（`from` → `to`）。
    #[tracing::instrument]
    pub async fn mv(&self, from: &str, to: &str) -> Result<()> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.mv(from, to).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.mv(from, to).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.mv(from, to).await,
        }
    }

    /// 将 `staging/...`（或对应公网 URL）挪到非 staging 路径，返回目标 path。
    #[tracing::instrument]
    pub async fn mv_out_of_staging(&self, path: &str) -> Result<String> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.mv_out_of_staging(path).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.mv_out_of_staging(path).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.mv_out_of_staging(path).await,
        }
    }

    /// 列出一层子条目（目录 + 文件）；路径不存在返回空列表。供目录遍历 / 过期清理。
    #[tracing::instrument]
    pub async fn list_children(&self, prefix: &str) -> Result<Vec<ListedEntry>> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.list_children(prefix).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.list_children(prefix).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.list_children(prefix).await,
        }
    }

    /// 读取整个对象；不存在返回 `Ok(None)`。
    #[tracing::instrument]
    pub async fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.read(path).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.read(path).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.read(path).await,
        }
    }

    /// 对象是否存在（轻量；不拉内容）。不安全 path → [`BlobError::UnsafeObjectPath`]。
    #[tracing::instrument]
    pub async fn exists(&self, path: &str) -> Result<bool> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.exists(path).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.exists(path).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.exists(path).await,
        }
    }

    /// 预签名直传 URL（Cos 原生 / Fs 本地 HMAC PUT；test 后端 unsupported）。
    #[tracing::instrument]
    pub async fn presign_upload_url(
        &self,
        path: impl AsRef<str> + Debug,
        content_type: &str,
        expire: Duration,
    ) -> Result<String> {
        match self {
            #[cfg(feature = "cos")]
            Self::Cos(b) => b.presign_upload_url(path, content_type, expire).await,
            #[cfg(feature = "fs")]
            Self::Fs(b) => b.presign_upload_url(path, content_type, expire).await,
            #[cfg(feature = "test-utils")]
            Self::Test(b) => b.presign_upload_url(path, content_type, expire).await,
        }
    }

    /// 仅 Fs 后端有值；供本地预签名 PUT 校验（勿在门面上再挂 verify_*）。
    #[cfg(feature = "fs")]
    #[must_use]
    pub fn as_fs(&self) -> Option<&Fs> {
        match self {
            Self::Fs(b) => Some(b),
            #[cfg(feature = "cos")]
            Self::Cos(_) => None,
            #[cfg(feature = "test-utils")]
            Self::Test(_) => None,
        }
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use std::io::Cursor;

    use tokio_util::io::ReaderStream;

    use super::*;
    use futures_util::stream::once;
    use std::io::SeekFrom;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::fs::File as TokioFile;

    fn assert_unsafe_path(err: &rootcause::Report) {
        assert!(
            err.downcast_current_context::<BlobError>()
                .is_some_and(|e| matches!(e, BlobError::UnsafeObjectPath))
        );
    }

    #[tokio::test]
    async fn write_rejects_unsafe_path() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        let err = blob
            .write("../escape.txt", Cursor::new(b"x"))
            .await
            .expect_err("reject");
        assert_unsafe_path(&err);
    }

    #[tokio::test]
    async fn read_delete_mv_reject_unsafe_path() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        assert_unsafe_path(&blob.read("../escape.txt").await.expect_err("read"));
        assert_unsafe_path(
            &blob
                .delete_many(vec!["../escape.txt".into()])
                .await
                .expect_err("delete"),
        );
        assert_unsafe_path(
            &blob
                .mv("staging/ok.txt", "../escape.txt")
                .await
                .expect_err("mv to"),
        );
        assert_unsafe_path(
            &blob
                .mv("../escape.txt", "staging/ok.txt")
                .await
                .expect_err("mv from"),
        );
    }

    #[tokio::test]
    async fn delete_many_allows_trailing_slash_rejects_dotdot() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        blob.delete_many(vec![]).await.expect("empty ok");
        // list 风格目录 path（尾 `/`）校验应通过。
        blob.delete_many(vec!["staging/2025/".into()])
            .await
            .expect("trailing slash dir");
        assert_unsafe_path(
            &blob
                .delete_many(vec!["../escape/".into()])
                .await
                .expect_err("dotdot"),
        );
    }

    #[tokio::test]
    async fn strip_public_url_scheme_alt_and_query() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        assert_eq!(
            blob.strip_public_url_prefix("https://test.local/files/staging/a.webp?w=256"),
            "staging/a.webp"
        );
        assert_eq!(
            blob.strip_public_url_prefix("staging/a.webp?w=128"),
            "staging/a.webp"
        );
        // 真外链保留 query（签名 CDN）；勿在 keep_current 时剥掉。
        assert_eq!(
            blob.strip_public_url_prefix("https://cdn.example/a.webp?sig=abc&w=64"),
            "https://cdn.example/a.webp?sig=abc&w=64"
        );
    }

    #[tokio::test]
    async fn exists_roundtrip() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        let path = "staging/exists.txt";
        assert!(!blob.exists(path).await.unwrap());
        blob.write(path, Cursor::new(b"ok")).await.unwrap();
        assert!(blob.exists(path).await.unwrap());
    }

    #[tokio::test]
    async fn facade_roundtrip() {
        let blob = Blob::new_for_test().await.expect("create test blob");

        let path = "staging/test.txt";
        assert_eq!(
            blob.write(path, Cursor::new("it's working")).await.unwrap(),
            12
        );
        assert_eq!(
            blob.fill_public_url(path),
            "http://test.local/files/staging/test.txt"
        );
        assert_eq!(
            blob.strip_public_url_prefix("http://test.local/files/staging/test.txt"),
            "staging/test.txt"
        );
        assert_eq!(
            blob.fill_public_url_optional(Some(path.to_string())),
            Some("http://test.local/files/staging/test.txt".to_string())
        );
        assert_eq!(
            blob.strip_public_url_prefix_optional(Some(
                "http://test.local/files/staging/test.txt".to_string()
            )),
            Some(path.to_string())
        );

        blob.delete_many(vec![path.to_string()]).await.unwrap();
    }

    #[tokio::test]
    async fn mv_out_of_staging_strips_prefix() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        let staging = "staging/2026/01/a.webp";
        blob.write(staging, Cursor::new(b"img"))
            .await
            .expect("write");
        let dest = blob.mv_out_of_staging(staging).await.expect("mv");
        assert_eq!(dest, "2026/01/a.webp");
        assert_eq!(Blob::strip_staging_prefix(staging), "2026/01/a.webp");
        assert!(blob.read(&dest).await.unwrap().is_some());
        assert!(blob.read(staging).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn mv_out_of_staging_rejects_traversal() {
        let blob = Blob::new_for_test().await.expect("create test blob");
        for bad in [
            "staging/../secret.webp",
            "staging/foo/../../x.webp",
            "staging//x.webp",
        ] {
            let err = blob.mv_out_of_staging(bad).await.expect_err("reject");
            assert!(
                err.downcast_current_context::<BlobError>()
                    .is_some_and(|e| matches!(
                        e,
                        BlobError::UnsafeObjectPath | BlobError::NotStagingPath
                    )),
                "unexpected error for {bad}: {err}"
            );
        }
    }

    #[test]
    fn public_readable_allows_staging_and_promoted_dated_paths() {
        assert!(is_public_readable_object_path("staging/2026/01/a.webp"));
        assert!(is_public_readable_object_path("2026/01/a.webp"));
        assert!(!is_public_readable_object_path("secrets/a.txt"));
        assert!(!is_public_readable_object_path("staging/../x.webp"));
        assert!(!is_public_readable_object_path("staging/evil/01/a.webp"));
    }

    #[tokio::test]
    async fn facade_write_stream_roundtrip() {
        use std::io::Seek as _;

        let blob = Blob::new_for_test().await.expect("create test blob");

        let file = NamedTempFile::new().expect("create temp file");
        Write::write_all(&mut file.as_file(), b"streamed data").expect("write temp");
        // 写入后 cursor 停在末尾，回卷到开头再交给 ReaderStream。
        let mut file = file.into_file();
        file.seek(SeekFrom::Start(0)).expect("rewind");
        let file = TokioFile::from_std(file);
        let stream = Box::pin(once(async { Ok(ReaderStream::new(file)) }));

        let path = "staging/stream.txt";
        assert_eq!(
            blob.write_stream(path, stream).await.expect("write stream"),
            13
        );
        assert_eq!(
            blob.fill_public_url(path),
            "http://test.local/files/staging/stream.txt"
        );

        blob.delete_many(vec![path.to_string()]).await.unwrap();
    }

    #[tokio::test]
    async fn list_children_nested_roundtrip() {
        use std::io::Cursor;

        let blob = Blob::new_for_test().await.expect("create test blob");
        // 空目录安全
        assert!(blob.list_children("staging/").await.unwrap().is_empty());

        blob.write("staging/2025/01/a.txt", Cursor::new("a"))
            .await
            .unwrap();
        blob.write("staging/2025/01/b.txt", Cursor::new("b"))
            .await
            .unwrap();
        blob.write("staging/2025/02/c.txt", Cursor::new("c"))
            .await
            .unwrap();

        let year_dirs = blob.list_children("staging/").await.unwrap();
        assert_eq!(year_dirs.len(), 1, "staging 下只有一个年份目录");
        let year_dir = &year_dirs[0];
        assert!(!year_dir.is_file);

        let month_dirs = blob.list_children(&year_dir.path).await.unwrap();
        assert_eq!(month_dirs.len(), 2, "2025 下有两个月份目录");
        assert!(month_dirs.iter().all(|e| !e.is_file));

        let files = blob.list_children(&month_dirs[0].path).await.unwrap();
        assert!(!files.is_empty());
        assert!(files.iter().all(|e| e.is_file));

        // 清理全部测试对象
        let mut all = vec![year_dir.path.clone()];
        for m in &month_dirs {
            all.push(m.path.clone());
            for f in blob.list_children(&m.path).await.unwrap() {
                all.push(f.path);
            }
        }
        blob.delete_many(all).await.unwrap();
        assert!(blob.list_children("staging/").await.unwrap().is_empty());
    }
}
