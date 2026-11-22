//! 进程级后台任务：apalis + Redis。
//!
//! - [`module`]：`provide` [`JobBus`]，`invoke` 注册入队器并挂 Lifecycle worker
//! - 不限 `MASTER`：Redis `lock_tasks` 去重；worker 名带 UUID
//! - commit 入队：`try_acquire` inflight（满则跳过、不堵 RPC）→ `spawn` push；
//!   push 前崩溃 / inflight 满会丢任务（显示端 fallback；见 [`thumb_enqueue_stats`]）
//! - 成功不 persist；坏图 / Join panic ack；IO 有限重试，耗尽 → [`IMAGE_THUMB_DLQ_KEY`]

use std::error::Error as StdError;
use std::fmt;
use std::future::ready;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ::config::cli::Cli;
use apalis::layers::retry::RetryPolicy;
use apalis::prelude::*;
use apalis_redis::{Config as RedisJobConfig, ConnectionManager, RedisStorage};
use blob_kit::{Blob, is_safe_object_path};
use file_attachment::register_image_thumbnail_enqueuer;
use file_contract::thumb_object_path;
use modrun::{Lifecycle, Module, task};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::spawn_blocking;
use uuid::Uuid;

/// Redis list：缩略图 IO 重试耗尽后的死信（JSON；`LTRIM` 最近 1000）。
pub const IMAGE_THUMB_DLQ_KEY: &str = "job:dlq:image_thumb";

static ENQUEUE_SKIPPED_FULL: AtomicU64 = AtomicU64::new(0);
static ENQUEUE_PUSH_FAIL: AtomicU64 = AtomicU64::new(0);
static THUMB_WRITTEN: AtomicU64 = AtomicU64::new(0);
static THUMB_DLQ: AtomicU64 = AtomicU64::new(0);

/// 进程内缩略图入队 / 结果计数（多实例各自累计；供运维 / 单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbEnqueueStats {
    pub skipped_inflight_full: u64,
    pub push_fail: u64,
    pub written: u64,
    pub dlq: u64,
}

/// 读取进程级计数快照。
#[must_use]
pub fn thumb_enqueue_stats() -> ThumbEnqueueStats {
    ThumbEnqueueStats {
        skipped_inflight_full: ENQUEUE_SKIPPED_FULL.load(Ordering::Relaxed),
        push_fail: ENQUEUE_PUSH_FAIL.load(Ordering::Relaxed),
        written: THUMB_WRITTEN.load(Ordering::Relaxed),
        dlq: THUMB_DLQ.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
fn reset_thumb_enqueue_stats_for_test() {
    ENQUEUE_SKIPPED_FULL.store(0, Ordering::Relaxed);
    ENQUEUE_PUSH_FAIL.store(0, Ordering::Relaxed);
    THUMB_WRITTEN.store(0, Ordering::Relaxed);
    THUMB_DLQ.store(0, Ordering::Relaxed);
}

/// 生成图片缩略图任务（正式 object path）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateImageThumb {
    pub object_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct JobQueueError(String);

impl JobQueueError {
    fn from_display(err: impl fmt::Display) -> Self {
        Self(err.to_string())
    }
}

#[derive(Clone)]
struct ThumbWorkerCtx {
    max_attempts: usize,
    dlq: ConnectionManager,
}

/// 入队 / worker 句柄。入队路径只 clone [`Self::thumb`]，不碰 DLQ 连接。
#[derive(Clone)]
pub struct JobBus {
    thumb: RedisStorage<GenerateImageThumb>,
    dlq: ConnectionManager,
}

impl JobBus {
    /// 入队缩略图任务。
    pub async fn enqueue_image_thumbnail(
        &self,
        object_path: String,
        trace_id: Option<String>,
    ) -> rootcause::Result<()> {
        push_thumb_job(&self.thumb, object_path, trace_id).await
    }
}

async fn push_thumb_job(
    storage: &RedisStorage<GenerateImageThumb>,
    object_path: String,
    trace_id: Option<String>,
) -> rootcause::Result<()> {
    let mut storage = storage.clone();
    storage
        .push(GenerateImageThumb {
            object_path,
            trace_id,
        })
        .await
        .map_err(JobQueueError::from_display)?;
    Ok(())
}

pub fn module() -> Module {
    Module::builder("job_queue")
        .provide_result_async(provide_job_bus)
        .invoke(register_workers)
}

async fn provide_job_bus(cli: Cli) -> rootcause::Result<JobBus> {
    let url = cli.job.redis_url(&cli.cache);
    let conn = apalis_redis::connect(url)
        .await
        .map_err(JobQueueError::from_display)?;
    let dlq = conn.clone();
    let config = RedisJobConfig::default()
        .queue("image_thumb")
        .persist_results(false)
        .lock_tasks(true)
        .heartbeat_interval(cli.job.heartbeat_interval.duration())
        .missed_heartbeats(cli.job.missed_heartbeats);
    Ok(JobBus {
        thumb: RedisStorage::new(conn).with_config(config),
        dlq,
    })
}

fn register_workers(lc: Lifecycle, cli: Cli, bus: JobBus, blob: Blob) -> modrun::Result<()> {
    let gate = Arc::new(Semaphore::new(cli.job.thumb_enqueue_inflight));
    let enqueue_storage = bus.thumb.clone();
    register_image_thumbnail_enqueuer(Arc::new(move |path: String| {
        let storage = enqueue_storage.clone();
        let gate = Arc::clone(&gate);
        let trace_id = current_trace_id();
        Box::pin(ready(schedule_thumb_enqueue(gate, storage, path, trace_id)))
    }));

    let concurrency = cli.job.thumb_concurrency;
    let max_attempts = cli.job.thumb_max_attempts;
    let retries = max_attempts.saturating_sub(1);
    let orphan = orphan_window(
        cli.job.heartbeat_interval.duration(),
        cli.job.missed_heartbeats,
    );
    let storage = bus.thumb;
    let ctx = ThumbWorkerCtx {
        max_attempts,
        dlq: bus.dlq,
    };
    lc.append(task("job_queue.image_thumb", move |stopped| async move {
        let worker_name = format!("image-thumb-{}", Uuid::new_v4());
        tracing::info!(
            %worker_name,
            max_attempts,
            concurrency,
            orphan_ms = orphan.as_millis(),
            "starting image thumb worker"
        );

        let worker = WorkerBuilder::new(worker_name)
            .backend(storage)
            .data(blob)
            .data(ctx)
            .retry(RetryPolicy::retries(retries))
            .concurrency(concurrency)
            .enable_tracing()
            .build(handle_generate_image_thumb);

        if let Err(err) = worker
            .run_until(async move {
                stopped.await;
                tracing::info!("image thumb worker stopping");
                Ok::<(), WorkerError>(())
            })
            .await
        {
            tracing::error!(error = %err, "image thumb worker exited with error");
        }
        Ok(())
    }))?;
    Ok(())
}

/// 同步调度：try_acquire → spawn push；恒 `Ok`（失败只计数 + warn）。
fn schedule_thumb_enqueue(
    gate: Arc<Semaphore>,
    storage: RedisStorage<GenerateImageThumb>,
    path: String,
    trace_id: Option<String>,
) -> rootcause::Result<()> {
    let Some(permit) = try_acquire_enqueue_permit(&gate, &path) else {
        return Ok(());
    };
    tokio::spawn(async move {
        let _permit: OwnedSemaphorePermit = permit;
        if let Err(err) = push_thumb_job(&storage, path.clone(), trace_id).await {
            ENQUEUE_PUSH_FAIL.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                error = %err,
                path = %path,
                "enqueue image thumbnail failed; display may fallback to original"
            );
        }
    });
    Ok(())
}

fn orphan_window(heartbeat: Duration, missed: usize) -> Duration {
    heartbeat.saturating_mul(missed as u32)
}

fn current_trace_id() -> Option<String> {
    #[cfg(feature = "trace")]
    {
        trace_kit::extract_trace_id(&http_1::HeaderMap::new())
    }
    #[cfg(not(feature = "trace"))]
    {
        None
    }
}

fn try_acquire_enqueue_permit(gate: &Arc<Semaphore>, path: &str) -> Option<OwnedSemaphorePermit> {
    match Arc::clone(gate).try_acquire_owned() {
        Ok(permit) => Some(permit),
        Err(_) => {
            let n = ENQUEUE_SKIPPED_FULL.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(
                path,
                skipped_total = n,
                "image thumb enqueue inflight full; skip (display may fallback)"
            );
            None
        }
    }
}

fn dlq_payload(path: &str, error: &str, at: u64) -> String {
    json!({
        "object_path": path,
        "error": error,
        "at": at,
    })
    .to_string()
}

async fn handle_generate_image_thumb(
    job: GenerateImageThumb,
    blob: Data<Blob>,
    ctx: Data<ThumbWorkerCtx>,
    attempt: Attempt,
) -> Result<(), BoxDynError> {
    match generate_image_thumb(&blob, &job.object_path).await {
        Ok(()) => {
            THUMB_WRITTEN.fetch_add(1, Ordering::Relaxed);
            tracing::info!(
                path = %job.object_path,
                trace_id = job.trace_id.as_deref().unwrap_or(""),
                "image thumbnail written"
            );
            Ok(())
        }
        Err(ThumbJobOutcome::Skip) => Ok(()),
        Err(ThumbJobOutcome::Permanent(err)) => {
            tracing::warn!(
                path = %job.object_path,
                error = %err,
                "permanent thumb failure; ack without retry"
            );
            Ok(())
        }
        Err(ThumbJobOutcome::Retryable(err)) => {
            let attempt_n = attempt.current();
            if should_record_dlq(attempt_n, ctx.max_attempts) {
                record_thumb_dlq(&ctx.dlq, &job.object_path, &err.to_string()).await;
            } else {
                tracing::warn!(
                    path = %job.object_path,
                    attempt = attempt_n,
                    max_attempts = ctx.max_attempts,
                    error = %err,
                    "image thumb attempt failed; will retry"
                );
            }
            Err(err)
        }
    }
}

#[must_use]
fn should_record_dlq(attempt: usize, max_attempts: usize) -> bool {
    max_attempts > 0 && attempt >= max_attempts
}

async fn generate_image_thumb(blob: &Blob, path: &str) -> Result<(), ThumbJobOutcome> {
    if !is_safe_object_path(path) {
        tracing::warn!(path, "reject unsafe thumb job path");
        return Err(ThumbJobOutcome::Skip);
    }
    let Some(thumb_path) = thumb_object_path(path) else {
        tracing::debug!(path, "skip non-image or already-thumb path");
        return Err(ThumbJobOutcome::Skip);
    };
    // `thumb_object_path` 只改文件名后缀，父路径安全 ⇒ thumb 安全；再校验防契约漂移。
    if !is_safe_object_path(&thumb_path) {
        tracing::warn!(path, thumb = %thumb_path, "reject unsafe derived thumb path");
        return Err(ThumbJobOutcome::Skip);
    }

    let Some(bytes) = blob
        .read(path)
        .await
        .map_err(|e| ThumbJobOutcome::Retryable(display_err(e)))?
    else {
        tracing::info!(path, "original missing; ack thumb job");
        return Err(ThumbJobOutcome::Skip);
    };

    let thumb_bytes = match spawn_blocking(move || image_kit::render_thumb(&bytes)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => return Err(ThumbJobOutcome::Permanent(typed_err(err))),
        Err(err) => return Err(ThumbJobOutcome::Permanent(typed_err(err))),
    };

    // 写前 exists：压低与 GC 的孤儿窗口（仍有 TOCTOU；purge 连带删约定 thumb）。
    match blob.exists(path).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!(path, "original gone before thumb write; ack");
            return Err(ThumbJobOutcome::Skip);
        }
        Err(err) => return Err(ThumbJobOutcome::Retryable(display_err(err))),
    }

    blob.write(&thumb_path, Cursor::new(thumb_bytes))
        .await
        .map_err(|e| ThumbJobOutcome::Retryable(display_err(e)))?;
    Ok(())
}

enum ThumbJobOutcome {
    Skip,
    Permanent(BoxDynError),
    Retryable(BoxDynError),
}

async fn record_thumb_dlq(dlq: &ConnectionManager, path: &str, error: &str) {
    let mut conn = dlq.clone();
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = dlq_payload(path, error, at);
    if let Err(err) = conn.lpush::<_, _, ()>(IMAGE_THUMB_DLQ_KEY, payload).await {
        tracing::error!(error = %err, path, "failed to LPUSH image thumb DLQ");
        return;
    }
    if let Err(err) = conn.ltrim::<_, ()>(IMAGE_THUMB_DLQ_KEY, 0, 999).await {
        tracing::warn!(error = %err, "failed to LTRIM image thumb DLQ");
    }
    THUMB_DLQ.fetch_add(1, Ordering::Relaxed);
    tracing::error!(
        path,
        dlq = IMAGE_THUMB_DLQ_KEY,
        "image thumb retries exhausted; recorded in DLQ (display may fallback)"
    );
}

#[inline]
fn typed_err(err: impl StdError + Send + Sync + 'static) -> BoxDynError {
    Box::new(err)
}

#[inline]
fn display_err(err: impl fmt::Display) -> BoxDynError {
    Box::new(JobQueueError(err.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::env::var;
    use std::io::Cursor;

    fn tiny_png() -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_fn(32, 16, |x, y| Rgba([x as u8, y as u8, 128, 255]));
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    }

    fn redis_test_url() -> Option<String> {
        for key in ["JOB_REDIS_URL", "CACHE_REDIS_URL", "REDIS_URL"] {
            if let Ok(url) = var(key) {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    #[test]
    fn dlq_on_final_attempt_only() {
        assert!(!should_record_dlq(0, 5));
        assert!(!should_record_dlq(1, 5));
        assert!(!should_record_dlq(4, 5));
        assert!(should_record_dlq(5, 5));
        assert!(should_record_dlq(6, 5));
        assert!(should_record_dlq(1, 1));
        assert!(!should_record_dlq(1, 0));
    }

    #[test]
    fn dlq_payload_shape() {
        let raw = dlq_payload("2026/01/a.jpg", "boom", 1_700_000_000);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            v.get("object_path").and_then(|x| x.as_str()),
            Some("2026/01/a.jpg")
        );
        assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("boom"));
        assert_eq!(v.get("at").and_then(|x| x.as_u64()), Some(1_700_000_000));
    }

    #[test]
    fn orphan_window_uses_full_duration() {
        assert_eq!(
            orphan_window(Duration::from_millis(500), 3),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn schedule_skips_when_inflight_full_and_bumps_counter() {
        reset_thumb_enqueue_stats_for_test();
        let gate = Arc::new(Semaphore::new(1));
        let held = try_acquire_enqueue_permit(&gate, "held.jpg").unwrap();
        assert!(try_acquire_enqueue_permit(&gate, "overflow.jpg").is_none());
        assert_eq!(thumb_enqueue_stats().skipped_inflight_full, 1);
        drop(held);
        assert!(try_acquire_enqueue_permit(&gate, "after.jpg").is_some());
        assert_eq!(thumb_enqueue_stats().skipped_inflight_full, 1);
    }

    #[tokio::test]
    async fn generate_writes_thumb_when_original_present() {
        let blob = Blob::new_for_test().await.unwrap();
        let path = "2026/01/demo.png";
        blob.write(path, Cursor::new(tiny_png())).await.unwrap();
        assert!(generate_image_thumb(&blob, path).await.is_ok());
        let thumb = thumb_object_path(path).unwrap();
        assert!(blob.exists(&thumb).await.unwrap());
        assert!(!blob.read(&thumb).await.unwrap().unwrap().is_empty());
    }

    #[tokio::test]
    async fn generate_skips_when_original_missing() {
        let blob = Blob::new_for_test().await.unwrap();
        assert!(matches!(
            generate_image_thumb(&blob, "2026/01/gone.png").await,
            Err(ThumbJobOutcome::Skip)
        ));
    }

    #[tokio::test]
    async fn generate_acks_permanent_bad_image() {
        let blob = Blob::new_for_test().await.unwrap();
        let path = "2026/01/bad.png";
        blob.write(path, Cursor::new(b"not-an-image".to_vec()))
            .await
            .unwrap();
        assert!(matches!(
            generate_image_thumb(&blob, path).await,
            Err(ThumbJobOutcome::Permanent(_))
        ));
        assert!(
            !blob
                .exists(&thumb_object_path(path).unwrap())
                .await
                .unwrap()
        );
    }

    /// 需可达 Redis：`JOB_REDIS_URL` / `CACHE_REDIS_URL` / `REDIS_URL`。
    /// 无 URL 或连不上则跳过（不 fail CI）。
    #[tokio::test]
    async fn push_and_dlq_against_redis_if_available() {
        let Some(url) = redis_test_url() else {
            return;
        };
        let Ok(conn) = apalis_redis::connect(url.as_str()).await else {
            return;
        };
        reset_thumb_enqueue_stats_for_test();

        let config = RedisJobConfig::default()
            .queue("image_thumb_test")
            .persist_results(false)
            .lock_tasks(true);
        let storage = RedisStorage::new(conn.clone()).with_config(config);
        push_thumb_job(
            &storage,
            "2026/01/review-test.png".into(),
            Some("a".repeat(32)),
        )
        .await
        .expect("push thumb job");

        record_thumb_dlq(&conn, "2026/01/review-test.png", "test_exhausted").await;
        assert_eq!(thumb_enqueue_stats().dlq, 1);

        let mut conn = conn;
        let raw: Option<String> = conn.lindex(IMAGE_THUMB_DLQ_KEY, 0).await.ok().flatten();
        let raw = raw.expect("DLQ head");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            v.get("object_path").and_then(|x| x.as_str()),
            Some("2026/01/review-test.png")
        );
        assert_eq!(
            v.get("error").and_then(|x| x.as_str()),
            Some("test_exhausted")
        );
    }
}
