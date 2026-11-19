//! Request-scoped trace id for JSON-RPC errors (`error.data.trace_id`).
//!
//! Header rules align with `trace_kit::extract_trace_id` (`traceparent` then
//! `x-request-id`). Callers may supply a fallback (e.g. current OTel span id)
//! before minting. Protocol crate stays free of OpenTelemetry.

use axum::http::HeaderMap;
use serde_json::{Map, Value};

use super::types::ErrorObject;

const REQUEST_ID_HEADER: &str = "x-request-id";
const TRACEPARENT_HEADER: &str = "traceparent";

/// Resolve a 32-hex trace id: headers → `fallback` → mint.
pub fn resolve_trace_id_with_fallback(
    headers: &HeaderMap,
    fallback: impl FnOnce() -> Option<String>,
) -> String {
    extract_trace_id(headers)
        .or_else(fallback)
        .unwrap_or_else(mint_trace_id)
}

/// Resolve from headers only, else mint (tests / callers without a span fallback).
pub fn resolve_trace_id(headers: &HeaderMap) -> String {
    resolve_trace_id_with_fallback(headers, || None)
}

fn extract_trace_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(TRACEPARENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_traceparent_trace_id)
        .or_else(|| {
            headers
                .get(REQUEST_ID_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(normalize_trace_id)
        })
}

fn parse_traceparent_trace_id(traceparent: &str) -> Option<String> {
    let mut parts = traceparent.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if !is_hex_len(version, 2) {
        return None;
    }
    if !is_hex_len(flags, 2) {
        return None;
    }
    if !is_hex_len(parent_id, 16) || parent_id.as_bytes().iter().all(|b| *b == b'0') {
        return None;
    }
    normalize_trace_id(trace_id)
}

fn normalize_trace_id(value: &str) -> Option<String> {
    let trace_id = value.trim().to_ascii_lowercase();
    if trace_id.len() != 32 {
        return None;
    }
    if !trace_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if trace_id.as_bytes().iter().all(|b| *b == b'0') {
        return None;
    }
    Some(trace_id)
}

fn is_hex_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn mint_trace_id() -> String {
    let mut bytes = [0u8; 16];
    // RNG failure or all-zero → force a non-zero nibble (normalize rejects all-zero).
    if getrandom::fill(&mut bytes).is_err() || bytes.iter().all(|b| *b == 0) {
        bytes[15] = 1;
    }
    bytes.iter().fold(String::with_capacity(32), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Attach `trace_id` into `error.data` without dropping existing payload.
pub fn attach_trace_id(error: &mut ErrorObject, trace_id: &str) {
    let mut obj = match error.data.take() {
        None => Map::new(),
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("detail".into(), other);
            map
        }
    };
    obj.insert("trace_id".into(), Value::String(trace_id.to_owned()));
    error.data = Some(Value::Object(obj));
}

pub(crate) fn with_trace_id(mut error: ErrorObject, trace_id: &str) -> ErrorObject {
    attach_trace_id(&mut error, trace_id);
    error
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    #[test]
    fn parse_traceparent_returns_pure_trace_id() {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let parsed = parse_traceparent_trace_id(traceparent);
        assert_eq!(parsed.as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
    }

    #[test]
    fn attach_trace_id_wraps_non_object_data() {
        let mut err = ErrorObject {
            code: -32007,
            message: "Request is too big".into(),
            data: Some(Value::String("Exceeded max limit of 1".into())),
        };
        attach_trace_id(&mut err, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(
            err.data,
            Some(json!({
                "detail": "Exceeded max limit of 1",
                "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736"
            }))
        );
    }

    #[test]
    fn resolve_prefers_traceparent_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("ignored"));
        assert_eq!(
            resolve_trace_id(&headers),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[test]
    fn resolve_uses_x_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert(
            REQUEST_ID_HEADER,
            HeaderValue::from_static("4bf92f3577b34da6a3ce929d0e0e4736"),
        );
        assert_eq!(
            resolve_trace_id(&headers),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[test]
    fn resolve_prefers_fallback_over_mint() {
        let id = resolve_trace_id_with_fallback(&HeaderMap::new(), || {
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into())
        });
        assert_eq!(id, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }

    #[test]
    fn mint_when_headers_and_fallback_missing() {
        let id = resolve_trace_id(&HeaderMap::new());
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
