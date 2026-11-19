//! Invalid-params 文案渲染（进程级钩子，由 `module()` 注册）。

use axum_jsonrpc::{DeserializeErrorKind, InvalidParamsCause};

pub(crate) fn render_invalid_params_cause(cause: &InvalidParamsCause) -> String {
    match cause {
        InvalidParamsCause::Validation { code, location } => {
            render_validation_params(code, location)
        }
        InvalidParamsCause::Deserialize { path, kind, .. } => {
            render_deserialize_params(path, *kind)
        }
        InvalidParamsCause::InvalidShape => locale::translate_current("invalid_params_shape"),
        InvalidParamsCause::Other { .. } => locale::translate_current("invalid_params"),
    }
}

/// Client-facing path: schema-ish identifiers only (no raw unknown-field echo of arbitrary keys).
///
/// Normalizes validify-style `/a/b` into dotted `a.b`.
fn client_safe_path(path: &str) -> Option<String> {
    const MAX_LEN: usize = 64;
    let normalized = path.replace('/', ".");
    let normalized = normalized.trim_start_matches('.');
    if normalized.is_empty() || normalized.len() > MAX_LEN || normalized.contains("..") {
        return None;
    }
    normalized
        .bytes()
        .all(|b| {
            matches!(
                b,
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'[' | b']'
            )
        })
        .then(|| normalized.to_owned())
}

fn render_validation_params(code: &str, location: &str) -> String {
    let msg = locale::translate_current(code);
    match client_safe_path(location) {
        Some(path) => format!("{path}: {msg}"),
        None => msg,
    }
}

fn render_deserialize_params(path: &str, kind: DeserializeErrorKind) -> String {
    match kind {
        // Request keys are attacker-controlled; never echo them.
        DeserializeErrorKind::UnknownField => {
            locale::translate_current("invalid_params_unknown_field")
        }
        DeserializeErrorKind::MissingField => {
            render_with_safe_path(path, "invalid_params_missing_field", "invalid_params")
        }
        // `path` is the enum *field* (schema); variant string stays only in server logs.
        DeserializeErrorKind::UnknownVariant => {
            render_with_safe_path(path, "invalid_params_unknown_variant", "invalid_params")
        }
        DeserializeErrorKind::DuplicateField => {
            render_with_safe_path(path, "invalid_params_duplicate_field", "invalid_params")
        }
        DeserializeErrorKind::InvalidType => render_with_safe_path(
            path,
            "invalid_params_invalid_type",
            "invalid_params_invalid_type_root",
        ),
        DeserializeErrorKind::InvalidValue => render_with_safe_path(
            path,
            "invalid_params_invalid_value",
            "invalid_params_invalid_value_root",
        ),
        DeserializeErrorKind::InvalidLength => render_with_safe_path(
            path,
            "invalid_params_invalid_length",
            "invalid_params_invalid_length_root",
        ),
        DeserializeErrorKind::TrailingData => locale::translate_current("invalid_params_trailing"),
        DeserializeErrorKind::Other => locale::translate_current("invalid_params"),
    }
}

fn render_with_safe_path(path: &str, keyed: &str, root_fallback: &str) -> String {
    match client_safe_path(path) {
        Some(path) => locale::translate_current_with_args(keyed, &[("path", path.as_str())]),
        None => locale::translate_current(root_fallback),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod render_tests {
    use super::{
        client_safe_path, render_deserialize_params, render_invalid_params_cause,
        render_validation_params,
    };
    use axum_jsonrpc::{DeserializeErrorKind, InvalidParamsCause};

    #[test]
    fn client_safe_path_rejects_odd_keys() {
        assert_eq!(client_safe_path("password").as_deref(), Some("password"));
        assert_eq!(client_safe_path("/password").as_deref(), Some("password"));
        assert_eq!(client_safe_path("/user/name").as_deref(), Some("user.name"));
        assert_eq!(client_safe_path("user.name").as_deref(), Some("user.name"));
        assert_eq!(client_safe_path("items[0]").as_deref(), Some("items[0]"));
        assert_eq!(client_safe_path("__proto__").as_deref(), Some("__proto__"));
        assert_eq!(client_safe_path("a b"), None);
        assert_eq!(client_safe_path("x\ny"), None);
        assert_eq!(client_safe_path("a..b"), None);
        assert_eq!(client_safe_path(&"a".repeat(65)), None);
    }

    #[test]
    fn unknown_field_does_not_echo_path() {
        let msg = render_deserialize_params("evil<script>", DeserializeErrorKind::UnknownField);
        assert_eq!(msg, "Unknown field");
        assert!(!msg.contains("evil"));
    }

    #[test]
    fn missing_field_includes_safe_path() {
        let msg = render_deserialize_params("password", DeserializeErrorKind::MissingField);
        assert_eq!(msg, "Missing required field `password`");
    }

    #[test]
    fn unknown_variant_uses_field_path_not_variant_name() {
        let msg = render_deserialize_params("mode", DeserializeErrorKind::UnknownVariant);
        assert_eq!(msg, "Unknown variant at `mode`");
    }

    #[test]
    fn duplicate_field_includes_safe_path() {
        let msg = render_deserialize_params("password", DeserializeErrorKind::DuplicateField);
        assert_eq!(msg, "Duplicate field `password`");
    }

    #[test]
    fn invalid_type_root_keeps_type_semantics() {
        let msg = render_deserialize_params("", DeserializeErrorKind::InvalidType);
        assert_eq!(msg, "Invalid params type");
    }

    #[test]
    fn invalid_length_root() {
        let msg = render_deserialize_params("", DeserializeErrorKind::InvalidLength);
        assert_eq!(msg, "Invalid params length");
    }

    #[test]
    fn trailing_data_message() {
        let msg = render_deserialize_params("", DeserializeErrorKind::TrailingData);
        assert_eq!(msg, "Unexpected extra params data");
    }

    #[test]
    fn invalid_shape_message() {
        let msg = render_invalid_params_cause(&InvalidParamsCause::InvalidShape);
        assert_eq!(msg, "Params must be a JSON array or object");
    }

    #[test]
    fn validation_prefixes_safe_location() {
        let msg = render_validation_params("password_invalid_length", "password");
        assert_eq!(msg, "password: Password must be 8-64 characters long");
        let nested = render_validation_params("password_invalid_length", "/user/password");
        assert_eq!(
            nested,
            "user.password: Password must be 8-64 characters long"
        );
    }
}
