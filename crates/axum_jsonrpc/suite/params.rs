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
async fn validated_params_rejects_with_invalid_params() {
    // 不在此挂 set_invalid_params_renderer（OnceLock 全局，会污染并行用例）；
    // 默认 message 仍为 "Invalid params"；locale 文案由 http_server + e2e 覆盖。
    #[derive(Deserialize, Validate)]
    struct Login {
        #[validate(length(min = 8, max = 64, code = "password_invalid_length"))]
        password: String,
    }

    async fn login(
        ValidatedParams(_): ValidatedParams<Login>,
    ) -> Result<&'static str, ErrorObject> {
        Ok("ok")
    }

    let router = RpcRouter::<()>::new()
        .method("login", login)
        .expect("register");
    let resp = assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"login","params":{"password":"12"},"id":1}"#,
                TEST_TRACE_ID,
            )
            .await,
        error_codes::INVALID_PARAMS,
    );
    let err = resp.error.expect("error");
    assert_eq!(err.message, "Invalid params");
}

#[tokio::test]
async fn dispatch_positional_params_tuple() {
    async fn subtract(
        Params((minuend, subtrahend)): Params<(i64, i64)>,
    ) -> Result<i64, ErrorObject> {
        Ok(minuend - subtrahend)
    }

    let router = RpcRouter::<()>::new()
        .method("subtract", subtract)
        .expect("register");

    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"subtract","params":[42,23],"id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => {
            assert_eq!(result_value(&resp), json!(19));
            assert_eq!(resp.id, Id::Number(1.into()));
        }
        other => panic!("unexpected {other:?}"),
    }
}
