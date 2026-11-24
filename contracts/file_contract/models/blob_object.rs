use jiff::Timestamp;
use serde::Serialize;
use toasty::Model;

/// 正式对象引用登记（staging 不进表；由 BlobGc 清 staging）。
#[derive(Debug, Serialize, Model)]
pub struct BlobObject {
    #[key]
    pub path: String,
    pub ref_count: i32,
    #[auto]
    pub created_at: Timestamp,
    pub tombstoned_at: Option<Timestamp>,
}
