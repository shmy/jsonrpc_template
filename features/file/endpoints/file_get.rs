//! `GET /files/{*path}`：静态回源；可选 `?w=` 走 [`crate::shared::image_derive`]。

use std::path::Path;

use axum::body::Body;
use axum::extract::{Extension, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use blob_kit::Blob;
use file_contract::{DERIVE_WIDTHS_HINT, DeriveRequest, resolve_derive_request};
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

use crate::shared::blob_fs::BlobFsRoot;
use crate::shared::image_derive::{self, Materialize};

const DERIVED_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const DERIVE_RETRY_AFTER_SECS: &str = "1";

#[derive(Debug, Deserialize)]
pub(crate) struct DeriveQuery {
    /// 最长边像素；白名单见 [`file_contract::DERIVE_WIDTHS`]。
    w: Option<u32>,
}

/// nest `/files` 后的 GET。
pub(crate) async fn handler(
    State(blob): State<Blob>,
    Extension(fs): Extension<BlobFsRoot>,
    Query(query): Query<DeriveQuery>,
    request: Request,
) -> Response {
    let root = fs.as_path();
    let object_path = request.uri().path().trim_start_matches('/');

    let Some(width) = query.w else {
        return serve_dir(root, request).await;
    };

    let cache_path = match resolve_derive_request(object_path, width) {
        DeriveRequest::CachePath(path) => path,
        DeriveRequest::BadWidth => {
            return (
                StatusCode::BAD_REQUEST,
                format!("unsupported w; allow {DERIVE_WIDTHS_HINT}"),
            )
                .into_response();
        }
        // staging / 非图 / 已是派生缓存：忽略 w。
        DeriveRequest::NotApplicable => return serve_dir(root, request).await,
    };

    match blob.exists(&cache_path).await {
        Ok(true) => return serve_cached(root, &cache_path).await,
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(error = %err, path = %cache_path, "derive cache exists check failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let Some(permit) = image_derive::try_acquire() else {
        return derive_busy_response();
    };

    materialize_response(
        image_derive::materialize(&blob, object_path, &cache_path, width, permit).await,
        root,
        &cache_path,
    )
    .await
}

async fn materialize_response(outcome: Materialize, root: &Path, cache_path: &str) -> Response {
    match outcome {
        Materialize::OnDisk => serve_cached(root, cache_path).await,
        Materialize::Fresh(bytes) => webp_response(bytes),
        Materialize::SourceMissing => StatusCode::NOT_FOUND.into_response(),
        Materialize::Unprocessable => StatusCode::UNPROCESSABLE_ENTITY.into_response(),
        Materialize::Failed => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn derive_busy_response() -> Response {
    let mut res = StatusCode::SERVICE_UNAVAILABLE.into_response();
    res.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(DERIVE_RETRY_AFTER_SECS),
    );
    res
}

fn with_derived_cache_headers(mut response: Response) -> Response {
    if response.status().is_success() {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(DERIVED_CACHE_CONTROL),
        );
    }
    response
}

async fn serve_dir(root: &Path, request: Request) -> Response {
    match ServeDir::new(root).oneshot(request).await {
        Ok(res) => res.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn serve_cached(root: &Path, cache_path: &str) -> Response {
    let req = Request::new(Body::empty());
    match ServeFile::new(root.join(cache_path)).oneshot(req).await {
        Ok(res) => with_derived_cache_headers(res.into_response()),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn webp_response(bytes: Vec<u8>) -> Response {
    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CACHE_CONTROL, DERIVED_CACHE_CONTROL)
        .body(Body::from(bytes))
    {
        Ok(res) => res,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
