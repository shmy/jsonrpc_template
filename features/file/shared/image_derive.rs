//! 正式对象按需派生：读原图 → `image_kit` → 尽力落盘缓存。
//!
//! 与 HTTP 无关；闸门 / 结果由 endpoint 映射状态码。
//! 对象是否可派生由 [`file_contract::resolve_derive_request`] 先行决议。

use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use blob_kit::Blob;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::spawn_blocking;

/// 同进程派生并发上限（CPU 型；满则由调用方回 503，不排队）。
const CONCURRENCY: usize = 4;

static GATE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn gate() -> Arc<Semaphore> {
    Arc::clone(GATE.get_or_init(|| Arc::new(Semaphore::new(CONCURRENCY))))
}

/// 派生落盘结果（无 HTTP 语义）。
#[derive(Debug)]
pub(crate) enum Materialize {
    /// 缓存文件已在磁盘（含获锁后二次命中）。
    OnDisk,
    /// 刚算出的 WebP；已尽力 `write`，调用方应直接回字节以免再读盘。
    Fresh(Vec<u8>),
    /// 原图不存在。
    SourceMissing,
    /// 解码 / 缩放失败（坏图等）。
    Unprocessable,
    /// 读 / exists 等基础设施失败。
    Failed,
}

/// 尝试占用一个派生许可；满则 `None`（→ 503）。
#[must_use]
pub(crate) fn try_acquire() -> Option<OwnedSemaphorePermit> {
    gate().try_acquire_owned().ok()
}

/// 在持有许可期间：若缓存未命中则派生并尽力写入 `cache_path`。
pub(crate) async fn materialize(
    blob: &Blob,
    source_path: &str,
    cache_path: &str,
    width: u32,
    _permit: OwnedSemaphorePermit,
) -> Materialize {
    if let Some(outcome) = cache_hit(blob, cache_path).await {
        return outcome;
    }

    let source = match blob.read(source_path).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Materialize::SourceMissing,
        Err(err) => {
            tracing::warn!(error = %err, path = source_path, "derive source read failed");
            return Materialize::Failed;
        }
    };

    let rendered = match spawn_blocking(move || image_kit::render_max_edge(&source, width)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            tracing::debug!(error = %err, path = source_path, width, "image derive failed");
            return Materialize::Unprocessable;
        }
        Err(err) => {
            tracing::warn!(error = %err, path = source_path, "image derive join failed");
            return Materialize::Failed;
        }
    };

    if let Err(err) = blob
        .write(cache_path, Cursor::new(rendered.as_slice()))
        .await
    {
        tracing::warn!(error = %err, path = cache_path, "derive cache write failed; serving memory");
    }
    Materialize::Fresh(rendered)
}

/// `Some(OnDisk|Failed)` 已有定论；`None` 表示需继续派生。
async fn cache_hit(blob: &Blob, cache_path: &str) -> Option<Materialize> {
    match blob.exists(cache_path).await {
        Ok(true) => Some(Materialize::OnDisk),
        Ok(false) => None,
        Err(err) => {
            tracing::warn!(error = %err, path = cache_path, "derive cache exists recheck failed");
            Some(Materialize::Failed)
        }
    }
}
