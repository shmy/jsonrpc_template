use std::collections::BTreeMap;

use serde_json::Value;

use super::types::OpenRpcContent;

pub(crate) fn detect_param_structure(schema: &Value) -> &'static str {
    if schema.get("properties").is_some() {
        return "by-name";
    }
    if schema.get("prefixItems").is_some() || schema.get("items").is_some() {
        return "by-position";
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => "by-position",
        Some("object") => "by-name",
        _ => "by-name",
    }
}

pub(crate) fn content_params_from_schema(schema: &Value, structure: &str) -> Vec<OpenRpcContent> {
    if structure == "by-position" {
        return positional_params(schema);
    }

    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return vec![content_from_schema("params".into(), schema, true)];
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut params: Vec<OpenRpcContent> = props
        .iter()
        .map(|(name, prop)| {
            content_from_schema(name.clone(), prop, required.contains(&name.as_str()))
        })
        .collect();
    params.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    params
}

pub(crate) fn positional_params(schema: &Value) -> Vec<OpenRpcContent> {
    if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
        return prefix
            .iter()
            .enumerate()
            .map(|(idx, item)| content_from_schema(format!("arg{idx}"), item, true))
            .collect();
    }

    if let Some(items) = schema.get("items") {
        if let Some(arr) = items.as_array() {
            return arr
                .iter()
                .enumerate()
                .map(|(idx, item)| content_from_schema(format!("arg{idx}"), item, true))
                .collect();
        }
        return vec![content_from_schema("arg0".into(), items, true)];
    }

    vec![content_from_schema("params".into(), schema, true)]
}

pub(crate) fn content_from_schema(name: String, schema: &Value, required: bool) -> OpenRpcContent {
    let schema = schema.clone();
    // `summary` on Content Descriptor; leave `description` inside schema (schemars `///`).
    let summary = schema_description(&schema)
        .map(|text| split_summary_description(text).0)
        .unwrap_or(None);
    OpenRpcContent {
        name,
        summary,
        description: None,
        required,
        schema,
    }
}

pub(crate) fn schema_description(schema: &Value) -> Option<&str> {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn split_summary_description(text: &str) -> (Option<String>, Option<String>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    let mut parts = trimmed.splitn(2, "\n\n");
    let summary = parts.next().map(str::trim).filter(|s| !s.is_empty());
    let description = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (summary.map(str::to_string), description)
}

pub(crate) fn take_schema(root: &Value, schemas: &mut BTreeMap<String, Value>) -> Value {
    merge_definitions(root, schemas);
    schema_object(root)
}

pub(crate) fn schema_object(root: &Value) -> Value {
    // schemars 0.8 RootSchema shape.
    if let Some(schema) = root.get("schema") {
        return schema.clone();
    }
    // schemars 1.x: root object is the schema; defs live alongside properties.
    match root {
        Value::Object(map) => {
            let mut cleaned = map.clone();
            cleaned.remove("$defs");
            cleaned.remove("definitions");
            cleaned.remove("$schema");
            Value::Object(cleaned)
        }
        other => other.clone(),
    }
}

pub(crate) fn merge_definitions(root: &Value, schemas: &mut BTreeMap<String, Value>) {
    for key in ["definitions", "$defs"] {
        let Some(defs) = root.get(key).and_then(Value::as_object) else {
            continue;
        };
        for (name, schema) in defs {
            schemas
                .entry(name.clone())
                .or_insert_with(|| schema.clone());
        }
    }
}

pub(crate) fn rewrite_schema_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some(name) = reference.strip_prefix("#/definitions/") {
                    *reference = format!("#/components/schemas/{name}");
                } else if let Some(name) = reference.strip_prefix("#/$defs/") {
                    *reference = format!("#/components/schemas/{name}");
                }
            }
            for child in map.values_mut() {
                rewrite_schema_refs(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                rewrite_schema_refs(child);
            }
        }
        _ => {}
    }
}
