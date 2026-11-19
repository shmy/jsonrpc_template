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
fn register_rejects_reserved_and_duplicate() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    match RpcRouter::<()>::new().method("rpc.foo", ping) {
        Err(RegisterError::ReservedMethod(_)) => {}
        Err(_) => panic!("expected reserved method error"),
        Ok(_) => panic!("reserved method should fail"),
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    match router.method("ping", ping) {
        Err(RegisterError::DuplicateMethod(_)) => {}
        Err(_) => panic!("expected duplicate method error"),
        Ok(_) => panic!("duplicate method should fail"),
    }
}

#[tokio::test]
async fn state_extractor_receives_router_state() {
    #[derive(Clone)]
    struct AppState {
        prefix: &'static str,
    }

    async fn greet(State(state): State<AppState>) -> Result<String, ErrorObject> {
        Ok(format!("{}-ok", state.prefix))
    }

    let router = RpcRouter::<AppState>::new()
        .method("greet", greet)
        .expect("register")
        .with_state(AppState { prefix: "hi" });

    match router
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"greet","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!("hi-ok")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn merge_keeps_self_state_state_replaces_after_merge() {
    #[derive(Clone)]
    struct St {
        id: u8,
    }

    async fn who(State(state): State<St>) -> Result<u8, ErrorObject> {
        Ok(state.id)
    }

    let merged = RpcRouter::<St>::new()
        .merge(RpcRouter::<St>::new().method("who", who).expect("register"))
        .expect("merge")
        .with_state(St { id: 1 });

    match merged
        .clone()
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"who","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!(1)),
        other => panic!("unexpected {other:?}"),
    }

    let replaced = merged.with_state(St { id: 9 });
    match replaced
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"who","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!(9)),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn merge_methods_and_duplicate() {
    async fn a() -> Result<&'static str, ErrorObject> {
        Ok("a")
    }
    async fn b() -> Result<&'static str, ErrorObject> {
        Ok("b")
    }

    let left = RpcRouter::<()>::new().method("a", a).expect("register");
    let right = RpcRouter::<()>::new().method("b", b).expect("register");
    let merged = left.merge(right).expect("merge");

    match merged
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"b","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!("b")),
        other => panic!("unexpected {other:?}"),
    }

    let dup = RpcRouter::<()>::new()
        .method("a", a)
        .expect("register")
        .merge(RpcRouter::<()>::new().method("a", a).expect("register"));
    assert!(matches!(dup, Err(RegisterError::DuplicateMethod(_))));
}

#[tokio::test]
async fn before_each_gates_only_wrapped_fragment_after_merge() {
    async fn public_ping() -> Result<&'static str, ErrorObject> {
        Ok("public")
    }
    async fn private_ping() -> Result<&'static str, ErrorObject> {
        Ok("private")
    }

    let public = RpcRouter::<()>::new()
        .method("public.ping", public_ping)
        .expect("register");
    let private = RpcRouter::<()>::new()
        .method("private.ping", private_ping)
        .expect("register")
        .before_each(|_parts, _state| Err(ErrorObject::new(-32020, "Access token is missing")));
    let merged = public.merge(private).expect("merge");

    match merged
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"public.ping","id":1}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!("public")),
        other => panic!("unexpected {other:?}"),
    }

    assert_single_error(
        merged
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"private.ping","id":2}"#,
                TEST_TRACE_ID,
            )
            .await,
        -32020,
    );
}

#[tokio::test]
async fn before_each_also_wraps_methods_registered_afterwards() {
    async fn early() -> Result<&'static str, ErrorObject> {
        Ok("early")
    }
    async fn late() -> Result<&'static str, ErrorObject> {
        Ok("late")
    }

    let router = RpcRouter::<()>::new()
        .method("early.ping", early)
        .expect("register")
        .before_each(|_parts, _state| Err(ErrorObject::new(-32020, "Access token is missing")))
        .method("late.ping", late)
        .expect("register");

    assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"early.ping","id":1}"#,
                TEST_TRACE_ID,
            )
            .await,
        -32020,
    );
    assert_single_error(
        router
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"late.ping","id":2}"#,
                TEST_TRACE_ID,
            )
            .await,
        -32020,
    );
}

#[tokio::test]
async fn before_each_sticky_does_not_wrap_merged_in_methods() {
    async fn gated() -> Result<&'static str, ErrorObject> {
        Ok("gated")
    }
    async fn open() -> Result<&'static str, ErrorObject> {
        Ok("open")
    }

    let gated = RpcRouter::<()>::new()
        .method("gated.ping", gated)
        .expect("register")
        .before_each(|_parts, _state| Err(ErrorObject::new(-32020, "Access token is missing")));
    let open = RpcRouter::<()>::new()
        .method("open.ping", open)
        .expect("register");
    // Merge open into gated: sticky gate must not wrap open.ping.
    let merged = gated.merge(open).expect("merge");

    assert_single_error(
        merged
            .dispatch_bytes(
                parts(),
                br#"{"jsonrpc":"2.0","method":"gated.ping","id":1}"#,
                TEST_TRACE_ID,
            )
            .await,
        -32020,
    );
    match merged
        .dispatch_bytes(
            parts(),
            br#"{"jsonrpc":"2.0","method":"open.ping","id":2}"#,
            TEST_TRACE_ID,
        )
        .await
    {
        DispatchOutcome::Single(resp) => assert_eq!(result_value(&resp), json!("open")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn merge_keeps_other_trace_id_fallback_when_self_empty() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let left = RpcRouter::<()>::new().method("a", ping).expect("register");
    let right = RpcRouter::<()>::new()
        .trace_id_fallback(|| Some("dddddddddddddddddddddddddddddddd".into()))
        .method("b", ping)
        .expect("register");
    let merged = left.merge(right).expect("merge");

    let response = merged
        .into_axum_rpc_router()
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
        "dddddddddddddddddddddddddddddddd"
    );
}
