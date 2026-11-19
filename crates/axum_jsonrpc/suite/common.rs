use axum::body::Body;
use axum::extract::Request;
use axum::http::header::CONTENT_TYPE;
use axum::http::request::Parts;
use serde_json::Value;
use serde_json::value::RawValue;

use super::super::parse::{ParseCall, parse_call};
use super::super::router::DispatchOutcome;
use super::super::types::Response;

pub(super) const TEST_TRACE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// `parse_call` 接口为 raw JSON 文本；测试侧从 `Value` 构造。
pub(super) fn parse_call_json(value: Value) -> ParseCall {
    parse_call(&serde_json::to_string(&value).expect("serialize"))
}

/// `Response.result` 现为 raw JSON 文本；解析回 `Value` 便于断言。
pub(super) fn result_value(resp: &Response) -> Value {
    let raw = resp.result.as_ref().expect("result");
    serde_json::from_str(raw.get()).expect("parse result")
}

pub(super) fn raw_json(value: impl serde::Serialize) -> Box<RawValue> {
    RawValue::from_string(serde_json::to_string(&value).expect("serialize")).expect("raw value")
}

pub(super) fn parts() -> Parts {
    Request::builder()
        .method("POST")
        .uri("/jsonrpc")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .expect("request")
        .into_parts()
        .0
}

pub(super) fn assert_single_error(outcome: DispatchOutcome, code: i32) -> Response {
    match outcome {
        DispatchOutcome::Single(resp) => {
            assert_eq!(resp.error.as_ref().map(|e| e.code), Some(code));
            let data = resp
                .error
                .as_ref()
                .and_then(|e| e.data.as_ref())
                .expect("error.data");
            assert_eq!(
                data.get("trace_id").and_then(Value::as_str),
                Some(TEST_TRACE_ID)
            );
            resp
        }
        other => panic!("unexpected {other:?}"),
    }
}
