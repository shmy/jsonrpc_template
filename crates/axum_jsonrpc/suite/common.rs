use axum::body::Body;
use axum::extract::Request;
use axum::http::Response as HttpResponse;
use axum::http::header::CONTENT_TYPE;
use axum::http::request::Parts;
use serde_json::Value;
use serde_json::value::RawValue;

use super::super::parse::{ParseCall, parse_call};
use super::super::router::DispatchOutcome;
use super::super::trace_id::REQUEST_ID_HEADER;
use super::super::types::Response;

pub(super) const TEST_TRACE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const KNOWN_TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

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
            resp
        }
        other => panic!("unexpected {other:?}"),
    }
}

pub(super) fn assert_request_id(response: &HttpResponse<Body>, expected: &str) {
    assert_eq!(
        response
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some(expected)
    );
}

pub(super) fn assert_minted_request_id(response: &HttpResponse<Body>) {
    let id = response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok());
    assert!(
        id.is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())),
        "expected minted 32-hex {REQUEST_ID_HEADER}, got {id:?}"
    );
}
