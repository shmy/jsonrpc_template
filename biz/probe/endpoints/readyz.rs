use std::env::var;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use cache_kit::Cache;
use serde::Serialize;
use toasty::Db;

#[derive(Serialize)]
pub struct ReadyzResponse {
    pub image_tag: Option<String>,
}

/// Readiness: DB 可取连接且 Redis 可探活。
pub async fn readyz(State(db): State<Db>, State(cache): State<Cache>) -> impl IntoResponse {
    if db.connection().await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "Service unavailable").into_response();
    }
    if cache.probe().await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "Cache unavailable").into_response();
    }
    let image_tag = var("IMAGE_TAG").ok();
    Json(ReadyzResponse { image_tag }).into_response()
}
