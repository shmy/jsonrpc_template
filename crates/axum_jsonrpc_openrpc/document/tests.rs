#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use axum_jsonrpc::MethodMeta;
use schemars::JsonSchema;
use serde_json::{Value, json};

use super::build_document;
use super::schema::{detect_param_structure, merge_definitions, rewrite_schema_refs};
use super::types::OpenRpcInfo;

fn schema_for_value<T: JsonSchema>() -> Value {
    schemars::schema_for!(T).to_value()
}

#[test]
fn merge_definitions_reads_definitions_and_dollar_defs() {
    let root = json!({
        "schema": { "type": "object" },
        "definitions": { "Old": { "type": "string" } },
        "$defs": { "New": { "type": "number" } }
    });
    let mut schemas = BTreeMap::new();
    merge_definitions(&root, &mut schemas);
    assert_eq!(schemas["Old"]["type"], "string");
    assert_eq!(schemas["New"]["type"], "number");
}

#[test]
fn detect_param_structure_for_object_and_array() {
    assert_eq!(
        detect_param_structure(&json!({"type": "object", "properties": {"a": {"type": "string"}}})),
        "by-name"
    );
    assert_eq!(
        detect_param_structure(
            &json!({"type": "array", "prefixItems": [{"type": "integer"}, {"type": "integer"}]})
        ),
        "by-position"
    );
}

#[test]
fn rewrite_schema_refs_points_at_components() {
    let mut schema = json!({
        "type": "array",
        "items": { "$ref": "#/definitions/Account" }
    });
    rewrite_schema_refs(&mut schema);
    assert_eq!(schema["items"]["$ref"], "#/components/schemas/Account");
}

#[test]
fn build_document_rewrites_nested_defs_refs() {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct Account {
        id: String,
    }

    let meta = MethodMeta {
        name: "identity.account.search".into(),
        summary: None,
        description: None,
        tags: vec!["identity".into()],
        deprecated: false,
        params_schema: None,
        result_schema: Some(schema_for_value::<Vec<Account>>()),
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let result_schema = &doc.methods[0].result.as_ref().unwrap().schema;
    let reference = result_schema
        .pointer("/items/$ref")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        reference.starts_with("#/components/schemas/"),
        "expected components ref, got {reference}"
    );
    let name = reference.trim_start_matches("#/components/schemas/");
    assert!(doc.components.schemas.contains_key(name));
}

#[test]
fn unit_result_synthesizes_null_example_value() {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct DeleteAccountRequest {
        #[schemars(example = &"1234567890")]
        id: String,
    }

    let meta = MethodMeta {
        name: "identity.account.delete".into(),
        summary: None,
        description: None,
        tags: vec!["identity".into()],
        deprecated: false,
        params_schema: Some(schema_for_value::<DeleteAccountRequest>()),
        result_schema: Some(schema_for_value::<()>()),
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let example = &doc.methods[0].examples[0];
    assert_eq!(example.name, "identity.account.deleteExample");
    assert_eq!(
        example
            .params
            .iter()
            .find(|p| p.name == "id")
            .map(|p| &p.value),
        Some(&json!("1234567890"))
    );
    assert_eq!(
        example.result.as_ref().map(|r| &r.value),
        Some(&Value::Null)
    );
}

#[test]
fn auth_required_methods_get_x_auth_extension() {
    let metas = [MethodMeta {
        name: "identity.account.read".into(),
        summary: None,
        description: None,
        tags: vec![],
        deprecated: false,
        params_schema: None,
        result_schema: None,
    }];
    let auth: BTreeSet<String> = ["identity.account.read".into()].into_iter().collect();
    let doc = build_document(OpenRpcInfo::default(), Vec::new(), &metas, &auth);
    assert_eq!(doc.methods[0].x_auth, Some("required"));

    let open = build_document(OpenRpcInfo::default(), Vec::new(), &metas, &BTreeSet::new());
    assert_eq!(open.methods[0].x_auth, None);
}

#[test]
fn primitive_fields_without_examples_get_type_placeholders() {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct TokenResponse {
        access_token: String,
        expires_in: u64,
        avatar: Option<String>,
    }

    let meta = MethodMeta {
        name: "identity.account.login".into(),
        summary: None,
        description: None,
        tags: vec![],
        deprecated: false,
        params_schema: None,
        result_schema: Some(schema_for_value::<TokenResponse>()),
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let result = doc.methods[0].examples[0].result.as_ref().unwrap();
    // 无显式示例的字段按类型占位；可空字段展示缺省形态 null。
    assert_eq!(result.value["access_token"], json!(""));
    assert_eq!(result.value["expires_in"], json!(0));
    assert_eq!(result.value["avatar"], json!(null));
}

#[test]
fn nullable_fields_synthesize_null_examples() {
    struct IdLike;

    impl JsonSchema for IdLike {
        fn inline_schema() -> bool {
            true
        }
        fn schema_name() -> Cow<'static, str> {
            "IdLike".into()
        }
        fn schema_id() -> Cow<'static, str> {
            "IdLike".into()
        }
        fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
            schemars::json_schema!({
                "type": "string",
                "pattern": "^[0-9]+$",
                "examples": ["1234567890"],
            })
        }
    }

    fn default_limit() -> usize {
        20
    }

    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct SearchRequest {
        name: Option<String>,
        /// 上一页最后一条的 id；首屏省略。
        cursor: Option<IdLike>,
        account: Option<AccountDump>,
        #[serde(default = "default_limit")]
        limit: usize,
    }

    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct AccountDump {
        id: String,
    }

    let meta = MethodMeta {
        name: "identity.account.search".into(),
        summary: None,
        description: None,
        tags: vec![],
        deprecated: false,
        params_schema: Some(schema_for_value::<SearchRequest>()),
        result_schema: None,
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let example = &doc.methods[0].examples[0];
    let value_of = |name: &str| {
        example
            .params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.clone())
            .unwrap_or_else(|| panic!("missing param {name}"))
    };
    // Option 字段一律合成 null：内联简单型（含内型自带 examples 的 Id）
    // 与 $ref 型（anyOf 含 null 变体）都覆盖。
    assert_eq!(value_of("name"), json!(null));
    assert_eq!(value_of("cursor"), json!(null));
    assert_eq!(value_of("account"), json!(null));
    // serde default 反射进 schema 的默认值优先于类型占位（0 会被 clamp 成 1）。
    assert_eq!(value_of("limit"), json!(20));
}

#[test]
fn schemars_field_examples_become_method_example_pairing() {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct CreateAccountRequest {
        #[schemars(example = &"Alice")]
        name: String,
        #[schemars(example = &"alice@example.com")]
        email: String,
    }

    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct CreateAccountResponse {
        #[schemars(example = &"alice@example.com")]
        id: String,
    }

    let meta = MethodMeta {
        name: "identity.account.create".into(),
        summary: None,
        description: None,
        tags: vec!["identity".into()],
        deprecated: false,
        params_schema: Some(schema_for_value::<CreateAccountRequest>()),
        result_schema: Some(schema_for_value::<CreateAccountResponse>()),
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let example = &doc.methods[0].examples[0];
    assert_eq!(example.name, "identity.account.createExample");
    assert_eq!(
        example
            .params
            .iter()
            .find(|p| p.name == "email")
            .map(|p| &p.value),
        Some(&json!("alice@example.com"))
    );
    assert_eq!(
        example.result.as_ref().map(|r| &r.value),
        Some(&json!({ "id": "alice@example.com" }))
    );
}

#[test]
fn field_doc_comments_become_summary_and_stay_in_schema() {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct CreateAccountRequest {
        /// The name of the account.
        name: String,
        /// The email of the account.
        ///
        /// Must be unique.
        email: String,
    }

    let meta = MethodMeta {
        name: "identity.account.create".into(),
        summary: None,
        description: None,
        tags: vec!["identity".into()],
        deprecated: false,
        params_schema: Some(schema_for_value::<CreateAccountRequest>()),
        result_schema: None,
    };
    let doc = build_document(
        OpenRpcInfo::default(),
        Vec::new(),
        &[meta],
        &BTreeSet::new(),
    );
    let name = doc.methods[0]
        .params
        .iter()
        .find(|p| p.name == "name")
        .expect("name param");
    assert_eq!(name.summary.as_deref(), Some("The name of the account."));
    assert!(name.description.is_none());
    assert_eq!(
        name.schema.get("description").and_then(Value::as_str),
        Some("The name of the account.")
    );

    let email = doc.methods[0]
        .params
        .iter()
        .find(|p| p.name == "email")
        .expect("email param");
    assert_eq!(email.summary.as_deref(), Some("The email of the account."));
    assert!(email.description.is_none());
    assert!(
        email
            .schema
            .get("description")
            .and_then(Value::as_str)
            .is_some_and(|d| d.contains("Must be unique."))
    );
}
