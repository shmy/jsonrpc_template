//! OpenRPC JSON → IR for Swift / Kotlin / Dart codegen.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub swift_type: String,
    pub kotlin_type: String,
    pub dart_type: String,
}

impl Field {
    fn new(name: impl Into<String>, swift: String, kotlin: String, dart: String) -> Self {
        Self {
            name: name.into(),
            swift_type: swift,
            kotlin_type: kotlin,
            dart_type: dart,
        }
    }

    fn named(name: impl Into<String>, ty: &str, required: bool) -> Self {
        let t = opt(ty, !required);
        Self::new(name, t.clone(), t.clone(), t)
    }

    pub fn is_optional(&self) -> bool {
        self.swift_type.ends_with('?')
            || self.kotlin_type.ends_with('?')
            || self.dart_type.ends_with('?')
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub fn_name: String,
    pub requires_auth: bool,
    pub params_type: Option<String>,
    pub result_type: Option<String>,
    pub summary: String,
}

impl Method {
    pub fn auth_lit(&self) -> &'static str {
        if self.requires_auth { "true" } else { "false" }
    }

    pub fn auth_doc_suffix(&self) -> &'static str {
        if self.requires_auth {
            " (Bearer required)"
        } else {
            ""
        }
    }

    pub fn summary_one_line(&self) -> String {
        self.summary.replace('\n', " ")
    }
}

#[derive(Debug, Clone)]
pub struct Ir {
    pub models: Vec<Model>,
    pub methods: Vec<Method>,
    pub server: String,
    pub title: String,
    pub version: String,
}

impl Ir {
    pub fn sorted_models(&self) -> Vec<&Model> {
        let mut models: Vec<_> = self.models.iter().collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }
}

fn opt(ty: &str, optional: bool) -> String {
    if optional {
        format!("{ty}?")
    } else {
        ty.to_string()
    }
}

fn split_ident(name: &str) -> Vec<&str> {
    name.split(['.', '-', '_'])
        .filter(|p| !p.is_empty())
        .collect()
}

fn camel(parts: &[&str], upper: bool) -> String {
    let mut out = String::new();
    for part in parts {
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        out.push(first.to_ascii_uppercase());
        out.extend(chars);
    }
    if out.is_empty() || upper {
        return out;
    }
    let mut chars = out.chars();
    let Some(first) = chars.next() else {
        return out;
    };
    let mut lower = first.to_ascii_lowercase().to_string();
    lower.extend(chars);
    lower
}

pub fn method_fn_name(rpc_name: &str) -> String {
    camel(&split_ident(rpc_name), false)
}

fn type_name_from_rpc(rpc_name: &str, suffix: &str) -> String {
    format!("{}{suffix}", camel(&split_ident(rpc_name), true))
}

fn escape_ident(name: &str, keywords: &[&str], style: IdentEscape) -> String {
    if !keywords.contains(&name) {
        return name.to_string();
    }
    match style {
        IdentEscape::Backtick => format!("`{name}`"),
        IdentEscape::SuffixUnderscore => format!("{name}_"),
    }
}

#[derive(Clone, Copy)]
enum IdentEscape {
    Backtick,
    SuffixUnderscore,
}

const SWIFT_KEYWORDS: &[&str] = &[
    "extension",
    "default",
    "class",
    "struct",
    "enum",
    "protocol",
    "import",
    "operator",
    "return",
    "switch",
    "var",
    "let",
    "func",
    "init",
    "self",
    "Type",
];
const KOTLIN_KEYWORDS: &[&str] = &[
    "object",
    "class",
    "fun",
    "val",
    "var",
    "when",
    "in",
    "is",
    "as",
    "typealias",
];
const DART_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "covariant",
    "default",
    "deferred",
    "do",
    "dynamic",
    "else",
    "enum",
    "export",
    "extends",
    "extension",
    "external",
    "factory",
    "false",
    "final",
    "finally",
    "for",
    "Function",
    "get",
    "hide",
    "if",
    "implements",
    "import",
    "in",
    "interface",
    "is",
    "late",
    "library",
    "mixin",
    "new",
    "null",
    "on",
    "operator",
    "part",
    "required",
    "rethrow",
    "return",
    "set",
    "show",
    "static",
    "super",
    "switch",
    "sync",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "var",
    "void",
    "when",
    "while",
    "with",
    "yield",
];

pub fn swift_ident(name: &str) -> String {
    escape_ident(name, SWIFT_KEYWORDS, IdentEscape::Backtick)
}

pub fn kotlin_ident(name: &str) -> String {
    escape_ident(name, KOTLIN_KEYWORDS, IdentEscape::Backtick)
}

/// Dart 保留字加 `_`；`toJson`/`fromJson` 仍用原始 wire name。
pub fn dart_ident(name: &str) -> String {
    escape_ident(name, DART_KEYWORDS, IdentEscape::SuffixUnderscore)
}

fn ref_name(r: &str) -> String {
    r.rsplit('/').next().unwrap_or(r).to_string()
}

fn schema_nullability(schema: &Value) -> (String, bool) {
    match schema.get("type") {
        Some(Value::Array(arr)) => {
            let nullable = arr.iter().any(|v| v.as_str() == Some("null"));
            let base = arr
                .iter()
                .find_map(|v| v.as_str().filter(|s| *s != "null"))
                .unwrap_or("null");
            (base.to_string(), nullable)
        }
        Some(Value::String(s)) => (s.clone(), false),
        None if schema.get("$ref").is_some() => ("object".into(), false),
        _ => ("object".into(), false),
    }
}

struct LangTypes {
    swift: &'static str,
    kotlin: &'static str,
    dart: &'static str,
}

fn primitives(base: &str) -> LangTypes {
    match base {
        "string" => LangTypes {
            swift: "String",
            kotlin: "String",
            dart: "String",
        },
        "integer" => LangTypes {
            swift: "Int64",
            kotlin: "Long",
            dart: "int",
        },
        "number" => LangTypes {
            swift: "Double",
            kotlin: "Double",
            dart: "double",
        },
        "boolean" => LangTypes {
            swift: "Bool",
            kotlin: "Boolean",
            dart: "bool",
        },
        "null" => LangTypes {
            swift: "JsonRpcNull",
            kotlin: "JsonRpcNull",
            dart: "Null",
        },
        "object" => LangTypes {
            swift: "[String: JsonRpcValue]",
            kotlin: "Map<String, JsonRpcValue>",
            dart: "Map<String, dynamic>",
        },
        "array" => LangTypes {
            swift: "[JsonRpcValue]",
            kotlin: "List<JsonRpcValue>",
            dart: "List<dynamic>",
        },
        _ => LangTypes {
            swift: "JsonRpcValue",
            kotlin: "JsonRpcValue",
            dart: "dynamic",
        },
    }
}

fn lang_types(base: &str, optional: bool) -> (String, String, String) {
    let p = primitives(base);
    (
        opt(p.swift, optional),
        opt(p.kotlin, optional),
        opt(p.dart, optional),
    )
}

fn array_of(
    inner_s: &str,
    inner_k: &str,
    inner_d: &str,
    optional: bool,
) -> (String, String, String) {
    (
        opt(&format!("[{inner_s}]"), optional),
        opt(&format!("List<{inner_k}>"), optional),
        opt(&format!("List<{inner_d}>"), optional),
    )
}

fn field_from_schema(name: &str, schema: &Value, required: bool) -> Field {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        return Field::named(name, &ref_name(r), required);
    }

    let (base, nullable) = schema_nullability(schema);
    let optional = nullable || !required;

    if base == "array" {
        let items = schema
            .get("items")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        let (inner_s, inner_k, inner_d) = if let Some(r) = items.get("$ref").and_then(Value::as_str)
        {
            let inner = ref_name(r);
            (inner.clone(), inner.clone(), inner)
        } else {
            let (ibase, _) = schema_nullability(&items);
            lang_types(&ibase, false)
        };
        let (st, kt, dt) = array_of(&inner_s, &inner_k, &inner_d, optional);
        return Field::new(name, st, kt, dt);
    }

    let (st, kt, dt) = lang_types(&base, optional);
    Field::new(name, st, kt, dt)
}

fn ensure_ref(
    ref_path: &str,
    components: &BTreeMap<String, Value>,
    models: &mut BTreeMap<String, Model>,
) -> Result<(), String> {
    let rn = ref_name(ref_path);
    let Some(target) = components.get(&rn) else {
        return Ok(());
    };
    collect_object_model(&rn, target, components, models)?;
    Ok(())
}

fn collect_object_model(
    name: &str,
    schema: &Value,
    components: &BTreeMap<String, Value>,
    models: &mut BTreeMap<String, Model>,
) -> Result<String, String> {
    let typ = schema.get("type").and_then(Value::as_str);
    let title = schema.get("title").and_then(Value::as_str);
    if typ == Some("null") || title == Some("null") {
        return Ok("null".into());
    }

    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let ref_n = ref_name(r);
        let target = components
            .get(&ref_n)
            .ok_or_else(|| format!("missing component schema: {ref_n}"))?;
        collect_object_model(&ref_n, target, components, models)?;
        return Ok(ref_n);
    }

    if typ == Some("object") || schema.get("properties").is_some() {
        let model_name = title.unwrap_or(name).to_string();
        if models.contains_key(&model_name) {
            return Ok(model_name);
        }
        // Placeholder breaks cycles while fields are resolved.
        models.insert(
            model_name.clone(),
            Model {
                name: model_name.clone(),
                fields: Vec::new(),
            },
        );

        let props = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let required: BTreeSet<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let mut fields = Vec::new();
        for (pname, pschema) in &props {
            fields.push(field_from_schema(pname, pschema, required.contains(pname)));

            if pschema.get("type").and_then(Value::as_str) == Some("object")
                && let Some(nested_title) = pschema.get("title").and_then(Value::as_str)
            {
                collect_object_model(nested_title, pschema, components, models)?;
            }
            if let Some(r) = pschema.get("$ref").and_then(Value::as_str) {
                ensure_ref(r, components, models)?;
            }
            if let Some(r) = pschema
                .get("items")
                .and_then(|items| items.get("$ref"))
                .and_then(Value::as_str)
            {
                ensure_ref(r, components, models)?;
            }
        }

        models.insert(
            model_name.clone(),
            Model {
                name: model_name.clone(),
                fields,
            },
        );
        return Ok(model_name);
    }

    let (base, _) = schema_nullability(schema);
    if base == "null" {
        return Ok("null".into());
    }
    Ok("JsonRpcValue".into())
}

pub fn parse_document(doc: &Value) -> Result<Ir, String> {
    let components: BTreeMap<String, Value> = doc
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let mut models: BTreeMap<String, Model> = BTreeMap::new();
    for (cname, cschema) in &components {
        collect_object_model(cname, cschema, &components, &mut models)?;
    }

    let mut methods = Vec::new();
    let method_list = doc
        .get("methods")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for m in method_list {
        let name = m
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "method missing name".to_string())?
            .to_string();

        let mut fields = Vec::new();
        for p in m
            .get("params")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let pname = p
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{name}: param missing name"))?;
            let schema = p
                .get("schema")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
            fields.push(field_from_schema(pname, &schema, required));
        }

        let params_type = if fields.is_empty() {
            None
        } else {
            let t = type_name_from_rpc(&name, "Params");
            models.insert(
                t.clone(),
                Model {
                    name: t.clone(),
                    fields,
                },
            );
            Some(t)
        };

        let result_schema = m
            .pointer("/result/schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "null"}));
        let result_type_name = collect_object_model(
            &type_name_from_rpc(&name, "Result"),
            &result_schema,
            &components,
            &mut models,
        )?;
        let result_type = (result_type_name != "null").then_some(result_type_name);

        methods.push(Method {
            name: name.clone(),
            fn_name: method_fn_name(&name),
            requires_auth: m.get("x-auth").and_then(Value::as_str) == Some("required"),
            params_type,
            result_type,
            summary: m
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        });
    }

    Ok(Ir {
        models: models.into_values().collect(),
        methods,
        server: doc
            .pointer("/servers/0/url")
            .and_then(Value::as_str)
            .unwrap_or("http://127.0.0.1:9000/jsonrpc")
            .to_string(),
        title: doc
            .pointer("/info/title")
            .and_then(Value::as_str)
            .unwrap_or("JSON-RPC API")
            .to_string(),
        version: doc
            .pointer("/info/version")
            .and_then(Value::as_str)
            .unwrap_or("0.0.0")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn change_password_fn_name() {
        assert_eq!(
            super::method_fn_name("identity.account.change_password"),
            "identityAccountChangePassword"
        );
    }

    #[test]
    fn dart_extension_keyword() {
        assert_eq!(super::dart_ident("extension"), "extension_");
        assert_eq!(super::swift_ident("extension"), "`extension`");
    }
}
