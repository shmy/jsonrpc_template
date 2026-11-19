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
fn content_type_accepts_json_and_json_rpc() {
    assert!(!is_json_content_type(None));
    assert!(is_json_content_type(Some(&HeaderValue::from_static(
        "application/json"
    ))));
    assert!(is_json_content_type(Some(&HeaderValue::from_static(
        "application/json; charset=utf-8"
    ))));
    assert!(is_json_content_type(Some(&HeaderValue::from_static(
        "application/json;charset=utf-8"
    ))));
    assert!(is_json_content_type(Some(&HeaderValue::from_static(
        "application/json-rpc"
    ))));
    assert!(is_json_content_type(Some(&HeaderValue::from_static(
        "application/json-rpc; charset=utf-8"
    ))));
    assert!(!is_json_content_type(Some(&HeaderValue::from_static(
        "text/plain"
    ))));
    assert!(!is_json_content_type(Some(&HeaderValue::from_static(
        "application/json; charset=iso-8859-1"
    ))));
}
