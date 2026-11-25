//! 正式对象登记表 GC：pending / tombstone 过期后删 blob + 行。
//!
//! 安全网：经 [`file_attachment::collect_all_in_use`] 合并各业务注册的引用
//!（如 identity avatar），不硬编码业务表。
//! **fail-closed**：无任何 in-use 收集器时跳过本轮，避免漏挂 feature 时误删。

use blob_kit::Blob;
use rootcause::Result;
use toasty::Db;

/// 跑一轮 attachment GC，返回删除数量。
pub async fn run_once(db: &mut Db, blob: &Blob) -> Result<u64> {
    if file_attachment::attachment_in_use_collector_count() == 0 {
        tracing::warn!(
            "attachment gc skipped: no in-use collectors registered \
             (holders of formal blob refs must call register_attachment_in_use)"
        );
        return Ok(0);
    }
    let candidates = file_attachment::list_gc_candidates(db).await?;
    let in_use = file_attachment::collect_all_in_use(db, blob).await?;
    let to_purge = file_attachment::filter_gc_candidates(candidates, &in_use);
    let mut deleted = 0_u64;
    for path in to_purge {
        match file_attachment::purge_object(db, blob, &path).await {
            Ok(()) => deleted += 1,
            Err(err) => {
                tracing::warn!(error = %err, path, "attachment gc purge failed");
            }
        }
    }
    Ok(deleted)
}
