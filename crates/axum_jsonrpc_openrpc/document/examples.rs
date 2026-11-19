use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::types::{OpenRpcContent, OpenRpcExample, OpenRpcExampleParam, OpenRpcExampleResult};

pub(crate) fn synthesize_examples(
    method_name: &str,
    params: &[OpenRpcContent],
    result: Option<&OpenRpcContent>,
    components: &BTreeMap<String, Value>,
) -> Vec<OpenRpcExample> {
    let example_params: Vec<OpenRpcExampleParam> = params
        .iter()
        .filter_map(|param| {
            example_value_from_schema(&param.schema, components).map(|value| OpenRpcExampleParam {
                name: param.name.clone(),
                value,
            })
        })
        .collect();

    let example_result = result.and_then(|content| {
        example_value_from_schema(&content.schema, components).map(|value| OpenRpcExampleResult {
            name: content.name.clone(),
            value,
        })
    });

    if example_params.is_empty() && example_result.is_none() {
        return Vec::new();
    }

    vec![OpenRpcExample {
        name: format!("{method_name}Example"),
        params: example_params,
        result: example_result,
    }]
}

pub(crate) fn example_value_from_schema(
    schema: &Value,
    components: &BTreeMap<String, Value>,
) -> Option<Value> {
    let schema = resolve_schema_ref(schema, components);

    // Nullable wire shapes（`Option<T>`）合成 `null`：即使内型自带 examples
    // （如 `Id` 的 `"1234567890"`），字段级示例也应展示「缺省」形态。
    if is_nullable(schema) {
        return Some(Value::Null);
    }

    if let Some(value) = first_schema_example(schema) {
        return Some(value);
    }

    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        let mut object = Map::new();
        let mut any = false;
        for (name, prop) in props {
            if let Some(value) = example_value_from_schema(prop, components) {
                object.insert(name.clone(), value);
                any = true;
            }
        }
        return any.then_some(Value::Object(object));
    }

    if schema.get("type").and_then(Value::as_str) == Some("array")
        && let Some(items) = schema.get("items")
        && let Some(value) = example_value_from_schema(items, components)
    {
        return Some(Value::Array(vec![value]));
    }

    // serde `#[serde(default)]` 在 schema 上反射出的 `default`：优先于类型占位，
    // 让示例可以原样发送（如 `limit` 的 20 而非占位 0——0 会被 clamp 成 1）。
    if let Some(default) = schema.get("default") {
        return Some(default.clone());
    }

    // Primitive fields without explicit examples get type-based placeholders so
    // synthesized examples carry the full wire shape (otherwise a result schema
    // with ten fields renders as a lone `{"id": "…"}` pairing).
    match schema_type_name(schema) {
        Some("string") => Some(Value::String(String::new())),
        Some("integer") | Some("number") => Some(Value::from(0)),
        Some("boolean") => Some(Value::Bool(false)),
        _ => None,
    }
}

/// 可空判定：`type` 为/含 `"null"`（unit `()` 或 `Option<内联简单型>` 被 schemars
/// 合并成 type 数组）、`anyOf` / `oneOf` 含 `null` 变体（`Option<$ref>`）、`const: null`。
pub(crate) fn is_nullable(schema: &Value) -> bool {
    let type_is_null = |value: &Value| match value.get("type") {
        Some(Value::String(kind)) => kind == "null",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "null"),
        _ => false,
    };
    if type_is_null(schema) || schema.get("const") == Some(&Value::Null) {
        return true;
    }
    ["anyOf", "oneOf"].iter().any(|key| {
        schema
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|entries| entries.iter().any(type_is_null))
    })
}

/// Non-null `type` name: `"string"`, or the first non-`null` entry of `["string","null"]`.
pub(crate) fn schema_type_name(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(kind)) => Some(kind.as_str()),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null"),
        _ => None,
    }
}

pub(crate) fn resolve_schema_ref<'a>(
    schema: &'a Value,
    components: &'a BTreeMap<String, Value>,
) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    reference
        .strip_prefix("#/components/schemas/")
        .and_then(|name| components.get(name))
        .unwrap_or(schema)
}

pub(crate) fn first_schema_example(schema: &Value) -> Option<Value> {
    if let Some(examples) = schema.get("examples").and_then(Value::as_array)
        && let Some(value) = examples.first()
    {
        return Some(value.clone());
    }
    schema.get("example").cloned()
}
