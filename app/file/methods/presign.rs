use std::time::Duration;

use app_state::AuthRateLimit;
use axum::extract::State;
use axum_jsonrpc::{ValidatedParams, rpc};
use blob_kit::Blob;
use cache_kit::Cache;
use file_contract::{FileError, staging_dated_file_path};
use http_auth::AuthedAccount;
use rootcause::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use validify::Validate;

use crate::shared::allowed_extensions;
use crate::shared::extensions::content_type_for_extension;
use crate::shared::rate_limit::enforce_presign_subject;

const DEFAULT_EXPIRE_SECS: u64 = 600;
const MAX_EXPIRE_SECS: u64 = 3600;

#[derive(Debug, Deserialize, JsonSchema, Validate)]
pub struct PresignRequest {
    /// 扩展名（不含点）；须在 `BLOB_ALLOWED_EXTENSIONS` 内，且 `mime_guess` 能识别。
    #[validate(length(min = 1, max = 16, code = "invalid_params_invalid_length"))]
    pub extension: String,
    /// 预签名有效期秒数；默认 600，上限 3600。
    pub expire_secs: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PresignResponse {
    pub path: String,
    pub upload_url: String,
    pub public_url: String,
    /// PUT 时须原样带上的 `Content-Type`（与 `upload_url` 的 `ct` 一致）。
    pub content_type: String,
}

/// 生成预签名直传 URL（PrivateRpc）。
///
/// - **COS**：对象存储原生 PUT URL
/// - **fs**：本服务 `PUT /files/presigned?...`（HMAC，与 Cos 同形）
///
/// 服务端**不**校验文件内容（与 Cos 直传一致）；扩展名白名单见 `BLOB_ALLOWED_EXTENSIONS`。
#[rpc("file.presign")]
#[tracing::instrument(skip(blob, cache, auth_rate, request), fields(%caller))]
pub async fn presign(
    AuthedAccount(caller): AuthedAccount,
    State(blob): State<Blob>,
    State(cache): State<Cache>,
    State(auth_rate): State<AuthRateLimit>,
    ValidatedParams(request): ValidatedParams<PresignRequest>,
) -> Result<PresignResponse> {
    enforce_presign_subject(&cache, auth_rate, caller).await?;
    let ext = request
        .extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if !allowed_extensions::is_allowed(&ext) {
        return Err(FileError::AttachmentInvalid.into());
    }
    let Some(content_type) = content_type_for_extension(&ext) else {
        return Err(FileError::AttachmentInvalid.into());
    };
    let expire = Duration::from_secs(
        request
            .expire_secs
            .unwrap_or(DEFAULT_EXPIRE_SECS)
            .clamp(1, MAX_EXPIRE_SECS),
    );
    let path = staging_dated_file_path(&ext);
    let upload_url = blob
        .presign_upload_url(&path, &content_type, expire)
        .await
        .map_err(file_attachment::map_blob_report)?;

    file_attachment::claim_staging(&cache, &path, caller).await?;
    let public_url = blob.fill_public_url(&path).into_owned();
    Ok(PresignResponse {
        path,
        upload_url,
        public_url,
        content_type,
    })
}
