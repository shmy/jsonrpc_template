//! One-pass wire parsing: extract `method` / `id` / `jsonrpc` without building a
//! `serde_json::Value` tree. `params` are captured as raw JSON text
//! ([`RawValue`]) and only deserialized later inside the handler's `Params<T>`.

use std::borrow::Cow;
use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::Number;
use serde_json::value::RawValue;

use super::types::{Call, ErrorObject, Id, Response};

#[derive(Debug)]
pub(crate) enum ParseCall {
    Invalid {
        id: Id,
        error: ErrorObject,
    },
    Notification(Call),
    Request {
        call: Call,
        id: Id,
    },
    /// Spec §4.1: notification envelope (valid `jsonrpc`+`method`, omitted `id`)
    /// with illegal `params` shape — must not reply and must not invoke.
    Dropped,
}

/// Top-level JSON-RPC payload (single call, batch of raw calls, or valid-JSON scalar).
pub(crate) enum TopCall<'a> {
    Single(WireCall),
    Batch(Vec<&'a RawValue>),
    /// 合法 JSON 但根是标量（数字/字符串/bool/null）→ 单个 Invalid Request。
    ScalarRoot,
}

/// Parse the full request body. `Err` means syntactically invalid JSON (→ -32700).
///
/// Note: batch items are captured with per-item lazy validation — `RawValue`
/// capture checks JSON grammar only, numeric range is checked later in
/// `parse_call`. So an unrepresentable number (e.g. `1e400`) in one item only
/// fails *that* item (`-32600`), not the whole batch — consistent with
/// jsonrpsee's per-item semantics. (The old whole-body `Value` parse failed
/// the entire batch with `-32700`.)
pub(crate) fn parse_top(bytes: &[u8]) -> Result<TopCall<'_>, ()> {
    match first_non_whitespace(bytes) {
        Some(b'[') => serde_json::from_slice::<Vec<&RawValue>>(bytes)
            .map(TopCall::Batch)
            .map_err(|_| ()),
        Some(b'{') => serde_json::from_slice::<WireCall>(bytes)
            .map(TopCall::Single)
            .map_err(|_| ()),
        // 标量 / 其它：先验语法——合法则按 Invalid Request 处理，非法则 Parse error。
        _ => match serde_json::from_slice::<IgnoredAny>(bytes) {
            Ok(_) => Ok(TopCall::ScalarRoot),
            Err(_) => Err(()),
        },
    }
}

/// Parse one batch item (already valid JSON). Non-object → Invalid Request.
pub(crate) fn parse_call(raw: &str) -> ParseCall {
    match serde_json::from_str::<WireCall>(raw) {
        Ok(call) => validate_call(call),
        Err(_) => ParseCall::Invalid {
            id: Id::Null,
            error: ErrorObject::invalid_request(),
        },
    }
}

/// Semantic validation, mirroring JSON-RPC 2.0 spec §4 / §4.2.
pub(crate) fn validate_call(call: WireCall) -> ParseCall {
    let WireCall {
        id,
        jsonrpc,
        method,
        params,
    } = call;

    let id = match id {
        None => None,
        Some(IdField::Number(n)) => Some(Id::Number(n)),
        Some(IdField::String(s)) => Some(Id::String(s)),
        Some(IdField::Null) => Some(Id::Null),
        Some(IdField::Other(_)) => {
            return ParseCall::Invalid {
                id: Id::Null,
                error: ErrorObject::invalid_request(),
            };
        }
    };
    let id_or_null = || id.clone().unwrap_or(Id::Null);

    if !matches!(jsonrpc, Some(StrField::Str(ref v)) if v == Response::VERSION) {
        return ParseCall::Invalid {
            id: id_or_null(),
            error: ErrorObject::invalid_request(),
        };
    }

    let Some(StrField::Str(method)) = method else {
        return ParseCall::Invalid {
            id: id_or_null(),
            error: ErrorObject::invalid_request(),
        };
    };

    // Spec §4.2: if present, params MUST be Array (by-position) or Object (by-name).
    // Wrong shape is Invalid params (-32602) for Requests; for Notifications (§4.1)
    // the Server MUST NOT reply (and we skip invoke so unit handlers get no side effect).
    let params = match params {
        None => None,
        Some(raw) => match raw.get().trim_start().as_bytes().first() {
            Some(b'[') | Some(b'{') => Some(raw),
            _ => {
                return match id {
                    None => ParseCall::Dropped,
                    Some(id) => ParseCall::Invalid {
                        id,
                        error: ErrorObject::invalid_params(),
                    },
                };
            }
        },
    };

    let call = Call { method, params };
    match id {
        Some(id) => ParseCall::Request { call, id },
        None => ParseCall::Notification(call),
    }
}

/// Raw fields of a single JSON-RPC call object; semantics checked in [`validate_call`].
///
/// `params` is captured as an *owned* `Box<RawValue>` (one substring copy):
/// borrowing the body would introduce a lifetime parameter that infects
/// `Call` → `BoxMethodFuture` (requires `'static`). Batch items themselves
/// are still borrowed zero-copy at the `Vec<&RawValue>` layer.
pub(crate) struct WireCall {
    id: Option<IdField>,
    jsonrpc: Option<StrField>,
    method: Option<StrField>,
    params: Option<Box<RawValue>>,
}

impl WireCall {
    /// Method name if (and only if) it is a JSON string (for root span naming).
    pub(crate) fn method_name(&self) -> Option<&str> {
        match self.method.as_ref()? {
            StrField::Str(s) => Some(s),
            StrField::Other(_) => None,
        }
    }
}

/// id member: Number / String / null are valid; anything else is a shape error.
#[derive(Deserialize)]
#[serde(untagged)]
enum IdField {
    Number(Number),
    String(String),
    Null,
    Other(IgnoredAny),
}

/// A string-typed member; anything else is a shape error.
#[derive(Deserialize)]
#[serde(untagged)]
enum StrField {
    Str(String),
    Other(IgnoredAny),
}

impl<'de> Deserialize<'de> for WireCall {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WireCallVisitor;

        impl<'de> Visitor<'de> for WireCallVisitor {
            type Value = WireCall;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON-RPC call object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<WireCall, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut id = None;
                let mut jsonrpc = None;
                let mut method = None;
                let mut params = None;
                while let Some(key) = map.next_key::<Cow<'de, str>>()? {
                    match key.as_ref() {
                        "id" => id = Some(map.next_value::<IdField>()?),
                        "jsonrpc" => jsonrpc = Some(map.next_value::<StrField>()?),
                        "method" => method = Some(map.next_value::<StrField>()?),
                        // RawValue captures any valid JSON (incl. null); shape is
                        // validated later so omitted vs `null` stay distinguishable.
                        "params" => params = Some(map.next_value::<Box<RawValue>>()?),
                        _ => {
                            let _ = map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(WireCall {
                    id,
                    jsonrpc,
                    method,
                    params,
                })
            }
        }

        deserializer.deserialize_map(WireCallVisitor)
    }
}

/// serde_json's own leading-whitespace set (space, \t, \n, \r).
fn first_non_whitespace(bytes: &[u8]) -> Option<u8> {
    bytes
        .iter()
        .copied()
        .find(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}
