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

#[tokio::test]
async fn dispatch_parse_error() {
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(parts(), b"{", TEST_TRACE_ID)
            .await,
        error_codes::PARSE_ERROR,
    );
    assert_eq!(resp.id, Id::Null);
}

#[tokio::test]
async fn dispatch_all_notifications_empty() {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct P {
        n: i32,
    }

    async fn notify(Params(_p): Params<P>) -> Result<(), ErrorObject> {
        Ok(())
    }

    let router = RpcRouter::<()>::new()
        .method("notify_hello", notify)
        .expect("register");
    let outcome = router
        .dispatch_bytes(
            parts(),
            br#"[{"jsonrpc":"2.0","method":"notify_hello","params":{"n":7}}]"#,
            TEST_TRACE_ID,
        )
        .await;
    assert!(matches!(outcome, DispatchOutcome::Empty));
}

#[tokio::test]
async fn dispatch_method_not_found() {
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"foobar","id":"1"}"#,
                TEST_TRACE_ID,
            )
            .await,
        error_codes::METHOD_NOT_FOUND,
    );
    assert_eq!(resp.id, Id::String("1".into()));
}

#[tokio::test]
async fn dispatch_success_and_invalid_params() {
    #[derive(Deserialize)]
    struct Add {
        a: i32,
        b: i32,
    }

    async fn add(Params(p): Params<Add>) -> Result<i32, ErrorObject> {
        Ok(p.a + p.b)
    }

    let router = RpcRouter::<()>::new().method("add", add).expect("register");

    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"add","params":{"a":1,"b":2},"id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => {
            assert_eq!(result_value(&resp), json!(3));
            assert!(resp.error.is_none());
        }
        other => panic!("unexpected {other:?}"),
    }

    let resp = assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"add","params":{"a":"x","b":2},"id":2}"#,
                TEST_TRACE_ID,
            )
            .await,
        error_codes::INVALID_PARAMS,
    );
    let err = resp.error.expect("error");
    assert_eq!(err.message, "Invalid params");
    assert_eq!(
        err.data
            .as_ref()
            .and_then(|d| d.get("trace_id"))
            .and_then(Value::as_str),
        Some(TEST_TRACE_ID)
    );
}

#[tokio::test]
async fn dispatch_scalar_root_is_invalid_request() {
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(parts(), b"1", TEST_TRACE_ID)
            .await,
        error_codes::INVALID_REQUEST,
    );
    assert_eq!(resp.id, Id::Null);
}

#[tokio::test]
async fn dispatch_single_notification_empty() {
    async fn update(Params(_): Params<Vec<i32>>) -> Result<(), ErrorObject> {
        Ok(())
    }

    let router = RpcRouter::<()>::new()
        .method("update", update)
        .expect("register");
    let outcome = router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"update","params":[1,2,3]}"#,
            TEST_TRACE_ID,
        )
        .await;
    assert!(matches!(outcome, DispatchOutcome::Empty));
}

#[tokio::test]
async fn dispatch_leading_whitespace_and_pretty_body() {
    // 前导空白 + 多行 body + params 内部空白：RawValue 首字节形状检查须先 trim。
    #[derive(Deserialize)]
    struct Add {
        a: i32,
        b: i32,
    }

    async fn add(Params(p): Params<Add>) -> Result<i32, ErrorObject> {
        Ok(p.a + p.b)
    }

    let router = RpcRouter::<()>::new().method("add", add).expect("register");
    match router
        .dispatch_bytes(
            parts(),
            b"  \n\t{\n  \"jsonrpc\": \"2.0\",\n  \"method\": \"add\",\n  \"params\": { \"a\": 1, \"b\": 2 },\n  \"id\": 1\n}\n",
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!(3)),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_named_params_spec_example() {
    #[derive(Deserialize)]
    struct Subtract {
        minuend: i64,
        subtrahend: i64,
    }

    async fn subtract(Params(p): Params<Subtract>) -> Result<i64, ErrorObject> {
        Ok(p.minuend - p.subtrahend)
    }

    let router = RpcRouter::<()>::new()
        .method("subtract", subtract)
        .expect("register");

    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"subtract","params":{"subtrahend":23,"minuend":42},"id":3}"#,
            TEST_TRACE_ID)
        .await
    {
        DispatchOutcome::Single(resp) => {
            assert_eq!(result_value(&resp), json!(19));
            assert_eq!(resp.id, Id::Number(3.into()));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_notification_unknown_method_empty() {
    let outcome = RpcRouter::<()>::new()
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"foobar"}"#,
            TEST_TRACE_ID,
        )
        .await;
    assert!(matches!(outcome, DispatchOutcome::Empty));
}

#[tokio::test]
async fn dispatch_notification_invalid_params_empty() {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct P {
        n: i32,
    }

    async fn notify(Params(_): Params<P>) -> Result<(), ErrorObject> {
        Ok(())
    }

    let router = RpcRouter::<()>::new()
        .method("notify", notify)
        .expect("register");
    let outcome = router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"notify","params":{"n":"bad"}}"#,
            TEST_TRACE_ID,
        )
        .await;
    assert!(matches!(outcome, DispatchOutcome::Empty));
}

#[tokio::test]
async fn dispatch_omitted_params_unit() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"ping","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!("pong")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_null_result() {
    async fn nada() -> Result<Option<&'static str>, ErrorObject> {
        Ok(None)
    }

    let router = RpcRouter::<()>::new()
        .method("nada", nada)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"nada","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => {
            assert_eq!(result_value(&resp), Value::Null);
            assert!(resp.error.is_none());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_string_id_preserved() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"ping","id":"req-42"}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(resp.id, Id::String("req-42".into())),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_id_null_request_echoes_null() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"ping","id":null}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => {
            assert_eq!(resp.id, Id::Null);
            assert_eq!(result_value(&resp), json!("pong"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_wrong_jsonrpc_preserves_id() {
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"1.0","method":"ping","id":9}"#,
                TEST_TRACE_ID,
            )
            .await,
        error_codes::INVALID_REQUEST,
    );
    assert_eq!(resp.id, Id::Number(9.into()));
}

#[tokio::test]
async fn outcome_empty_is_http_200_null() {
    // jsonrpsee ACK: 200 + JSON `null`.
    let response = DispatchOutcome::Empty.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"null");
}
