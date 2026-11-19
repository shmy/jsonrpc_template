//! 头像绑定：identity 对通用附件绑定（`file_attachment::bind`）的薄适配。
//!
//! 通用时序（staging commit + attach 编排、fresh / stale 引用计数协议、
//! 行写入失败补偿、删行失败回滚、in-use 安全网）都在 `infra/file_attachment`；
//! 本模块只剩 identity 专属：
//! - [`fill_avatar_for_wire`]：`Account.avatar` 出站拼绝对 URL
//! - update 的三态 wire 形态（省略 / 清空 / 替换）
//! - 绑定策略：允许真外链、长度上限 2048
//!
//! 引用计数时序详见 `file_attachment::bind` 的模块文档。

use blob_kit::Blob;
use cache_kit::Cache;
use file_attachment::{AttachmentWrite, BindInput, BindOptions, bind, keep_current};
use identity_contract::{Account, Id};
use rootcause::Result;
use toasty::Db;

/// 入站兜底长度（create 侧 validify 已校验 2048；覆盖 update 直传路径）。
const MAX_AVATAR_LEN: usize = 2048;

/// 头像绑定策略：允许真外链原样入库。
const AVATAR_BIND: BindOptions = BindOptions {
    max_len: MAX_AVATAR_LEN,
    allow_external: true,
};

/// 读出/响应：相对 key → 公网绝对 URL；已是 http(s) 则不动。
#[inline]
pub fn fill_avatar_for_wire(blob: &Blob, account: &mut Account) {
    account.avatar = blob.fill_public_url_optional(account.avatar.take());
}

/// create：`None`/空 = 无头像（old = None，无旧引用可扣）。
pub async fn resolve_avatar_for_write(
    db: &mut Db,
    blob: &Blob,
    cache: &Cache,
    actor_id: Id,
    submitted: Option<String>,
) -> Result<AttachmentWrite> {
    let input = match submitted.as_deref() {
        None => BindInput::Clear,
        Some(raw) => BindInput::Set(raw),
    };
    bind(db, blob, cache, actor_id, None, input, AVATAR_BIND).await
}

/// update 三态：`None` 省略；`Some(None)` 清空；`Some(Some(_))` 替换（可回传 fill_url）。
///
/// 「要不要动列」的 wire 形态留在 identity；引用协议全在 `file_attachment::bind`。
pub async fn replace_avatar(
    db: &mut Db,
    blob: &Blob,
    cache: &Cache,
    actor_id: Id,
    old: Option<&str>,
    submitted: Option<Option<String>>,
) -> Result<AttachmentWrite> {
    match submitted {
        None => Ok(keep_current(blob, old)),
        Some(value) => {
            let input = match value.as_deref() {
                None => BindInput::Clear,
                Some(raw) => BindInput::Set(raw),
            };
            bind(db, blob, cache, actor_id, old, input, AVATAR_BIND).await
        }
    }
}
