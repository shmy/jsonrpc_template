#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::StatusCode;
use axum_jsonrpc::{ErrorObject, MethodSpec, Params, RpcRouter};

use crate::{OpenRpcInfo, OpenRpcServer, RpcRouterOpenRpcExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::ServiceExt;

#[derive(Debug, Deserialize, JsonSchema)]
struct EchoParams {
    message: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EchoResult {
    message: String,
}

async fn echo(Params(params): Params<EchoParams>) -> Result<EchoResult, ErrorObject> {
    Ok(EchoResult {
        message: params.message,
    })
}

#[test]
fn openrpc_methods_match_registered_names() {
    let router = RpcRouter::<()>::new()
        .register(
            MethodSpec::new("demo.echo", echo)
                .summary("Echo a message")
                .params::<EchoParams>()
                .result::<EchoResult>(),
        )
        .expect("register");

    let doc = router.openrpc(
        OpenRpcInfo {
            title: "Test".into(),
            version: "0.0.1".into(),
            description: None,
        },
        Vec::new(),
    );

    let doc_names: Vec<&str> = doc.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(doc_names, router.method_names());
    assert_eq!(doc.methods[0].summary.as_deref(), Some("Echo a message"));
    assert_eq!(doc.methods[0].param_structure, "by-name");
    assert!(doc.methods[0].params.iter().any(|p| p.name == "message"));
    assert!(doc.components.errors.contains_key("InvalidParams"));
}

#[tokio::test]
async fn http_openrpc_json_endpoint() {
    let app = RpcRouter::<()>::new()
        .register(
            MethodSpec::new("demo.echo", echo)
                .params::<EchoParams>()
                .result::<EchoResult>(),
        )
        .expect("register")
        .into_axum_openrpc_router_with_info(OpenRpcInfo {
            title: "Demo API".into(),
            version: "1.2.3".into(),
            description: None,
        });

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/openrpc.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["openrpc"], "1.3.2");
    assert_eq!(v["info"]["title"], "Demo API");
    assert_eq!(v["info"]["version"], "1.2.3");
    assert_eq!(v["methods"][0]["name"], "demo.echo");
    assert!(v.get("servers").is_none());
}

#[tokio::test]
async fn http_openrpc_json_includes_servers() {
    let app = RpcRouter::<()>::new()
        .register(
            MethodSpec::new("demo.echo", echo)
                .params::<EchoParams>()
                .result::<EchoResult>(),
        )
        .expect("register")
        .into_axum_openrpc_router_with(
            OpenRpcInfo {
                title: "Demo API".into(),
                version: "1.2.3".into(),
                description: Some("Demo description".into()),
            },
            vec![OpenRpcServer {
                name: "local".into(),
                url: "http://127.0.0.1:8080/jsonrpc".into(),
            }],
        );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/openrpc.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["info"]["title"], "Demo API");
    assert_eq!(v["info"]["version"], "1.2.3");
    assert_eq!(v["info"]["description"], "Demo description");
    assert_eq!(v["servers"][0]["name"], "local");
    assert_eq!(v["servers"][0]["url"], "http://127.0.0.1:8080/jsonrpc");
}

#[test]
fn openrpc_includes_method_registered_without_schema() {
    async fn ping() -> Result<&'static str, ErrorObject> {
        Ok("pong")
    }

    let router = RpcRouter::<()>::new()
        .method("ping", ping)
        .expect("register");
    let doc = router.openrpc(OpenRpcInfo::default(), Vec::new());
    let doc_names: Vec<&str> = doc.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(doc_names, router.method_names());
    assert_eq!(doc_names, vec!["ping"]);
}

#[test]
fn openrpc_merge_keeps_method_and_meta_aligned() {
    async fn left() -> Result<&'static str, ErrorObject> {
        Ok("l")
    }

    let a = RpcRouter::<()>::new().method("a.ping", left).expect("a");
    let b = RpcRouter::<()>::new()
        .register(
            MethodSpec::new("b.echo", echo)
                .params::<EchoParams>()
                .result::<EchoResult>(),
        )
        .expect("b");
    let merged = a.merge(b).expect("merge");
    let doc = merged.openrpc(OpenRpcInfo::default(), Vec::new());
    let doc_names: Vec<&str> = doc.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(doc_names, merged.method_names());
    assert_eq!(doc_names, vec!["a.ping", "b.echo"]);
}

#[test]
fn openrpc_positional_params_use_by_position() {
    async fn add(Params(args): Params<(i64, i64)>) -> Result<i64, ErrorObject> {
        Ok(args.0 + args.1)
    }

    let router = RpcRouter::<()>::new()
        .register(
            MethodSpec::new("math.add", add)
                .params::<(i64, i64)>()
                .result::<i64>(),
        )
        .expect("register");
    let doc = router.openrpc(OpenRpcInfo::default(), Vec::new());
    assert_eq!(doc.methods[0].param_structure, "by-position");
    assert!(
        !doc.methods[0].params.is_empty(),
        "positional methods should expose at least one param descriptor"
    );
}
