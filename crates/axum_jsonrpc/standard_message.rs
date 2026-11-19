//! Optional hooks so the app shell can localize standard JSON-RPC messages
//! without the protocol core depending on `locale`.

use std::sync::OnceLock;

/// Built-in error messages that are otherwise English literals on [`crate::ErrorObject`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardErrorKind {
    ParseError,
    InvalidRequest,
    MethodNotFound,
    OversizedRequest,
    TooBigBatchRequest,
}

static STANDARD_ERROR_MESSAGE: OnceLock<fn(StandardErrorKind) -> String> = OnceLock::new();

/// Install a message provider for standard protocol errors.
///
/// Call once from the app shell (e.g. `http_server::module`) before serving.
/// Subsequent calls are ignored.
pub fn set_standard_error_message(provider: fn(StandardErrorKind) -> String) {
    let _ = STANDARD_ERROR_MESSAGE.set(provider);
}

pub(crate) fn standard_error_message(kind: StandardErrorKind) -> String {
    STANDARD_ERROR_MESSAGE.get().map_or_else(
        || default_standard_error_message(kind).to_string(),
        |provider| provider(kind),
    )
}

fn default_standard_error_message(kind: StandardErrorKind) -> &'static str {
    match kind {
        StandardErrorKind::ParseError => "Parse error",
        StandardErrorKind::InvalidRequest => "Invalid Request",
        StandardErrorKind::MethodNotFound => "Method not found",
        StandardErrorKind::OversizedRequest => "Request is too big",
        StandardErrorKind::TooBigBatchRequest => "The batch request was too large",
    }
}
