//! Request-scoped correlation id for JSON-RPC over HTTP.
//!
//! Inbound: `traceparent` → [`REQUEST_ID_HEADER`] → optional fallback → mint
//! (aligned with `trace_kit::extract_trace_id`). Fallback strings are run through
//! the same 32-hex normalize as headers (invalid → treat as missing → mint).
//!
//! Outbound: router echoes the resolved id on every `/jsonrpc` response as
//! [`REQUEST_ID_HEADER`]. Apps can pre-resolve once (timeout / body-limit layers)
//! via [`ResolvedRequestId`] in request extensions so mint stays stable.
//! Protocol crate stays free of OpenTelemetry.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::Response as HttpResponse;

/// Response / request correlation header (`x-request-id`).
pub const REQUEST_ID_HEADER: &str = "x-request-id";
/// W3C Trace Context request header.
pub const TRACEPARENT_HEADER: &str = "traceparent";

/// Pre-resolved correlation id stored in request extensions by an outer layer
/// (e.g. timeout middleware) so `/jsonrpc` and transport errors share one mint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequestId(pub String);

/// Resolve a 32-hex trace id: headers → normalized `fallback` → mint.
pub fn resolve_trace_id_with_fallback(
    headers: &HeaderMap,
    fallback: impl FnOnce() -> Option<String>,
) -> String {
    extract_trace_id(headers)
        .or_else(|| fallback().and_then(|id| normalize_trace_id(&id)))
        .unwrap_or_else(mint_trace_id)
}

/// Resolve from headers only, else mint (tests / callers without a span fallback).
pub fn resolve_trace_id(headers: &HeaderMap) -> String {
    resolve_trace_id_with_fallback(headers, || None)
}

/// Set / overwrite [`REQUEST_ID_HEADER`] on an HTTP response (no-op if value is illegal).
pub fn set_request_id_header(response: &mut HttpResponse, trace_id: &str) {
    if let Ok(value) = HeaderValue::from_str(trace_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn parse_traceparent_returns_pure_trace_id() {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let parsed = parse_traceparent_trace_id(traceparent);
        assert_eq!(parsed.as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
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
    fn invalid_fallback_is_ignored_then_mint() {
        let id = resolve_trace_id_with_fallback(&HeaderMap::new(), || {
            Some("not-a-valid-trace-id\n".into())
        });
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn mint_when_headers_and_fallback_missing() {
        let id = resolve_trace_id(&HeaderMap::new());
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
