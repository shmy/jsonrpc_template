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
fn parse_named_request() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "subtract",
        "params": {"minuend": 42, "subtrahend": 23},
        "id": 3
    }));
    match parsed {
        ParseCall::Request { call, id } => {
            assert_eq!(call.method, "subtract");
            assert!(matches!(id, Id::Number(_)));
            assert!(
                call.params
                    .as_ref()
                    .is_some_and(|p| p.get().trim_start().starts_with('{'))
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_notification_has_no_id() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "update",
        "params": [1, 2, 3]
    }));
    assert!(matches!(parsed, ParseCall::Notification(_)));
}

#[test]
fn parse_invalid_method_type() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": 1,
        "params": "bar"
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Null);
            assert_eq!(error.code, error_codes::INVALID_REQUEST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_invalid_params_shape() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "foo",
        "params": "bar",
        "id": 1
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Number(1.into()));
            assert_eq!(error.code, error_codes::INVALID_PARAMS);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_params_null_is_invalid_params() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "foo",
        "params": null,
        "id": 1
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Number(1.into()));
            assert_eq!(error.code, error_codes::INVALID_PARAMS);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_id_null_is_request_not_notification() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": null
    }));
    match parsed {
        ParseCall::Request { id, .. } => assert_eq!(id, Id::Null),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_invalid_id_type() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": true
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Null);
            assert_eq!(error.code, error_codes::INVALID_REQUEST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

// --- Spec §7 leftovers & protocol edges ---

#[test]
fn parse_wrong_jsonrpc_version() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "1.0",
        "method": "ping",
        "id": 1
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Number(1.into()));
            assert_eq!(error.code, error_codes::INVALID_REQUEST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_missing_jsonrpc() {
    let parsed = parse_call_json(json!({
        "method": "ping",
        "id": 1
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Number(1.into()));
            assert_eq!(error.code, error_codes::INVALID_REQUEST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_missing_method() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "id": 1
    }));
    match parsed {
        ParseCall::Invalid { id, error } => {
            assert_eq!(id, Id::Number(1.into()));
            assert_eq!(error.code, error_codes::INVALID_REQUEST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parse_omitted_params() {
    let parsed = parse_call_json(json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": 1
    }));
    match parsed {
        ParseCall::Request { call, .. } => assert!(call.params.is_none()),
        other => panic!("unexpected {other:?}"),
    }
}

// --- One-pass parser regression edges ---

#[test]
fn parse_duplicate_keys_last_wins() {
    // 与旧 `Value` 语义一致：重复 key 后值覆盖前值（serde_json Map insert 行为）。
    let parsed = parse_call(r#"{"jsonrpc":"1.0","jsonrpc":"2.0","method":"ping","id":1}"#);
    assert!(matches!(parsed, ParseCall::Request { .. }));
}

#[test]
fn parse_float_id_is_valid_number() {
    // 旧实现接受 float id（Value::Number）；新 IdField::Number 保持一致。
    let parsed = parse_call(r#"{"jsonrpc":"2.0","method":"ping","id":1.5}"#);
    match parsed {
        ParseCall::Request { id, .. } => match id {
            Id::Number(n) => assert_eq!(n.to_string(), "1.5"),
            other => panic!("unexpected {other:?}"),
        },
        other => panic!("unexpected {other:?}"),
    }
}
