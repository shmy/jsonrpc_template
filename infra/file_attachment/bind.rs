//! 列存 path 附件的通用绑定编排（与具体业务无关）。
//!
//! 供任何「列里存 blob object key（或真外链）」的业务复用；identity avatar
//! 是薄适配示范（`api/identity/shared/avatar_ops.rs`）。
//!
//! # 入站分类
//!
//! | 提交值（trim 后） | 行为 |
//! |------|------|
//! | `staging/...` 或本站 fill_url | commit（消费上传者 claim → INSERT → mv）+ attach |
//! | 真外链 `http(s)://`（非本站 domain） | 原样入库，不登记引用（[`BindOptions::allow_external`]） |
//! | 正式 path / 其 fill_url | 仅当等于当前列值 → no-op；否则 [`FileError::AttachmentInvalid`] |
//! | [`BindInput::Clear`] / 空串 / 纯空白 | 清空该列 |
//!
//! **存库**：相对 object key（或真外链）。**出站**：业务层用
//! `Blob::fill_public_url_optional` 拼绝对 URL。
//!
//! # 引用计数时序（fresh / stale 协议）
//!
//! [`bind`] 返回 [`AttachmentWrite`]：
//!
//! - `fresh_path`：本次新 commit + attach 的正式对象。**行写入失败**的补偿
//!   **只** detach 它（[`detach_fresh_best_effort`]）——外链 / no-op 分支没有
//!   新增引用，盲目 detach 会错减仍被引用对象的 `ref_count`。
//! - `stale_path`：被替换 / 清除的旧对象 key。**延迟扣减**：行写入**成功后**才
//!   执行 [`detach_stale`]——行写入失败时旧引用从未动过，无需回滚。
//!
//! 删实体（如删号）：先 [`detach_column_for_delete`] 扣引用再删行；删行失败用
//! [`reattach_best_effort`] 回滚，避免行仍引用的对象计数失真。
//!
//! # 接入新业务清单
//!
//! 1. feature 在 `module()` 调 [`crate::register_attachment_in_use`]（GC 安全网），
//!    且该 module 排在 `file` 之前；
//! 2. 列存相对 key；入站让 [`bind`] 归一（本站 URL 自动剥 domain），出站拼 URL；
//! 3. handler 严格按上面 fresh / stale 时序调用（勿自行编排 commit + detach）。

use account_id::Id;
use blob_kit::{Blob, is_staging_object_path};
use cache_kit::Cache;
use file_contract::FileError;
use rootcause::Result;
use toasty::Db;

use crate::{attach, commit_staging, detach_allowing_missing, detach_best_effort};
use std::result;

/// 真外链：`http(s)://` 开头（归一后仍非本站 domain 的值）。
fn is_external_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// blob object key（相对 path）；外链不是 blob key。
fn is_blob_key(path: &str) -> bool {
    !is_external_url(path)
}

/// 列存 / in-use 归一：剥本站 domain（含 http↔https、去 `?s=`）；真外链或空 → `None`。
pub(crate) fn normalize_column_path(blob: &Blob, path: &str) -> Option<String> {
    let stripped = blob.strip_public_url_prefix(path.trim());
    let key = stripped.as_ref();
    if key.is_empty() || is_external_url(key) {
        None
    } else {
        Some(stripped.into_owned())
    }
}

/// 绑定策略。
#[derive(Clone, Copy, Debug)]
pub struct BindOptions {
    /// 提交值最大长度（按字节；含外链；业务 validify 校验之外的兜底）。
    pub max_len: usize,
    /// 是否接受真外链 `http(s)://`（非本站 domain）原样入库（不登记引用）。
    pub allow_external: bool,
}

/// 提交意图（显式区分清空与省略，勿退化成裸 `Option<&str>`：
/// `None` 的「省略 / 清空」歧义会让误用**静默清空业务列**）。
///
/// 省略态（列保持不变）不在本枚举内——那是 [`keep_current`] 的职责。
#[derive(Debug, Clone, Copy)]
pub enum BindInput<'a> {
    /// 清空该列。
    Clear,
    /// 绑定为提交值（staging / 本站 fill_url / 外链 / 正式 path）；
    /// 空串 / 纯空白视同 [`Self::Clear`]。
    Set(&'a str),
}

/// 列存 path 的绑定结果（fresh / stale 引用计数协议见模块文档）。
#[derive(Debug)]
pub struct AttachmentWrite {
    /// 入库值（object key / 真外链 / `None`）。
    pub stored: Option<String>,
    /// 本次新 commit + attach 的正式对象（行写入失败补偿只 detach 它）。
    pub fresh_path: Option<String>,
    /// 被替换 / 清除的旧对象 key（行写入成功后由 [`detach_stale`] 扣减）。
    pub stale_path: Option<String>,
}

/// 省略态：列保持不变；`stored` 为归一后的旧值，无引用变更。
pub fn keep_current(blob: &Blob, old: Option<&str>) -> AttachmentWrite {
    AttachmentWrite {
        stored: old.map(|o| blob.strip_public_url_prefix(o.trim()).into_owned()),
        fresh_path: None,
        stale_path: None,
    }
}

/// 清空该列：旧对象引用延迟扣减（stale）；外链无引用。
fn cleared(old_key: Option<String>) -> AttachmentWrite {
    AttachmentWrite {
        stored: None,
        fresh_path: None,
        stale_path: old_key.filter(|p| is_blob_key(p)),
    }
}

/// `classify` 的决策结果（不触碰 Db；staging 分支由 [`bind`] 执行副作用）。
#[derive(Debug)]
enum Classified<'a> {
    /// 决策即结果（清空 / 外链 / no-op）。
    Done(AttachmentWrite),
    /// staging：`submitted`（trim 后原值）交由 [`bind`] commit + attach；
    /// `stale` 为旧对象引用（已滤外链，与新 path 的相等性由 [`bind`] 收尾排除）。
    Staging {
        submitted: &'a str,
        stale: Option<String>,
    },
}

/// 纯决策表：归一 + 分类 + stale 计算（不触碰 Db，可全量单测）。
fn classify<'a>(
    blob: &Blob,
    old: Option<&str>,
    input: BindInput<'a>,
    opts: BindOptions,
) -> result::Result<Classified<'a>, FileError> {
    let old_key = old.map(|o| blob.strip_public_url_prefix(o.trim()).into_owned());

    let BindInput::Set(raw) = input else {
        return Ok(Classified::Done(cleared(old_key)));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Classified::Done(cleared(old_key)));
    }
    if trimmed.len() > opts.max_len {
        return Err(FileError::AttachmentInvalid);
    }

    let key = blob.strip_public_url_prefix(trimmed);

    // 真外链：原样入库，不登记引用（外链无 files 行可 detach）。
    if is_external_url(key.as_ref()) {
        if !opts.allow_external {
            return Err(FileError::AttachmentInvalid);
        }
        return Ok(Classified::Done(AttachmentWrite {
            stored: Some(trimmed.to_string()),
            fresh_path: None,
            stale_path: old_key.filter(|p| is_blob_key(p) && *p != trimmed),
        }));
    }

    // 正式 path 且等于当前列值：no-op，引用不变。
    if let Some(prev) = old_key.as_deref()
        && key.as_ref() == prev
        && !is_staging_object_path(key.as_ref())
    {
        return Ok(Classified::Done(AttachmentWrite {
            stored: Some(prev.to_string()),
            fresh_path: None,
            stale_path: None,
        }));
    }

    // 其余非 staging 形态（他人正式 path / 越权 path）：拒绝。
    if !is_staging_object_path(key.as_ref()) {
        return Err(FileError::AttachmentInvalid);
    }

    Ok(Classified::Staging {
        submitted: trimmed,
        stale: old_key.filter(|p| is_blob_key(p)),
    })
}

/// 绑定提交意图到列（create 传 `old = None`；update 传当前列值）。
///
/// `old`：当前列里的旧值（相对 key / 外链 / `None`），本函数内部归一。
/// 入站分类与 fresh / stale 时序见模块文档。
pub async fn bind(
    db: &mut Db,
    blob: &Blob,
    cache: &Cache,
    actor_id: Id,
    old: Option<&str>,
    input: BindInput<'_>,
    opts: BindOptions,
) -> Result<AttachmentWrite> {
    match classify(blob, old, input, opts)? {
        Classified::Done(write) => Ok(write),
        Classified::Staging { submitted, stale } => {
            let path = commit_and_attach(db, blob, cache, actor_id, submitted).await?;
            Ok(AttachmentWrite {
                stored: Some(path.clone()),
                fresh_path: Some(path.clone()),
                // 防御性排除：staging 目标为高熵随机名，old == path 现实不可达；
                // 若真发生（同 path 重传），不得把仍被引用的对象误 detach。
                stale_path: stale.filter(|p| *p != path.as_str()),
            })
        }
    }
}

/// 行写入成功后：扣减被替换 / 清除的旧对象引用（best-effort，失败只 warn）。
///
/// 漏减至多造成保守的引用残留（in-use 安全网兜底），不会误删。
/// `is_blob_key` 为防御性二次过滤（构造侧已滤，勿依赖单一防线）。
pub async fn detach_stale(db: &mut Db, stale: Option<&str>) {
    if let Some(path) = stale.filter(|p| is_blob_key(p)) {
        detach_best_effort(db, path).await;
    }
}

/// 行写入失败补偿：只 detach 本次新 attach 的正式对象（best-effort）。
///
/// `fresh_path` 必为正式 blob path（由 [`bind`] 的 staging 分支产生），无需外链判定。
pub async fn detach_fresh_best_effort(db: &mut Db, fresh: Option<&str>) {
    if let Some(path) = fresh {
        detach_best_effort(db, path).await;
    }
}

/// 删实体前扣减列引用：未登记的脏 path 只 warn 不阻断删除；外链跳过。
///
/// 先 [`normalize_column_path`]：库内脏本站绝对 URL 仍按相对 key detach。
pub async fn detach_column_for_delete(db: &mut Db, blob: &Blob, path: &str) -> Result<()> {
    let Some(key) = normalize_column_path(blob, path) else {
        return Ok(());
    };
    detach_allowing_missing(db, &key).await
}

/// 删行失败回滚：重新登记刚扣减的引用（失败只 warn，防阻断重试）；外链跳过。
pub async fn reattach_best_effort(db: &mut Db, blob: &Blob, path: &str) {
    let Some(key) = normalize_column_path(blob, path) else {
        return;
    };
    if let Err(err) = attach(db, &key).await {
        tracing::warn!(error = %err, path = %key, "attachment reattach degraded");
    }
}

/// commit（staging→正式）后登记引用；attach 失败补偿 detach。
async fn commit_and_attach(
    db: &mut Db,
    blob: &Blob,
    cache: &Cache,
    actor_id: Id,
    submitted: &str,
) -> Result<String> {
    let path = commit_staging(db, blob, cache, actor_id, submitted).await?;
    if let Err(err) = attach(db, &path).await {
        detach_best_effort(db, &path).await;
        return Err(err);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const OPTS: BindOptions = BindOptions {
        max_len: 128,
        allow_external: true,
    };
    const NO_EXTERNAL: BindOptions = BindOptions {
        max_len: 128,
        allow_external: false,
    };

    async fn test_blob() -> Blob {
        Blob::new_for_test().await.expect("test blob")
    }

    fn done(classified: Classified<'_>) -> AttachmentWrite {
        let Classified::Done(write) = classified else {
            panic!("expected Done");
        };
        write
    }

    fn staging(classified: Classified<'_>) -> (String, Option<String>) {
        let Classified::Staging { submitted, stale } = classified else {
            panic!("expected Staging");
        };
        (submitted.to_string(), stale)
    }

    #[test]
    fn external_vs_blob_key() {
        assert!(is_external_url("https://example.com/a.png"));
        assert!(is_external_url("http://example.com/a.png"));
        assert!(!is_external_url("staging/2026/01/a.webp"));
        assert!(!is_external_url("2026/01/a.webp"));
        assert!(is_blob_key("2026/11/a.webp"));
        assert!(!is_blob_key("https://cdn.example/a.webp"));
    }

    #[tokio::test]
    async fn normalize_column_path_strips_own_domain_skips_external() {
        let blob = test_blob().await;
        assert_eq!(
            normalize_column_path(&blob, "http://test.local/files/2026/01/a.webp").as_deref(),
            Some("2026/01/a.webp")
        );
        assert_eq!(
            normalize_column_path(&blob, "  2026/01/b.webp  ").as_deref(),
            Some("2026/01/b.webp")
        );
        assert_eq!(
            normalize_column_path(&blob, "https://cdn.example/a.webp"),
            None
        );
        assert_eq!(normalize_column_path(&blob, "   "), None);
    }

    #[tokio::test]
    async fn normalize_column_path_query_and_scheme_alt() {
        let blob = test_blob().await;
        // 配置 domain 为 http；脏数据 https + ?s= 仍归一。
        assert_eq!(
            normalize_column_path(&blob, "https://test.local/files/2026/01/a.webp?s=256")
                .as_deref(),
            Some("2026/01/a.webp")
        );
        assert_eq!(
            normalize_column_path(&blob, "2026/01/b.webp?s=512").as_deref(),
            Some("2026/01/b.webp")
        );
        // 外链（含 query）仍不进 in-use。
        assert_eq!(
            normalize_column_path(&blob, "https://cdn.example/a.webp?sig=1"),
            None
        );
    }

    #[tokio::test]
    async fn keep_current_normalizes_own_domain_url() {
        let blob = test_blob().await;
        let kept = keep_current(&blob, Some("http://test.local/files/2026/01/a.webp"));
        assert_eq!(kept.stored.as_deref(), Some("2026/01/a.webp"));
        assert!(kept.fresh_path.is_none() && kept.stale_path.is_none());
    }

    #[tokio::test]
    async fn keep_current_keeps_external_and_none_as_is() {
        let blob = test_blob().await;
        let ext = keep_current(&blob, Some("https://cdn.example/a.webp"));
        assert_eq!(ext.stored.as_deref(), Some("https://cdn.example/a.webp"));
        assert!(keep_current(&blob, None).stored.is_none());
    }

    #[tokio::test]
    async fn keep_current_preserves_external_signed_query() {
        let blob = test_blob().await;
        let url = "https://cdn.example/a.webp?sig=secret&exp=1";
        let kept = keep_current(&blob, Some(url));
        assert_eq!(kept.stored.as_deref(), Some(url));
    }

    /// 清空：旧 blob key 记入 stale 延迟扣减；旧外链无引用。
    #[tokio::test]
    async fn clear_marks_old_blob_key_stale_and_skips_external_old() {
        let blob = test_blob().await;
        let w = done(classify(&blob, Some("2026/01/a.webp"), BindInput::Clear, OPTS).unwrap());
        assert_eq!(w.stored, None);
        assert_eq!(w.stale_path.as_deref(), Some("2026/01/a.webp"));

        let w = done(
            classify(
                &blob,
                Some("https://cdn.example/a.webp"),
                BindInput::Clear,
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(w.stored, None);
        assert!(w.stale_path.is_none(), "external old must not be detached");
    }

    /// 空串 / 纯空白的 `Set` 视同 `Clear`。
    #[tokio::test]
    async fn blank_set_is_clear() {
        let blob = test_blob().await;
        let w = done(classify(&blob, Some("2026/01/a.webp"), BindInput::Set("   "), OPTS).unwrap());
        assert_eq!(w.stored, None);
        assert_eq!(w.stale_path.as_deref(), Some("2026/01/a.webp"));
    }

    /// 真外链：原样入库、无 fresh；旧 blob key 记 stale；同值外链无 stale；
    /// `allow_external = false` 拒绝。
    #[tokio::test]
    async fn external_passthrough_and_policy() {
        let blob = test_blob().await;
        let w = done(
            classify(
                &blob,
                Some("2026/01/a.webp"),
                BindInput::Set(" https://cdn.example/a.webp "),
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(w.stored.as_deref(), Some("https://cdn.example/a.webp"));
        assert!(w.fresh_path.is_none());
        assert_eq!(w.stale_path.as_deref(), Some("2026/01/a.webp"));

        // 旧值即同外链 → 无 stale
        let w = done(
            classify(
                &blob,
                Some("https://cdn.example/a.webp"),
                BindInput::Set("https://cdn.example/a.webp"),
                OPTS,
            )
            .unwrap(),
        );
        assert!(w.stale_path.is_none());

        // 旧值为外链、新外链不同 → 无 stale（外链不登记引用）
        let w = done(
            classify(
                &blob,
                Some("https://cdn.example/old.webp"),
                BindInput::Set("https://cdn.example/new.webp"),
                OPTS,
            )
            .unwrap(),
        );
        assert!(w.stale_path.is_none());

        assert!(
            classify(
                &blob,
                None,
                BindInput::Set("https://x.example/a"),
                NO_EXTERNAL
            )
            .is_err()
        );
    }

    /// 正式 path 等于当前列值（含 fill_url 回传归一后相等）→ no-op。
    #[tokio::test]
    async fn formal_path_noop_when_equal_after_normalize() {
        let blob = test_blob().await;
        let w = done(
            classify(
                &blob,
                Some("2026/01/a.webp"),
                BindInput::Set("2026/01/a.webp"),
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(w.stored.as_deref(), Some("2026/01/a.webp"));
        assert!(w.fresh_path.is_none() && w.stale_path.is_none());

        // 出站 URL 原样回传 → 剥 domain 后与当前列值相等 → no-op
        let w = done(
            classify(
                &blob,
                Some("2026/01/a.webp"),
                BindInput::Set("http://test.local/files/2026/01/a.webp"),
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(w.stored.as_deref(), Some("2026/01/a.webp"));
        assert!(w.stale_path.is_none());
    }

    /// 正式 path 不等于当前列值 → 拒绝（防挂他人对象）。
    #[tokio::test]
    async fn formal_path_rejects_when_differs() {
        let blob = test_blob().await;
        assert!(
            classify(
                &blob,
                Some("2026/01/a.webp"),
                BindInput::Set("2026/01/other.webp"),
                OPTS
            )
            .is_err()
        );
        assert!(classify(&blob, None, BindInput::Set("2026/01/a.webp"), OPTS).is_err());
    }

    /// staging 提交（相对 path / 本站 fill_url）→ 交由 bind commit，旧引用记 stale。
    #[tokio::test]
    async fn staging_classified_for_commit() {
        let blob = test_blob().await;
        let (submitted, stale) = staging(
            classify(
                &blob,
                Some("2026/01/a.webp"),
                BindInput::Set(" staging/2026/11/new.webp "),
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(submitted, "staging/2026/11/new.webp");
        assert_eq!(stale.as_deref(), Some("2026/01/a.webp"));

        // fill_url 回传：submitted 为归一前的 trim 原值（commit_staging 内再剥 domain）
        let (submitted, _) = staging(
            classify(
                &blob,
                None,
                BindInput::Set("http://test.local/files/staging/2026/11/new.webp"),
                OPTS,
            )
            .unwrap(),
        );
        assert_eq!(
            submitted,
            "http://test.local/files/staging/2026/11/new.webp"
        );
    }

    /// 超长提交值 → 拒绝（update 无 validify 长度规则时的唯一防线）。
    #[tokio::test]
    async fn oversize_rejected() {
        let blob = test_blob().await;
        let long = "x".repeat(129);
        assert!(classify(&blob, None, BindInput::Set(long.as_str()), OPTS).is_err());
        assert!(classify(&blob, None, BindInput::Set("https://e.example/a"), OPTS).is_ok());
    }
}
