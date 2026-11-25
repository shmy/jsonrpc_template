//! Optional hook so the composition root can localize `-32603` without the
//! protocol core depending on `locale`.

use std::sync::OnceLock;

static INTERNAL_ERROR_MESSAGE: OnceLock<fn() -> String> = OnceLock::new();

/// Install a message provider for [`crate::ErrorObject::internal_error`].
///
/// Call once from the composition root (e.g. `http_server::module`) before serving.
/// Subsequent calls are ignored.
pub fn set_internal_error_message(provider: fn() -> String) {
    let _ = INTERNAL_ERROR_MESSAGE.set(provider);
}

pub(crate) fn internal_error_message() -> String {
    INTERNAL_ERROR_MESSAGE
        .get()
        .map_or_else(|| "Internal error".to_string(), |provider| provider())
}
