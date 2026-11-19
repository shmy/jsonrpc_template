use std::collections::{BTreeMap, BTreeSet};

use axum_jsonrpc::{MethodMeta, error_codes};
use serde_json::Value;

use super::examples::synthesize_examples;
use super::schema::{
    content_params_from_schema, detect_param_structure, merge_definitions, rewrite_schema_refs,
    schema_description, schema_object, split_summary_description, take_schema,
};
use super::types::{
    OpenRpcComponents, OpenRpcContent, OpenRpcDocument, OpenRpcError, OpenRpcInfo, OpenRpcMethod,
    OpenRpcServer, OpenRpcTag,
};

pub fn build_document(
    info: OpenRpcInfo,
    servers: Vec<OpenRpcServer>,
    metas: &[MethodMeta],
    auth_required: &BTreeSet<String>,
) -> OpenRpcDocument {
    let mut components = OpenRpcComponents {
        errors: standard_errors(),
        schemas: BTreeMap::new(),
    };

    let mut methods: Vec<OpenRpcMethod> = metas
        .iter()
        .map(|meta| method_from_meta(meta, &mut components.schemas))
        .collect();
    // Caller (`RpcRouter::openrpc`) already sorts metas; keep a stable order if not.
    methods.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    for method in &mut methods {
        if auth_required.contains(&method.name) {
            method.x_auth = Some("required");
        }
    }

    // schemars emits `#/definitions/X` (or `#/$defs/X`); OpenRPC hosts them under
    // `components.schemas`, so rewrite pointers for playground / validators.
    for method in &mut methods {
        for param in &mut method.params {
            rewrite_schema_refs(&mut param.schema);
        }
        if let Some(result) = &mut method.result {
            rewrite_schema_refs(&mut result.schema);
        }
    }
    for schema in components.schemas.values_mut() {
        rewrite_schema_refs(schema);
    }
    for method in &mut methods {
        method.examples = synthesize_examples(
            &method.name,
            &method.params,
            method.result.as_ref(),
            &components.schemas,
        );
    }

    OpenRpcDocument {
        openrpc: "1.3.2",
        info,
        servers,
        methods,
        components,
    }
}

pub(crate) fn standard_errors() -> BTreeMap<String, OpenRpcError> {
    [
        ("ParseError", error_codes::PARSE_ERROR, "Parse error"),
        (
            "InvalidRequest",
            error_codes::INVALID_REQUEST,
            "Invalid Request",
        ),
        (
            "MethodNotFound",
            error_codes::METHOD_NOT_FOUND,
            "Method not found",
        ),
        (
            "InvalidParams",
            error_codes::INVALID_PARAMS,
            "Invalid params",
        ),
        (
            "InternalError",
            error_codes::INTERNAL_ERROR,
            "Internal error",
        ),
    ]
    .into_iter()
    .map(|(name, code, message)| {
        (
            name.into(),
            OpenRpcError {
                code,
                message: message.into(),
            },
        )
    })
    .collect()
}

pub(crate) fn method_from_meta(
    meta: &MethodMeta,
    schemas: &mut BTreeMap<String, Value>,
) -> OpenRpcMethod {
    let (param_structure, params) = match &meta.params_schema {
        Some(root) => {
            merge_definitions(root, schemas);
            let schema = schema_object(root);
            let structure = detect_param_structure(&schema);
            let params = content_params_from_schema(&schema, structure);
            (structure, params)
        }
        None => ("by-name", Vec::new()),
    };
    let result = meta.result_schema.as_ref().map(|root| {
        let schema = take_schema(root, schemas);
        let summary = schema_description(&schema)
            .map(|text| split_summary_description(text).0)
            .unwrap_or(None);
        OpenRpcContent {
            name: "result".into(),
            summary,
            description: None,
            required: false,
            schema,
        }
    });

    OpenRpcMethod {
        name: meta.name.clone(),
        summary: meta.summary.clone(),
        description: meta.description.clone(),
        tags: meta
            .tags
            .iter()
            .cloned()
            .map(|name| OpenRpcTag { name })
            .collect(),
        deprecated: meta.deprecated,
        param_structure,
        params,
        result,
        x_auth: None,
        examples: Vec::new(),
    }
}
