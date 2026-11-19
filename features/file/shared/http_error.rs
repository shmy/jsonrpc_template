//! HTTP 上传面的轻量错误响应（与 RPC `ErrorObject` 码/文案对齐）。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use file_contract::FileError;
use locale::translate_current;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct HttpErrorBody {
    pub code: i32,
    pub message: String,
}

/// HTTP 上传面状态码倾向（RPC 仍用 [`FileError::rpc_code`]）。
#[must_use]
const fn http_status(err: &FileError) -> StatusCode {
    match err {
        FileError::AttachmentNotFound => StatusCode::NOT_FOUND,
        FileError::PresignUnsupported => StatusCode::NOT_IMPLEMENTED,
        FileError::AttachmentInvalid | FileError::AttachmentNotRegistered => {
            StatusCode::BAD_REQUEST
        }
    }
}

pub fn file_error_response(err: &FileError) -> Response {
    (
        http_status(err),
        Json(HttpErrorBody {
            code: err.rpc_code(),
            message: translate_current(err.l10n_key()),
        }),
    )
        .into_response()
}
