//! Fs 本地预签名直传：`PUT /files/presigned?path=&exp=&ct=&sig=`（PublicRouter）。
//!
//! 鉴权靠 HMAC（与 Cos 预签名同形：无 Bearer）。仅 `blob-fs` 注册。
//! 先验签再读 body，避免匿名打满内存。

use std::io::Cursor;

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use blob_kit::{Blob, is_staging_object_path};
use file_contract::FileError;
use locale::translate_current;
use serde::Deserialize;

use crate::shared::http_error::{HttpErrorBody, file_error_response};
use axum::body::to_bytes;
use axum_jsonrpc::error_codes::INTERNAL_ERROR;

#[derive(Debug, Deserialize)]
pub(crate) struct PresignedPutQuery {
    pub path: String,
    pub exp: u64,
    pub ct: String,
    pub sig: String,
}

/// 与 [`http_routing::PUBLIC_BODY_LIMIT_BYTES`] 同值（PublicRouter 默认上限）。
pub(crate) const PRESIGNED_PUT_BODY_LIMIT: usize = http_routing::PUBLIC_BODY_LIMIT_BYTES;

fn attachment_invalid() -> rootcause::Report {
    FileError::AttachmentInvalid.into()
}

#[tracing::instrument(skip(blob, headers, request))]
pub(crate) async fn handler(
    State(blob): State<Blob>,
    Query(q): Query<PresignedPutQuery>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    match execute(&blob, &q, &headers, request).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => map_err(err),
    }
}

async fn execute(
    blob: &Blob,
    q: &PresignedPutQuery,
    headers: &HeaderMap,
    request: Request,
) -> rootcause::Result<()> {
    let path = q.path.trim();
    if !is_staging_object_path(path) {
        return Err(attachment_invalid());
    }

    let header_ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if header_ct != q.ct {
        return Err(attachment_invalid());
    }

    if let Some(len) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        && (len == 0 || len > PRESIGNED_PUT_BODY_LIMIT)
    {
        return Err(attachment_invalid());
    }

    let Some(fs) = blob.as_fs() else {
        return Err(attachment_invalid());
    };
    if fs.verify_presign_put(path, q.exp, &q.ct, &q.sig).is_err() {
        return Err(attachment_invalid());
    }

    let body = to_bytes(request.into_body(), PRESIGNED_PUT_BODY_LIMIT)
        .await
        .map_err(|_| attachment_invalid())?;
    if body.is_empty() {
        return Err(attachment_invalid());
    }

    let _ = blob.write(path, Cursor::new(body)).await?;
    Ok(())
}

fn map_err(err: rootcause::Report) -> Response {
    if let Some(file_err) = err.iter_downcast_context::<FileError>().next() {
        return file_error_response(file_err);
    }
    tracing::error!(error = %err, "presigned put failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(HttpErrorBody {
            code: INTERNAL_ERROR,
            message: translate_current("internal_server_error"),
        }),
    )
        .into_response()
}
