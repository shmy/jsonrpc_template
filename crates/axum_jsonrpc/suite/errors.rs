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
async fn dispatch_application_error() {
    async fn fail() -> Result<(), ErrorObject> {
        Err(ErrorObject::new(-32000, "app failed"))
    }

    let router = RpcRouter::<()>::new()
        .method("fail", fail)
        .expect("register");
    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"fail","id":7}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => {
            let err = resp.error.expect("error");
            assert_eq!(err.code, -32000);
            assert_eq!(err.message, "app failed");
            assert!(resp.result.is_none());
            assert_eq!(resp.id, Id::Number(7.into()));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[cfg(feature = "rootcause")]
#[tokio::test]
async fn dispatch_rootcause_maps_to_internal_error() {
    async fn boom() -> rootcause::Result<&'static str> {
        Err(rootcause::report!("boom"))
    }

    let router = RpcRouter::<()>::new()
        .method("boom", boom)
        .expect("register");
    let resp = assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"boom","id":1}"#,
                TEST_TRACE_ID,
            )
            .await,
        error_codes::INTERNAL_ERROR,
    );
    assert_eq!(resp.id, Id::Number(1.into()));
}

#[cfg(feature = "rootcause")]
#[tokio::test]
async fn dispatch_domain_mapper_maps_to_app_error() {
    #[derive(Debug)]
    struct FixtureNotFound;

    impl std::fmt::Display for FixtureNotFound {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("fixture_not_found")
        }
    }

    impl std::error::Error for FixtureNotFound {}

    fn ensure_fixture_mapper() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            crate::register_domain_mapper(|report| {
                report
                    .iter_downcast_context::<FixtureNotFound>()
                    .next()
                    .map(|_| ErrorObject::new(-32_042, "fixture missing"))
            });
        });
    }

    ensure_fixture_mapper();

    async fn boom() -> rootcause::Result<&'static str> {
        Err(FixtureNotFound.into())
    }

    let router = RpcRouter::<()>::new()
        .method("boom", boom)
        .expect("register");
    let resp = assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"boom","id":1}"#,
                TEST_TRACE_ID,
            )
            .await,
        -32_042,
    );
    assert_eq!(
        resp.error.as_ref().map(|e| e.message.as_str()),
        Some("fixture missing")
    );
}

#[test]
fn internal_error_uses_message_hook() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        crate::set_internal_error_message(|| "服务器内部错误".to_string());
    });
    let err = ErrorObject::internal_error();
    assert_eq!(err.message, "服务器内部错误");
}
