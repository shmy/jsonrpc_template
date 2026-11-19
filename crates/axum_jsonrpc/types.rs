use crate::internal_message::internal_error_message;
use crate::invalid_params::render_invalid_params;
use crate::params::InvalidParamsCause;
use crate::standard_message::{StandardErrorKind, standard_error_message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// Implementation-defined server errors (aligned with jsonrpsee).
    ///
    /// **Reserved for this protocol crate** — app authn / business bands must
    /// not reuse these（模板 authn 用 `-32020..`，identity 用 `-32001..-32009`
    /// 且跳过 `-32007`）。
    pub const OVERSIZED_REQUEST: i32 = -32007;
    pub const TOO_BIG_BATCH_REQUEST: i32 = -32010;
}

/// Max JSON-RPC HTTP body size (1 MiB).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Max calls allowed in a single batch request.
pub const MAX_BATCH_LEN: usize = 128;

/// Default max in-flight calls while processing one batch (`RpcRouter` default).
///
/// Override with [`RpcRouter::batch_concurrency`](crate::RpcRouter::batch_concurrency):
/// `1` = serial, `0` = unbounded.
pub const DEFAULT_BATCH_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Number(serde_json::Number),
    String(String),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn parse_error() -> Self {
        Self::new(
            error_codes::PARSE_ERROR,
            standard_error_message(StandardErrorKind::ParseError),
        )
    }

    pub fn invalid_request() -> Self {
        Self::new(
            error_codes::INVALID_REQUEST,
            standard_error_message(StandardErrorKind::InvalidRequest),
        )
    }

    pub fn method_not_found() -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            standard_error_message(StandardErrorKind::MethodNotFound),
        )
    }

    /// Params omitted shape check / generic invalid params (`InvalidParamsCause::InvalidShape`).
    pub fn invalid_params() -> Self {
        Self::new(
            error_codes::INVALID_PARAMS,
            render_invalid_params(&InvalidParamsCause::InvalidShape),
        )
    }

    pub fn internal_error() -> Self {
        Self::new(error_codes::INTERNAL_ERROR, internal_error_message())
    }

    /// HTTP body exceeded size limit (jsonrpsee `-32007`).
    pub fn oversized_request(limit: usize) -> Self {
        Self {
            code: error_codes::OVERSIZED_REQUEST,
            message: standard_error_message(StandardErrorKind::OversizedRequest),
            data: Some(Value::String(format!("Exceeded max limit of {limit}"))),
        }
    }

    /// Batch length exceeded configured max (jsonrpsee `-32010`).
    pub fn too_big_batch_request(limit: usize) -> Self {
        Self {
            code: error_codes::TOO_BIG_BATCH_REQUEST,
            message: standard_error_message(StandardErrorKind::TooBigBatchRequest),
            data: Some(Value::String(format!("Exceeded max limit of {limit}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    pub id: Id,
}

impl Response {
    pub const VERSION: &'static str = "2.0";

    pub fn success(id: Id, result: Box<RawValue>) -> Self {
        Self {
            jsonrpc: Self::VERSION,
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Id, error: ErrorObject) -> Self {
        Self {
            jsonrpc: Self::VERSION,
            result: None,
            error: Some(error),
            id,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub method: String,
    /// Raw params JSON（Object / Array；省略时为 `None`）。延迟反序列化到 handler 的 `Params<T>`。
    pub params: Option<Box<RawValue>>,
}
