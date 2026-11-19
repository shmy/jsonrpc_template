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
async fn dispatch_empty_batch() {
    assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(parts(), b"[]", TEST_TRACE_ID)
            .await,
        error_codes::INVALID_REQUEST,
    );
}

#[tokio::test]
async fn dispatch_batch_too_large() {
    let items: Vec<Value> = (0..=MAX_BATCH_LEN)
        .map(|i| json!({"jsonrpc":"2.0","method":"x","id": i}))
        .collect();
    let body = serde_json::to_vec(&items).expect("serialize");
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(parts(), &body, TEST_TRACE_ID)
            .await,
        error_codes::TOO_BIG_BATCH_REQUEST,
    );
    let err = resp.error.expect("error");
    assert_eq!(err.message, "The batch request was too large");
    assert!(err.data.is_some());
}

#[tokio::test]
async fn dispatch_mixed_batch_spec_example() {
    async fn sum(Params(nums): Params<Vec<i64>>) -> Result<i64, ErrorObject> {
        Ok(nums.iter().sum())
    }
    async fn subtract(Params((a, b)): Params<(i64, i64)>) -> Result<i64, ErrorObject> {
        Ok(a - b)
    }
    async fn notify_hello(Params(_): Params<Vec<i64>>) -> Result<(), ErrorObject> {
        Ok(())
    }
    async fn get_data() -> Result<Value, ErrorObject> {
        Ok(json!(["hello", 5]))
    }

    let router = RpcRouter::<()>::new()
        .method("sum", sum)
        .expect("register")
        .method("subtract", subtract)
        .expect("register")
        .method("notify_hello", notify_hello)
        .expect("register")
        .method("get_data", get_data)
        .expect("register");

    let body = br#"[
        {"jsonrpc":"2.0","method":"sum","params":[1,2,4],"id":"1"},
        {"jsonrpc":"2.0","method":"notify_hello","params":[7]},
        {"jsonrpc":"2.0","method":"subtract","params":[42,23],"id":"2"},
        {"foo":"boo"},
        {"jsonrpc":"2.0","method":"foo.get","params":{"name":"myself"},"id":"5"},
        {"jsonrpc":"2.0","method":"get_data","id":"9"}
    ]"#;

    match router.dispatch_bytes(parts(), body, TEST_TRACE_ID).await {
        DispatchOutcome::Batch(responses) => {
            assert_eq!(responses.len(), 5);
            assert_eq!(result_value(&responses[0]), json!(7));
            assert_eq!(responses[0].id, Id::String("1".into()));
            assert_eq!(result_value(&responses[1]), json!(19));
            assert_eq!(responses[1].id, Id::String("2".into()));
            assert_eq!(
                responses[2].error.as_ref().map(|e| e.code),
                Some(error_codes::INVALID_REQUEST)
            );
            assert_eq!(responses[2].id, Id::Null);
            assert_eq!(
                responses[3].error.as_ref().map(|e| e.code),
                Some(error_codes::METHOD_NOT_FOUND)
            );
            assert_eq!(responses[3].id, Id::String("5".into()));
            assert_eq!(result_value(&responses[4]), json!(["hello", 5]));
            assert_eq!(responses[4].id, Id::String("9".into()));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_invalid_batch_scalars() {
    match RpcRouter::<()>::new()
        .dispatch_bytes(parts(), b"[1,2,3]", TEST_TRACE_ID)
        .await
    {
        DispatchOutcome::Batch(responses) => {
            assert_eq!(responses.len(), 3);
            for resp in responses {
                assert_eq!(resp.id, Id::Null);
                assert_eq!(
                    resp.error.as_ref().map(|e| e.code),
                    Some(error_codes::INVALID_REQUEST)
                );
            }
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn batch_concurrency_preserves_response_order() {
    async fn echo(Params((n,)): Params<(i64,)>) -> Result<i64, ErrorObject> {
        Ok(n)
    }

    let body = br#"[
        {"jsonrpc":"2.0","method":"echo","params":[1],"id":1},
        {"jsonrpc":"2.0","method":"echo","params":[2],"id":2},
        {"jsonrpc":"2.0","method":"echo","params":[3],"id":3}
    ]"#;

    for concurrency in [1usize, 2, 0] {
        let router = RpcRouter::<()>::new()
            .batch_concurrency(concurrency)
            .method("echo", echo)
            .expect("register");
        match router.dispatch_bytes(parts(), body, TEST_TRACE_ID).await {
            DispatchOutcome::Batch(responses) => {
                assert_eq!(responses.len(), 3, "concurrency={concurrency}");
                assert_eq!(result_value(&responses[0]), json!(1));
                assert_eq!(result_value(&responses[1]), json!(2));
                assert_eq!(result_value(&responses[2]), json!(3));
            }
            other => panic!("unexpected {other:?} concurrency={concurrency}"),
        }
    }
}

#[tokio::test]
async fn dispatch_batch_out_of_range_number_is_invalid_item() {
    // 逐 item 校验（与 jsonrpsee 一致）：serde_json 无法表示的数字（1e400）
    // 只让该 item 变 Invalid Request，不拖垮整批（旧整树 parse 时代为整批 -32700）。
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"[{"jsonrpc":"2.0","method":"ping","id":1e400},{"jsonrpc":"2.0","method":"ping","id":2}]"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Batch(responses) => {
            assert_eq!(responses.len(), 2);
            assert_eq!(responses[0].id, Id::Null);
            assert_eq!(
                responses[0].error.as_ref().map(|e| e.code),
                Some(error_codes::INVALID_REQUEST)
            );
            assert_eq!(responses[1].id, Id::Number(2.into()));
            assert_eq!(result_value(&responses[1]), json!("pong"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_invalid_batch_single_element() {
    match RpcRouter::<()>::new()
        .dispatch_bytes(parts(), b"[1]", TEST_TRACE_ID)
        .await
    {
        DispatchOutcome::Batch(responses) => {
            assert_eq!(responses.len(), 1);
            assert_eq!(responses[0].id, Id::Null);
            assert_eq!(
                responses[0].error.as_ref().map(|e| e.code),
                Some(error_codes::INVALID_REQUEST)
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_batch_truncated_json_is_parse_error() {
    let resp = assert_single_error(
        RpcRouter::<()>::new()
            .dispatch_bytes(
                parts(),
                br#"[{"jsonrpc":"2.0","method":"sum","params":[1,2,4],"id":"1"},{"jsonrpc":"2.0","method""#,
            TEST_TRACE_ID)
            .await,
        error_codes::PARSE_ERROR,
    );
    assert_eq!(resp.id, Id::Null);
}
