use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Info block for an OpenRPC document (SpringDoc `api-docs` / service metadata).
#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcInfo {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for OpenRpcInfo {
    fn default() -> Self {
        Self {
            title: "JSON-RPC API".into(),
            version: "0.1.0".into(),
            description: None,
        }
    }
}

/// Server entry for an OpenRPC document (`servers` array).
#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcServer {
    pub name: String,
    pub url: String,
}

/// OpenRPC 1.3 document.
#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcDocument {
    pub openrpc: &'static str,
    pub info: OpenRpcInfo,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<OpenRpcServer>,
    pub methods: Vec<OpenRpcMethod>,
    #[serde(skip_serializing_if = "OpenRpcComponents::is_empty")]
    pub components: OpenRpcComponents,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct OpenRpcComponents {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub errors: BTreeMap<String, OpenRpcError>,
}

impl OpenRpcComponents {
    fn is_empty(&self) -> bool {
        self.schemas.is_empty() && self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcMethod {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<OpenRpcTag>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    #[serde(rename = "paramStructure")]
    pub param_structure: &'static str,
    pub params: Vec<OpenRpcContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OpenRpcContent>,
    /// Specification extension: methods gated by `before_each(require_authed_account)`
    /// carry `x-auth: "required"` so doc UIs can flag login-required methods.
    #[serde(rename = "x-auth", skip_serializing_if = "Option::is_none")]
    pub x_auth: Option<&'static str>,
    /// Method example pairings for OpenRPC playground (from schemars field/type examples).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<OpenRpcExample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcExample {
    pub name: String,
    pub params: Vec<OpenRpcExampleParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OpenRpcExampleResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcExampleParam {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcExampleResult {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcTag {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRpcContent {
    pub name: String,
    /// Short Content Descriptor summary (OpenRPC; playground "Summary" column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Verbose Content Descriptor description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    pub schema: Value,
}
