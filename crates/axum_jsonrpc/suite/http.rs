#![allow(unused_imports)]
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{self, HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use validify::Validate;

use super::super::observer::RpcObserver;
use super::super::params::{Params, ValidatedParams};
use super::super::parse::{ParseCall, parse_call};
use super::super::router::{DispatchOutcome, RegisterError, RpcRouter, is_json_content_type};
use super::super::types::{ErrorObject, Id, MAX_BATCH_LEN, MAX_BODY_BYTES, Response, error_codes};
use super::common::{
    TEST_TRACE_ID, assert_single_error, parse_call_json, parts, raw_json, result_value,
};

#[test]
fn response_wire_shape_exclusive_result_error() {
    let ok = serde_json::to_value(Response::success(
        Id::Number(1.into()),
        raw_json(json!(true)),
    ))
    .unwrap();
    assert_eq!(ok["jsonrpc"], "2.0");
    assert_eq!(ok["result"], true);
    assert!(ok.get("error").is_none());

    let err = serde_json::to_value(Response::error(Id::Null, ErrorObject::parse_error())).unwrap();
    assert_eq!(err["jsonrpc"], "2.0");
    assert!(err.get("result").is_none());
    assert_eq!(err["error"]["code"], error_codes::PARSE_ERROR);
}

#[test]
fn response_null_result_is_serialized() {
    let v = serde_json::to_value(Response::success(
        Id::Number(1.into()),
        raw_json(Value::Null),
    ))
    .unwrap();
    assert!(v.get("result").is_some());
    assert!(v["result"].is_null());
}

#[tokio::test]
async fn http_rejects_non_json_content_type() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let app = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register")
        .into_axum_rpc_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "text/plain")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn http_success_and_notification() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }
    async fn update(Params(_): Params<Vec<i32>>) -> Result<(), ErrorObject> {
        Ok(())
    }

    let app = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register")
        .method("update", update)
        .expect("register")
        .into_axum_rpc_router();

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(
        ok.headers().get(CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );
    let body = to_bytes(ok.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["result"], "pong");
    assert_eq!(v["id"], 1);
    assert!(v.get("error").is_none());

    let notify = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"update","params":[1]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(notify.status(), StatusCode::OK);
    let notify_body = to_bytes(notify.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&notify_body[..], b"null");
}

#[tokio::test]
async fn http_notification_invalid_params_shape_is_null() {
    // Spec §4.1 / jsonrpsee ACK: 200 + `null`, not -32602 with id null.
    async fn bump() -> Result<(), ErrorObject> {
        Ok(())
    }

    let app = RpcRouter::<()>::new()
        .method("bump", bump)
        .expect("register")
        .into_axum_rpc_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"bump","params":"bar"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"null");
}

#[tokio::test]
async fn http_all_notifications_batch_is_null() {
    async fn update(Params(_): Params<Vec<i32>>) -> Result<(), ErrorObject> {
        Ok(())
    }

    let app = RpcRouter::<()>::new()
        .method("update", update)
        .expect("register")
        .into_axum_rpc_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"[{"jsonrpc":"2.0","method":"update","params":[1]},{"jsonrpc":"2.0","method":"update","params":[2]}]"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"null");
}

#[tokio::test]
async fn http_accepts_json_rpc_content_type() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let app = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register")
        .into_axum_rpc_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json-rpc")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn http_rpc_error_still_200() {
    let app = RpcRouter::<()>::new().into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"missing","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], error_codes::METHOD_NOT_FOUND);
    assert!(v.get("result").is_none());
    assert!(
        v["error"]["data"]["trace_id"]
            .as_str()
            .is_some_and(|id| id.len() == 32),
        "expected minted trace_id in error.data"
    );
}

#[tokio::test]
async fn http_scalar_root_is_200_invalid_request() {
    // 合法 JSON 标量根（数字）走 TopCall::ScalarRoot 的完整 HTTP 链路
    // （single_nested=false 分支）：200 + Invalid Request，id 为 null。
    let app = RpcRouter::<()>::new().into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("1"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], error_codes::INVALID_REQUEST);
    assert_eq!(v["id"], Value::Null);
    assert!(v.get("result").is_none());
}

#[tokio::test]
async fn http_error_reuses_traceparent_trace_id() {
    let app = RpcRouter::<()>::new().into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"missing","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["error"]["data"]["trace_id"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert!(v.get("result").is_none());
}

#[tokio::test]
async fn http_error_reuses_x_request_id() {
    let app = RpcRouter::<()>::new().into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .header("x-request-id", "4bf92f3577b34da6a3ce929d0e0e4736")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"missing","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["error"]["data"]["trace_id"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
}

#[tokio::test]
async fn http_oversized_body_error_includes_traceparent() {
    let app = RpcRouter::<()>::new().into_axum_rpc_router();
    let mut body = vec![b'x'; MAX_BODY_BYTES + 1];
    // Valid-looking start so content-type check passes; size triggers 413.
    body[..1].copy_from_slice(b"{");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"]["code"], error_codes::OVERSIZED_REQUEST);
    assert_eq!(
        v["error"]["data"]["trace_id"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(
        v["error"]["data"]["detail"],
        format!("Exceeded max limit of {MAX_BODY_BYTES}")
    );
}

#[tokio::test]
async fn http_error_uses_trace_id_fallback_before_mint() {
    let app = RpcRouter::<()>::new()
        .trace_id_fallback(|| Some("cccccccccccccccccccccccccccccccc".into()))
        .into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"missing","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["error"]["data"]["trace_id"],
        "cccccccccccccccccccccccccccccccc"
    );
}

#[tokio::test]
async fn observer_sees_calls_and_batch_size() {
    #[derive(Default)]
    struct RecordingObserver {
        calls: Mutex<Vec<(String, Option<i32>)>>,
        batches: Mutex<Vec<usize>>,
    }

    impl RpcObserver for RecordingObserver {
        fn on_call(&self, method: &str, error_code: Option<i32>, _elapsed: Duration) {
            self.calls
                .lock()
                .expect("lock")
                .push((method.to_owned(), error_code));
        }

        fn on_batch(&self, size: usize) {
            self.batches.lock().expect("lock").push(size);
        }
    }

    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let observer = Arc::new(RecordingObserver::default());
    let router = RpcRouter::<()>::new()
        .observer(Arc::clone(&observer) as Arc<dyn RpcObserver>)
        .method("ping", ping)
        .expect("register");

    let _ = router
        .dispatch_bytes(
            parts(),
            br#"[{"jsonrpc":"2.0","method":"ping","id":1},{"jsonrpc":"2.0","method":"missing","id":2}]"#,
            TEST_TRACE_ID,
        )
        .await;

    assert_eq!(observer.batches.lock().unwrap().as_slice(), &[2]);
    let calls = observer.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], ("ping".into(), None));
    assert_eq!(
        calls[1],
        ("missing".into(), Some(error_codes::METHOD_NOT_FOUND))
    );
}

#[tokio::test]
async fn http_success_has_no_error_data_trace_id() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let app = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register")
        .into_axum_rpc_router();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jsonrpc")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["result"], "pong");
    assert!(v.get("error").is_none());
    assert!(v.get("trace_id").is_none());
}
