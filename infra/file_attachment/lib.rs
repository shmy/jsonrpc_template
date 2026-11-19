//! 正式对象引用登记：commit（staging→正式）/ attach / detach。
//!
//! 从 `features/file` 下沉，供多业务复用（勿再让 feature 互依实现 crate）。
//! staging 对象仍由 file 的 `staging_gc` 清理；本 crate 只跟踪转正后的 path。
//! 持有正式 blob 引用的业务 feature 须在 `module()` 里
//! [`register_attachment_in_use`]，供 AttachmentGc 安全网合并。

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use blob_kit::{Blob, BlobError, is_staging_object_path, resolve_staging_move_paths};
use cache_kit::Cache;
use file_contract::FileError;
use identity_contract::Id;
use rootcause::Result;
use toasty::sql::query;
use toasty::sql::statement;
use toasty::stmt::Value;
use toasty::{Db, Executor};

mod bind;

pub use bind::{
    AttachmentWrite, BindInput, BindOptions, bind, detach_column_for_delete,
    detach_fresh_best_effort, detach_stale, keep_current, reattach_best_effort,
};

/// 单次 in-use 收集 future（绑定 `&mut Db` 生命周期）。
pub type AttachmentInUseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<HashSet<String>>> + Send + 'a>>;

/// 进程级 in-use 收集器（`module()` 阶段注册）。
pub type AttachmentInUseCollector =
    Arc<dyn for<'a> Fn(&'a mut Db) -> AttachmentInUseFuture<'a> + Send + Sync>;

static IN_USE_COLLECTORS: Mutex<Vec<AttachmentInUseCollector>> = Mutex::new(Vec::new());

fn in_use_lock() -> MutexGuard<'static, Vec<AttachmentInUseCollector>> {
    IN_USE_COLLECTORS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 注册正式对象引用收集器（GC 安全网）。在 feature `module()` 内调用。
pub fn register_attachment_in_use(collector: AttachmentInUseCollector) {
    in_use_lock().push(collector);
}

/// 已注册的 in-use 收集器数量（AttachmentGc：0 时 fail-closed 跳过本轮）。
#[must_use]
pub fn attachment_in_use_collector_count() -> usize {
    in_use_lock().len()
}

/// 合并全部已注册收集器的 in-use path。
pub async fn collect_all_in_use(db: &mut Db) -> Result<HashSet<String>> {
    let collectors = in_use_lock().clone();
    let mut out = HashSet::new();
    for collector in collectors {
        out.extend(collector(db).await?);
    }
    Ok(out)
}

#[cfg(test)]
fn clear_attachment_in_use_collectors_for_test() {
    in_use_lock().clear();
}

/// pending commit（refcount=0 且未 tombstone）超过此时长可被 AttachmentGc 回收。
pub const PENDING_COMMIT_TTL_HOURS: i64 = 24;
/// tombstone 后保留天数，超过后 AttachmentGc 删 blob + 行。
pub const TOMBSTONE_RETENTION_DAYS: i64 = 7;
/// staging 归属 claim TTL（须覆盖「上传→commit」窗口；过期需重新上传）。
pub const STAGING_OWNER_TTL: Duration = Duration::from_secs(PENDING_COMMIT_TTL_HOURS as u64 * 3600);

fn staging_owner_key(staging_path: &str) -> String {
    format!("blob:staging_owner:{staging_path}")
}

/// 上传成功后绑定 staging path → 上传者；commit 时校验并消费。
pub async fn claim_staging(cache: &Cache, staging_path: &str, owner_id: Id) -> Result<()> {
    if !is_staging_object_path(staging_path) {
        return Err(FileError::AttachmentInvalid.into());
    }
    cache
        .set_ex(
            &staging_owner_key(staging_path),
            &owner_id,
            STAGING_OWNER_TTL,
        )
        .await?;
    Ok(())
}

/// 校验并消费 staging 归属；非本人或不存在 → 域错误（不泄露他人归属）。
pub async fn consume_staging_claim(cache: &Cache, staging_path: &str, actor_id: Id) -> Result<()> {
    let key = staging_owner_key(staging_path);
    let Some(owner) = cache.take::<Id>(&key).await? else {
        return Err(FileError::AttachmentNotFound.into());
    };
    if owner != actor_id {
        restore_staging_claim(cache, staging_path, owner).await;
        return Err(FileError::AttachmentInvalid.into());
    }
    Ok(())
}

async fn restore_staging_claim(cache: &Cache, staging_path: &str, owner_id: Id) {
    if let Err(err) = claim_staging(cache, staging_path, owner_id).await {
        tracing::error!(
            error = %err,
            staging_path,
            "restore staging claim failed; uploader must re-upload"
        );
    }
}

/// AttachmentGc：候选中去掉仍被业务列引用的 path（安全网）。
pub fn filter_gc_candidates(candidates: Vec<String>, in_use: &HashSet<String>) -> Vec<String> {
    candidates
        .into_iter()
        .filter(|path| !in_use.contains(path))
        .collect()
}

/// staging（或公网 URL）→ 正式 path，并登记 `ref_count = 0`。
///
/// 顺序：消费上传者 claim → INSERT `blob_objects` → `mv`。
pub async fn commit_staging(
    db: &mut Db,
    blob: &Blob,
    cache: &Cache,
    actor_id: Id,
    submitted: &str,
) -> Result<String> {
    let (from, path) = resolve_staging_move(blob, submitted)?;
    consume_staging_claim(cache, &from, actor_id).await?;
    if let Err(err) = insert_registration(db, &path).await {
        restore_staging_claim(cache, &from, actor_id).await;
        return Err(err);
    }
    if let Err(err) = blob.mv(&from, &path).await {
        if let Err(rollback_err) = delete_registration(db, &path).await {
            tracing::error!(
                error = %rollback_err,
                path,
                "commit_staging: mv failed and registration rollback failed"
            );
        }
        restore_staging_claim(cache, &from, actor_id).await;
        return Err(map_blob_report(err));
    }
    Ok(path)
}

fn resolve_staging_move(blob: &Blob, submitted: &str) -> Result<(String, String)> {
    let stripped = blob.strip_public_url_prefix(submitted.trim());
    resolve_staging_move_paths(stripped.as_ref()).map_err(|e| map_blob_report(e.into()))
}

/// 将 `BlobError` 映射为 `FileError`（未知变体透传原 report）。
#[must_use]
pub fn map_blob_report(err: rootcause::Report) -> rootcause::Report {
    match err.downcast_current_context::<BlobError>() {
        Some(BlobError::NotStagingPath | BlobError::UnsafeObjectPath) => {
            FileError::AttachmentInvalid.into()
        }
        Some(BlobError::ObjectNotFound) => FileError::AttachmentNotFound.into(),
        Some(BlobError::PresignUnsupported) => FileError::PresignUnsupported.into(),
        _ => err,
    }
}

async fn insert_registration(db: &mut dyn Executor, path: &str) -> Result<()> {
    // toasty 生成的 schema 无 DEFAULT；显式写 CURRENT_TIMESTAMP。
    statement(
        "INSERT INTO blob_objects (path, ref_count, created_at) VALUES ($1, 0, CURRENT_TIMESTAMP)",
    )
    .bind(path.to_string())
    .exec(db)
    .await?;
    Ok(())
}

async fn delete_registration(db: &mut dyn Executor, path: &str) -> Result<()> {
    statement("DELETE FROM blob_objects WHERE path = $1")
        .bind(path.to_string())
        .exec(db)
        .await?;
    Ok(())
}

/// 增加引用；清除 tombstone。
pub async fn attach(db: &mut dyn Executor, path: &str) -> Result<()> {
    let affected = statement(
        r#"
        UPDATE blob_objects
        SET ref_count = ref_count + 1,
            tombstoned_at = NULL
        WHERE path = $1
        "#,
    )
    .bind(path.to_string())
    .exec(db)
    .await?;
    if affected == 0 {
        return Err(FileError::AttachmentNotRegistered.into());
    }
    Ok(())
}

/// 减少引用；归 0（或本已是 pending 0）时打 tombstone。
pub async fn detach(db: &mut dyn Executor, path: &str) -> Result<()> {
    let affected = statement(
        r#"
        UPDATE blob_objects
        SET
            ref_count = GREATEST(ref_count - 1, 0),
            tombstoned_at = CASE
                WHEN ref_count <= 1 THEN COALESCE(tombstoned_at, CURRENT_TIMESTAMP)
                ELSE NULL
            END
        WHERE path = $1
        "#,
    )
    .bind(path.to_string())
    .exec(db)
    .await?;
    if affected == 0 {
        return Err(FileError::AttachmentNotRegistered.into());
    }
    Ok(())
}

/// 删实体用：登记缺失只 warn（历史脏数据），其它错误仍失败。
pub async fn detach_allowing_missing(db: &mut dyn Executor, path: &str) -> Result<()> {
    match detach(db, path).await {
        Ok(()) => Ok(()),
        Err(err)
            if err
                .downcast_current_context::<FileError>()
                .is_some_and(|e| matches!(e, FileError::AttachmentNotRegistered)) =>
        {
            tracing::warn!(
                path,
                "detach: blob_objects row missing (dirty avatar path); continue delete"
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

/// 尽力 detach；失败只 warn。
pub async fn detach_best_effort(db: &mut dyn Executor, path: &str) {
    if let Err(err) = detach(db, path).await {
        tracing::warn!(error = %err, path, "detach best-effort failed");
    }
}

/// AttachmentGc 候选：pending 超时或 tombstone 过期。
pub async fn list_gc_candidates(db: &mut dyn Executor) -> Result<Vec<String>> {
    let rows = query(
        r#"
        SELECT path
        FROM blob_objects
        WHERE
            (
                ref_count = 0
                AND tombstoned_at IS NULL
                AND created_at < NOW() - ($1::bigint * INTERVAL '1 hour')
            )
            OR (
                tombstoned_at IS NOT NULL
                AND tombstoned_at < NOW() - ($2::bigint * INTERVAL '1 day')
            )
        "#,
    )
    .bind(PENDING_COMMIT_TTL_HOURS)
    .bind(TOMBSTONE_RETENTION_DAYS)
    .exec(db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let Value::Record(fields) = row else {
            continue;
        };
        let Some(Value::String(path)) = fields.into_iter().next() else {
            continue;
        };
        out.push(path);
    }
    Ok(out)
}

/// 删除 blob 并移除登记行（Gc 用）。
pub async fn purge_object(db: &mut dyn Executor, blob: &Blob, path: &str) -> Result<()> {
    if let Err(err) = blob.delete_many(vec![path.to_string()]).await {
        tracing::warn!(error = %err, path, "attachment gc: blob delete failed");
        return Err(err);
    }
    delete_registration(db, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn filter_gc_candidates_skips_in_use() {
        let candidates = vec!["a.webp".into(), "b.webp".into(), "c.webp".into()];
        let in_use = HashSet::from(["b.webp".to_string()]);
        assert_eq!(
            filter_gc_candidates(candidates, &in_use),
            vec!["a.webp".to_string(), "c.webp".to_string()]
        );
    }

    #[test]
    fn collector_registry_register_and_clear() {
        clear_attachment_in_use_collectors_for_test();
        assert_eq!(attachment_in_use_collector_count(), 0);
        register_attachment_in_use(Arc::new(|_db| {
            Box::pin(async { Ok(HashSet::from(["a.webp".to_string()])) })
        }));
        register_attachment_in_use(Arc::new(|_db| {
            Box::pin(async { Ok(HashSet::from(["b.webp".to_string()])) })
        }));
        assert_eq!(attachment_in_use_collector_count(), 2);
        // fail-closed 门槛：file attachment_gc 在 count==0 时跳过 purge
        clear_attachment_in_use_collectors_for_test();
        assert_eq!(attachment_in_use_collector_count(), 0);
    }
}
