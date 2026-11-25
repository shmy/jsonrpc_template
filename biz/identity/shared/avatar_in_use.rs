//! AttachmentGc 安全网：仍被账号 avatar 引用的正式 path。

use std::collections::HashSet;
use std::sync::Arc;

use rootcause::Result;
use toasty::Db;
use toasty::sql::query;
use toasty::stmt::Value;

/// 在 `identity::module()` 注册到 `file_attachment`。
pub fn register() {
    file_attachment::register_attachment_in_use(Arc::new(|db| Box::pin(avatar_paths_in_use(db))));
}

async fn avatar_paths_in_use(db: &mut Db) -> Result<HashSet<String>> {
    let rows = query(
        r#"
        SELECT DISTINCT avatar
        FROM accounts
        WHERE avatar IS NOT NULL AND avatar <> ''
        "#,
    )
    .exec(db)
    .await?;

    let mut out = HashSet::new();
    for row in rows {
        let Value::Record(fields) = row else {
            continue;
        };
        match fields.into_iter().next() {
            Some(Value::String(path)) if !path.is_empty() => {
                out.insert(path);
            }
            _ => {}
        }
    }
    Ok(out)
}
