//! Method 内 Redis 限流（`file.presign`）：按 subject 滑动窗。
//! 配额复用进程级 `AuthRateLimit`（`RATE_LIMIT_AUTH_*`），非硬编码。

use std::fmt::Display;

use app_state::AuthRateLimit;
use authn_kit::AuthnError;
use cache_kit::Cache;
use file_contract::FileError;
use rootcause::{Report, Result};

fn presign_subject_key(subject: &impl Display) -> String {
    format!("file:presign:subject:{subject}")
}

fn map_cache_err(err: Report) -> Report {
    tracing::error!(error = %err, "cache unavailable");
    // 与 identity 限流一致：会话/配额基础设施 → authn `-32023`。
    Report::from(AuthnError::CacheUnavailable)
}

/// `file.presign`：按调用者滑动窗；cache 故障 → [`AuthnError::CacheUnavailable`]（fail-closed）。
pub(crate) async fn enforce_presign_subject(
    cache: &Cache,
    auth_rate: AuthRateLimit,
    subject: impl Display,
) -> Result<()> {
    let key = presign_subject_key(&subject);
    let allowed = auth_rate.allow(cache, &key).await.map_err(map_cache_err)?;
    if !allowed {
        return Err(FileError::RateLimited.into());
    }
    Ok(())
}
