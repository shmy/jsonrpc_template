//! 后端共享实现：所有后端仅差在 Operator 构造，方法面完全一致。
//!
//! 每个后端类型（`Cos` / `Fs` / `TestBlob`）持有 `BackendCore`，方法一行转发，
//! 避免 fill_public_url / write / delete_many 等在多个后端文件里逐字重复。

use std::{
    borrow::Cow,
    fmt::Debug,
    io::{Read, Seek},
};

use futures_util::{SinkExt as _, Stream, StreamExt as _};
use opendal::ErrorKind;
use opendal::{EntryMode, Operator};
use rootcause::Result;
use std::fmt;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::error::BlobError;
use std::result;

/// staging 对象前缀（权威常量）。`file_contract::STORAGE_DIR_PREFIX` 须同值（`file` 单测锁死）。
pub const STAGING_PREFIX: &str = "staging/";

/// 去掉 `staging/` 前缀；非 staging 路径原样返回。与 [`BackendCore::fill_public_url`] 同属 path/URL 工具。
#[inline]
pub fn strip_staging_prefix(path: &str) -> &str {
    path.strip_prefix(STAGING_PREFIX).unwrap_or(path)
}

/// 相对对象 path：非空、非绝对、无空段、无 `.` / `..`。
#[inline]
pub fn is_safe_object_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != ".." && seg != ".")
}

/// 去掉 `?` / `#` 及之后（公网 URL 查询串 / fragment）。
#[inline]
fn trim_query_fragment(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

/// 配置 domain 的 http↔https 对偶（无 scheme 则 `None`）。
fn alt_scheme_domain(domain: &str) -> Option<String> {
    domain
        .strip_prefix("http://")
        .map(|rest| format!("https://{rest}"))
        .or_else(|| {
            domain
                .strip_prefix("https://")
                .map(|rest| format!("http://{rest}"))
        })
}

/// 剥 `{domain}/`；domain 的 http/https 对偶也试一次。未匹配 → `None`。
fn try_strip_domain_prefix<'a>(domain: &str, path: &'a str) -> Option<&'a str> {
    let prefix = format!("{domain}/");
    if let Some(rest) = path.strip_prefix(&prefix) {
        return Some(rest);
    }
    if let Some(alt) = alt_scheme_domain(domain) {
        let alt_prefix = format!("{alt}/");
        if let Some(rest) = path.strip_prefix(&alt_prefix) {
            return Some(rest);
        }
    }
    None
}

fn is_absolute_http_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// staging 对象：以 [`STAGING_PREFIX`] 开头且为安全相对 path。
#[inline]
#[must_use]
pub fn is_staging_object_path(path: &str) -> bool {
    path.starts_with(STAGING_PREFIX) && is_safe_object_path(path)
}

/// 将已去掉公网 URL 前缀的 staging path 解析为 `(from, to)`；`to` 为去掉 `staging/` 后的正式 path。
///
/// 供 `mv_out_of_staging` 与上层「先登记再 mv」编排共用，避免双写 strip/安全校验。
pub fn resolve_staging_move_paths(
    from_after_url_strip: &str,
) -> result::Result<(String, String), BlobError> {
    let from = from_after_url_strip.trim();
    let Some(to) = from
        .strip_prefix(STAGING_PREFIX)
        .filter(|rest| !rest.is_empty())
    else {
        return Err(BlobError::NotStagingPath);
    };
    if !is_safe_object_path(from) || !is_safe_object_path(to) {
        return Err(BlobError::UnsafeObjectPath);
    }
    Ok((from.to_string(), to.to_string()))
}

/// 公网可读对象形态：安全 path，且为 `staging/YYYY/MM/<file>.<ext>` 或已转正
/// `YYYY/MM/<file>.<ext>`。
///
/// `blob-fs` 的 `ServeDir` 守卫用此过滤 HTTP GET（非整棵 root 无约束暴露）。
pub fn is_public_readable_object_path(path: &str) -> bool {
    if !is_safe_object_path(path) {
        return false;
    }
    let rest = match path.strip_prefix(STAGING_PREFIX) {
        Some(rest) => rest,
        None => path,
    };
    is_dated_object_rest(rest)
}

fn is_dated_object_rest(rest: &str) -> bool {
    let mut parts = rest.split('/');
    let (Some(year), Some(month), Some(name), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if year.len() != 4 || year.parse::<u16>().is_err() {
        return false;
    }
    let Ok(m) = month.parse::<u32>() else {
        return false;
    };
    if month.len() != 2 || !(1..=12).contains(&m) {
        return false;
    }
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && !ext.is_empty() && !stem.contains('.')
}

/// 后端共享内核：`opendal Operator` + 公网 `domain`。克隆共享。
#[derive(Clone)]
pub(crate) struct BackendCore {
    pub(crate) op: Operator,
    pub(crate) domain: String,
}

impl Debug for BackendCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackendCore").finish()
    }
}

impl BackendCore {
    /// 拼接公网访问 URL：已是 http(s) 绝对地址则原样返回，否则 `{domain}/{path}`。
    #[inline]
    pub(crate) fn fill_public_url<'a>(&self, path: &'a str) -> Cow<'a, str> {
        if path.starts_with("http://") || path.starts_with("https://") {
            Cow::Borrowed(path)
        } else {
            Cow::Owned(format!("{}/{}", self.domain, path))
        }
    }

    #[inline]
    pub(crate) fn fill_public_url_optional(&self, path: Option<String>) -> Option<String> {
        path.filter(|p| !p.is_empty())
            .map(|p| self.fill_public_url(&p).into_owned())
    }

    /// 从公网 URL 还原存储路径：剥离 `{domain}/` 前缀；非本站 URL 原样返回。
    ///
    /// - `http` / `https` 与配置 domain 可互换匹配（脏数据 scheme 不一致）
    /// - **本站**剥前缀后、或**相对 object key**：去掉 `?` / `#`（`?s=` 脏数据）
    /// - **真外链**保留 query/fragment（签名 CDN 等，勿在 `keep_current` 时改写）
    #[inline]
    pub(crate) fn strip_public_url_prefix<'a>(&self, path: &'a str) -> Cow<'a, str> {
        let trimmed = path.trim();
        let key = if let Some(rest) = try_strip_domain_prefix(&self.domain, trimmed) {
            trim_query_fragment(rest)
        } else if is_absolute_http_url(trimmed) {
            // 真外链：只 trim 空白，保留 ?sig= / ?s=
            trimmed
        } else {
            // 相对 key：去 query（脏 `2026/01/a.webp?s=256`）
            trim_query_fragment(trimmed)
        };
        if key == path {
            Cow::Borrowed(path)
        } else {
            Cow::Owned(key.to_string())
        }
    }

    #[inline]
    pub(crate) fn strip_public_url_prefix_optional(&self, path: Option<String>) -> Option<String> {
        path.as_deref()
            .map(|p| self.strip_public_url_prefix(p).into_owned())
    }
}

impl BackendCore {
    /// 流式写入，返回写入字节数。
    /// `concurrent(8)` 仅 Cos multipart 直传生效；Fs / Memory 无分片语义，选项被忽略（无害）。
    #[tracing::instrument(skip(stream))]
    pub(crate) async fn write_stream(
        &self,
        path: impl AsRef<str> + Debug,
        mut stream: impl Stream<Item = Result<ReaderStream<File>>> + Unpin,
    ) -> Result<u64> {
        let path = path.as_ref();
        if !is_safe_object_path(path) {
            return Err(BlobError::UnsafeObjectPath.into());
        }
        let writer = self.op.writer_with(path).concurrent(8).await?;
        let mut sink = writer.into_bytes_sink();
        let mut total_size: u64 = 0;

        while let Some(rs) = stream.next().await {
            let mut rs = rs?;
            while let Some(chunk) = rs.next().await {
                let chunk = chunk?;
                total_size += chunk.len() as u64;
                sink.send(chunk).await?;
            }
        }

        sink.close().await?;
        Ok(total_size)
    }

    /// 整块写入（`Read + Seek`，如 `Cursor` / `NamedTempFile`），返回写入字节数。
    ///
    /// 注意：会把整个对象缓冲进内存——仅适合模板 body 上限（16 MiB）内的小对象；
    /// 大文件请走 [`Self::write_stream`]，勿在无上限路径上误用。
    #[tracing::instrument(skip(reader))]
    pub(crate) async fn write(
        &self,
        path: impl AsRef<str> + Debug,
        mut reader: impl Read + Seek,
    ) -> Result<u64> {
        let path = path.as_ref();
        if !is_safe_object_path(path) {
            return Err(BlobError::UnsafeObjectPath.into());
        }
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        let total_size = buf.len() as u64;
        let _ = self.op.write(path, buf).await?;
        Ok(total_size)
    }

    /// 对象是否存在（`stat`/`exists`；不拉内容）。
    #[tracing::instrument(skip(self))]
    pub(crate) async fn exists(&self, path: &str) -> Result<bool> {
        if !is_safe_object_path(path) {
            return Err(BlobError::UnsafeObjectPath.into());
        }
        Ok(self.op.exists(path).await?)
    }

    /// 批量删除（空列表安全）。
    ///
    /// list 返回的目录 path 常带尾 `/`；校验前剥掉再套 [`is_safe_object_path`]。
    #[tracing::instrument]
    pub(crate) async fn delete_many(&self, paths: Vec<String>) -> Result<()> {
        for path in &paths {
            let trimmed = path.trim_end_matches('/');
            if trimmed.is_empty() || !is_safe_object_path(trimmed) {
                return Err(BlobError::UnsafeObjectPath.into());
            }
        }
        self.op.delete_iter(paths).await?;
        Ok(())
    }

    /// 移动对象：优先 `rename`；不支持时退化为读+写+删（三端行为对齐）。
    #[tracing::instrument]
    pub(crate) async fn mv(&self, from: &str, to: &str) -> Result<()> {
        if !is_safe_object_path(from) || !is_safe_object_path(to) {
            return Err(BlobError::UnsafeObjectPath.into());
        }
        if from == to {
            return Ok(());
        }
        match self.op.rename(from, to).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::Unsupported => {
                let Some(out) = self.read(from).await? else {
                    return Err(BlobError::ObjectNotFound.into());
                };
                let _ = self.op.write(to, out.bytes).await?;
                // 源删失败不回滚目标：正式 path 已在，交由调用方 / GC 处理残留 staging。
                if let Err(err) = self.op.delete(from).await {
                    tracing::warn!(
                        error = %err,
                        from,
                        to,
                        "mv: copied ok but delete source failed"
                    );
                }
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Err(BlobError::ObjectNotFound.into()),
            Err(e) => Err(e.into()),
        }
    }

    /// 将 `staging/...` 挪到去掉前缀后的正式路径，返回目标 path。
    #[tracing::instrument]
    pub(crate) async fn mv_out_of_staging(&self, path: &str) -> Result<String> {
        let stripped = self.strip_public_url_prefix(path.trim());
        let (from, to) = match resolve_staging_move_paths(stripped.as_ref()) {
            Ok(pair) => pair,
            Err(err) => return Err(err.into()),
        };
        self.mv(&from, &to).await?;
        Ok(to)
    }

    /// 供 test-utils 内存后端返回明确错误（Fs 走本地 HMAC；Cos 走原生 presign）。
    #[cfg(feature = "test-utils")]
    pub(crate) fn presign_unsupported() -> Result<String> {
        use crate::error::BlobError;

        Err(BlobError::PresignUnsupported.into())
    }
}

/// 一次 `list` 拉取的子条目（一层），供目录遍历 / 过期清理使用。
#[derive(Debug, Clone)]
pub struct ListedEntry {
    pub path: String,
    pub is_file: bool,
}

impl BackendCore {
    /// 列出一层子条目（目录 + 文件）。路径不存在按空列表处理（容错）。
    #[tracing::instrument(skip(self))]
    pub(crate) async fn list_children(&self, prefix: &str) -> Result<Vec<ListedEntry>> {
        let entries = match self.op.list(prefix).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(entries
            .into_iter()
            .map(|entry| ListedEntry {
                path: entry.path().to_string(),
                is_file: entry.metadata().mode() == EntryMode::FILE,
            })
            .collect())
    }

    /// 读取整个对象；不存在返回 `Ok(None)`（供静态文件服务区分 404 与故障）。
    #[tracing::instrument(skip(self))]
    pub(crate) async fn read(&self, path: &str) -> Result<Option<ReadOutput>> {
        if !is_safe_object_path(path) {
            return Err(BlobError::UnsafeObjectPath.into());
        }
        match self.op.read(path).await {
            Ok(buffer) => Ok(Some(ReadOutput {
                bytes: buffer.to_vec(),
            })),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// 一次整块读取的结果。
pub(crate) struct ReadOutput {
    pub bytes: Vec<u8>,
}

impl Debug for ReadOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadOutput")
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[cfg(test)]
mod staging_path_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn is_staging_object_path_requires_prefix_and_safety() {
        assert!(is_staging_object_path("staging/2026/01/a.webp"));
        assert!(!is_staging_object_path("2026/01/a.webp"));
        assert!(!is_staging_object_path("staging/../x.webp"));
        assert!(!is_staging_object_path(""));
    }

    #[test]
    fn resolve_staging_move_paths_strips_prefix() {
        let (from, to) =
            resolve_staging_move_paths("staging/2026/01/a.webp").expect("staging path");
        assert_eq!(from, "staging/2026/01/a.webp");
        assert_eq!(to, "2026/01/a.webp");
    }

    #[test]
    fn resolve_staging_move_paths_rejects_non_staging() {
        assert!(matches!(
            resolve_staging_move_paths("2026/01/a.webp"),
            Err(BlobError::NotStagingPath)
        ));
        assert!(matches!(
            resolve_staging_move_paths("staging/../x.webp"),
            Err(BlobError::UnsafeObjectPath)
        ));
    }
}
