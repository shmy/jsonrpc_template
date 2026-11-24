//! 头像绑定：identity 对通用附件绑定（`file_attachment::bind`）的薄适配。
//!
//! 通用时序（staging commit + attach 编排、fresh / stale 引用计数协议、
//! 行写入失败补偿、删行失败回滚、in-use 安全网）都在 `infra/file_attachment`；
//! 本模块只剩 identity 专属：
//! - [`into_view`] / [`FillAvatar`]：Model → wire（脱敏后填头像绝对 URL）
//! - update 的三态 wire 形态（省略 / 清空 / 替换）
//! - 绑定策略：允许真外链、长度上限 2048
//!
//! 引用计数时序详见 `file_attachment::bind` 的模块文档。

use account_id::Id;
use blob_kit::Blob;
use cache_kit::Cache;
use file_attachment::{AttachmentWrite, BindInput, BindOptions, bind, keep_current};
use identity_contract::{Account, AccountView};
use rootcause::Result;
use toasty::Db;

/// 入站兜底长度（create 侧 validify 已校验 2048；覆盖 update 直传路径）。
const MAX_AVATAR_LEN: usize = 2048;

/// 头像绑定策略：允许真外链原样入库。
const AVATAR_BIND: BindOptions = BindOptions {
    max_len: MAX_AVATAR_LEN,
    allow_external: true,
};

/// 出站填充头像公网 URL（相对 key → 绝对；已是 http(s) 则不动）。
///
/// 挂在 identity 而非 contract：避免 `AccountView` 依赖 `blob_kit`。
pub trait FillAvatar {
    fn fill_avatar(&mut self, blob: &Blob);
}

impl FillAvatar for AccountView {
    #[inline]
    fn fill_avatar(&mut self, blob: &Blob) {
        self.avatar = blob.fill_public_url_optional(self.avatar.take());
    }
}

/// `Account` → [`AccountView`]：o2o 映射（含 phone 脱敏）后填头像 URL。
#[must_use]
pub fn into_view(account: Account, blob: &Blob) -> AccountView {
    let mut view = AccountView::from(account);
    view.fill_avatar(blob);
    view
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use account_id::Id;
    use jiff::Timestamp;

    #[tokio::test]
    async fn into_view_fills_relative_avatar_and_masks_phone() {
        let blob = Blob::new_for_test().await.expect("test blob");
        let account = Account {
            id: Id::new_unchecked(7),
            name: "Bob".into(),
            phone: "13912345678".into(),
            password: "hash".into(),
            privileged: false,
            avatar: Some("2026/01/x.jpg".into()),
            remark: None,
            version: 1,
            created_at: Timestamp::from_second(1).unwrap(),
            updated_at: Timestamp::from_second(2).unwrap(),
        };
        let view = into_view(account, &blob);
        assert_eq!(view.phone, "139****5678");
        let avatar = view.avatar.expect("avatar");
        assert!(
            avatar.starts_with("http") && avatar.ends_with("2026/01/x.jpg"),
            "relative key should become absolute URL, got {avatar}"
        );
    }

    #[tokio::test]
    async fn into_view_keeps_external_avatar_url() {
        let blob = Blob::new_for_test().await.expect("test blob");
        let external = "https://cdn.example/a.png";
        let account = Account {
            id: Id::new_unchecked(8),
            name: "Bob".into(),
            phone: "13912345678".into(),
            password: "hash".into(),
            privileged: false,
            avatar: Some(external.into()),
            remark: None,
            version: 1,
            created_at: Timestamp::from_second(1).unwrap(),
            updated_at: Timestamp::from_second(2).unwrap(),
        };
        let view = into_view(account, &blob);
        assert_eq!(view.avatar.as_deref(), Some(external));
    }
}
