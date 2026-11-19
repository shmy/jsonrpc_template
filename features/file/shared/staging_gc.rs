//! `staging/` 过期对象清理。

use blob_kit::Blob;
use file_contract::STORAGE_DIR_PREFIX;
use jiff::Span;
use jiff::civil::Date;
use jiff::{Timestamp, tz::TimeZone};
use rootcause::Result;

/// 默认保留整月粒度 30 天（与 modrun 对齐）。
pub const STAGING_RETENTION_DAYS: u32 = 30;

fn cutoff_from_now(retention_days: u32) -> Date {
    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
    today
        .checked_sub(Span::new().days(i64::from(retention_days)))
        .unwrap_or(today)
}

async fn collect_expired(blob: &Blob, cutoff_month: Date) -> Result<Vec<String>> {
    let mut expired = Vec::new();
    let year_dirs = blob.list_children(STORAGE_DIR_PREFIX).await?;
    for year_dir in year_dirs {
        if year_dir.is_file {
            expired.push(year_dir.path);
            continue;
        }
        let month_dirs = blob.list_children(&year_dir.path).await?;
        for month_dir in month_dirs {
            if month_dir.is_file {
                expired.push(month_dir.path);
                continue;
            }
            let Some(month) = file_contract::staging_dir_date(&month_dir.path) else {
                continue;
            };
            if month < cutoff_month {
                let files = blob.list_children(&month_dir.path).await?;
                for file in files {
                    if file.is_file {
                        expired.push(file.path);
                    }
                }
            }
        }
    }
    Ok(expired)
}

/// 清理 `staging/` 下过期的对象，返回删除数量。
pub async fn purge_expired(blob: &Blob, retention_days: u32) -> Result<u64> {
    let cutoff = cutoff_from_now(retention_days);
    let expired = collect_expired(blob, cutoff).await?;
    let deleted = expired.len() as u64;
    if deleted > 0 {
        blob.delete_many(expired).await?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn collect_only_expired_months_and_stray_files() {
        let blob = Blob::new_for_test().await.expect("test blob");
        for path in [
            "staging/2026/07/old.webp",
            "staging/2026/08/old.webp",
            "staging/2026/09/keep.webp",
        ] {
            blob.write(path, Cursor::new("x")).await.expect("write");
        }
        blob.write("staging/orphan.webp", Cursor::new("x"))
            .await
            .expect("write orphan");

        // 与月份目录的「月初」比较：cutoff=9/1 时仅 <9 月过期，9 月保留。
        let cutoff = Date::new(2026, 9, 1).expect("valid date");
        let expired = collect_expired(&blob, cutoff).await.expect("collect");

        assert!(expired.iter().any(|p| p.contains("/07/")));
        assert!(expired.iter().any(|p| p.contains("/08/")));
        assert!(!expired.iter().any(|p| p.contains("/09/")));
        assert!(expired.iter().any(|p| p == "staging/orphan.webp"));
    }
}
