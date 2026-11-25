//! Optional hook so the composition root can localize `-32602` without the
//! protocol core depending on `locale`.

use std::sync::OnceLock;

use super::params::InvalidParamsCause;

static INVALID_PARAMS_RENDERER: OnceLock<fn(&InvalidParamsCause) -> String> = OnceLock::new();

/// Install a message renderer for invalid-params rejections.
///
/// Call once from the composition root (e.g. `http_server::module`) before serving.
/// Subsequent calls are ignored.
pub fn set_invalid_params_renderer(provider: fn(&InvalidParamsCause) -> String) {
    let _ = INVALID_PARAMS_RENDERER.set(provider);
}

pub(crate) fn render_invalid_params(cause: &InvalidParamsCause) -> String {
    INVALID_PARAMS_RENDERER.get().map_or_else(
        || default_invalid_params_message(cause),
        |provider| provider(cause),
    )
}

fn default_invalid_params_message(_cause: &InvalidParamsCause) -> String {
    // Keep ASCII default so protocol-core tests stay locale-free.
    "Invalid params".to_string()
}
