use std::convert::Infallible;
use std::error::Error;
use std::fmt;

use axum::extract::FromRequestParts;
use axum::extract::rejection::ExtensionRejection;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use tracing::debug;
use validify::Validate;

use super::invalid_params::render_invalid_params;
use super::types::ErrorObject;
use serde_json::Deserializer;

/// Raw JSON-RPC `params` for the current call (request extensions).
///
/// `None` means the member was omitted; `Some` is raw JSON text (guaranteed
/// Object / Array by `parse::validate_call`).
#[derive(Clone, Debug)]
pub(crate) struct RpcParams(pub Option<Box<RawValue>>);

/// Classified `serde` failure (parsed from the error message; serde itself is unstructured).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeserializeErrorKind {
    MissingField,
    UnknownField,
    /// Enum discriminant from the request was not recognized.
    ///
    /// `path` is the enum *field* location (schema); the variant string itself is
    /// request-controlled and must not be echoed to clients.
    UnknownVariant,
    /// Same struct field appeared more than once (`duplicate field \`name\``).
    DuplicateField,
    InvalidType,
    InvalidValue,
    InvalidLength,
    /// Extra tokens after a successful partial decode (e.g. tuple too long).
    TrailingData,
    Other,
}

/// Why params extraction failed (fed to [`crate::set_invalid_params_renderer`]).
#[derive(Debug, Clone)]
pub enum InvalidParamsCause {
    /// `serde` / `serde_path_to_error` failure while decoding `params`.
    ///
    /// `path` is normalized: missing/unknown field names are appended to the parent
    /// path (`serde_path_to_error` reports those at the parent, often `"."` at root).
    Deserialize {
        path: String,
        kind: DeserializeErrorKind,
        message: String,
    },
    /// `validify::Validate` rejected the decoded value (first error only).
    Validation { code: String, location: String },
    /// JSON-RPC `params` present but not Array/Object (wire shape; see `parse`).
    InvalidShape,
    /// Missing extractor context or other non-serde failure.
    Other { detail: String },
}

/// Extractor that deserializes JSON-RPC `params` into `T` (no field validation).
///
/// - **By-name**: `params` is an Object → use a struct (`Params<MyArgs>`).
/// - **By-position**: `params` is an Array → use a tuple (`Params<(i32, i32)>`)
///   or any type that deserializes from a JSON array.
/// - Omitted `params` is treated as JSON `null` during deserialization.
///
/// Uses [`serde_path_to_error`] so deserialize failures carry a field path.
/// For `validify` rules after decode, use [`ValidatedParams`].
#[derive(Debug, Clone)]
pub struct Params<T>(pub T);

/// Like [`Params`], then runs [`Validate::validate`] and maps the first error
/// to `-32602` via [`InvalidParamsCause::Validation`].
#[derive(Debug, Clone)]
pub struct ValidatedParams<T>(pub T);

#[derive(Debug)]
pub struct ParamsRejection {
    cause: InvalidParamsCause,
}

impl ParamsRejection {
    #[must_use]
    pub fn cause(&self) -> &InvalidParamsCause {
        &self.cause
    }

    fn deserialize(raw_path: impl Into<String>, message: impl Into<String>) -> Self {
        let message = message.into();
        let (path, kind) = classify_deserialize(&raw_path.into(), &message);
        Self {
            cause: InvalidParamsCause::Deserialize {
                path,
                kind,
                message,
            },
        }
    }

    fn validation(code: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            cause: InvalidParamsCause::Validation {
                code: code.into(),
                location: location.into(),
            },
        }
    }

    fn other(detail: impl Into<String>) -> Self {
        Self {
            cause: InvalidParamsCause::Other {
                detail: detail.into(),
            },
        }
    }
}

impl fmt::Display for ParamsRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            InvalidParamsCause::Deserialize { path, message, .. } => {
                if path.is_empty() {
                    write!(f, "{message}")
                } else {
                    write!(f, "{path}: {message}")
                }
            }
            InvalidParamsCause::Validation { code, location } => {
                if location.is_empty() {
                    write!(f, "{code}")
                } else {
                    write!(f, "{location}: {code}")
                }
            }
            InvalidParamsCause::InvalidShape => f.write_str("params must be array or object"),
            InvalidParamsCause::Other { detail } => f.write_str(detail),
        }
    }
}

impl Error for ParamsRejection {}

impl IntoResponse for ParamsRejection {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, self.to_string()).into_response()
    }
}

/// Rejection → JSON-RPC [`ErrorObject`] (used by `#[rpc]` / `RpcHandler` bounds).
pub trait IntoExtractError {
    fn into_extract_error(self) -> ErrorObject;
}

impl IntoExtractError for ParamsRejection {
    fn into_extract_error(self) -> ErrorObject {
        debug!(cause = %self, "jsonrpc invalid params");
        ErrorObject::new(
            super::types::error_codes::INVALID_PARAMS,
            render_invalid_params(&self.cause),
        )
    }
}

impl IntoExtractError for Infallible {
    fn into_extract_error(self) -> ErrorObject {
        match self {}
    }
}

impl IntoExtractError for ExtensionRejection {
    fn into_extract_error(self) -> ErrorObject {
        debug!(detail = %self, "jsonrpc extension extract failed");
        ErrorObject::internal_error()
    }
}

fn take_rpc_params(parts: &mut Parts) -> Result<Option<Box<RawValue>>, ParamsRejection> {
    let Some(RpcParams(params)) = parts.extensions.remove::<RpcParams>() else {
        return Err(ParamsRejection::other("missing RPC params context"));
    };
    Ok(params)
}

fn deserialize_params<T: DeserializeOwned>(raw: &str) -> Result<T, ParamsRejection> {
    let mut de = Deserializer::from_str(raw);
    match serde_path_to_error::deserialize(&mut de) {
        Ok(parsed) => Ok(parsed),
        Err(err) => Err(ParamsRejection::deserialize(
            err.path().to_string(),
            err.into_inner().to_string(),
        )),
    }
}

/// Turn `serde_path_to_error` path + serde message into a field-oriented location + kind.
///
/// Empty / `"."` means the root `params` value.
fn classify_deserialize(raw_path: &str, message: &str) -> (String, DeserializeErrorKind) {
    let parent = normalize_parent_path(raw_path);

    if let Some(field) = backtick_field_after(message, "missing field ") {
        return (
            path_for_named_field(parent.as_deref(), field),
            DeserializeErrorKind::MissingField,
        );
    }
    if let Some(field) = backtick_field_after(message, "unknown field ") {
        // `serde_path_to_error` often already pushed the unknown key onto the path.
        return (
            path_for_named_field(parent.as_deref(), field),
            DeserializeErrorKind::UnknownField,
        );
    }
    if backtick_field_after(message, "unknown variant ").is_some() {
        // Keep the enum *field* path only; do not append the request variant name.
        return (
            parent.unwrap_or_default(),
            DeserializeErrorKind::UnknownVariant,
        );
    }
    if let Some(field) = backtick_field_after(message, "duplicate field ") {
        return (
            path_for_named_field(parent.as_deref(), field),
            DeserializeErrorKind::DuplicateField,
        );
    }
    if message.starts_with("invalid type:") {
        return (
            parent.unwrap_or_default(),
            DeserializeErrorKind::InvalidType,
        );
    }
    if message.starts_with("invalid value:") {
        return (
            parent.unwrap_or_default(),
            DeserializeErrorKind::InvalidValue,
        );
    }
    if message.starts_with("invalid length") {
        return (
            parent.unwrap_or_default(),
            DeserializeErrorKind::InvalidLength,
        );
    }
    if message.starts_with("trailing characters") {
        return (
            parent.unwrap_or_default(),
            DeserializeErrorKind::TrailingData,
        );
    }
    (parent.unwrap_or_default(), DeserializeErrorKind::Other)
}

fn normalize_parent_path(raw_path: &str) -> Option<String> {
    if raw_path.is_empty() || raw_path == "." {
        None
    } else {
        Some(raw_path.to_owned())
    }
}

fn path_for_named_field(parent: Option<&str>, field: &str) -> String {
    match parent {
        Some(p) if p == field || path_ends_with_field(p, field) => p.to_owned(),
        Some(p) if !p.is_empty() => format!("{p}.{field}"),
        _ => field.to_owned(),
    }
}

fn path_ends_with_field(path: &str, field: &str) -> bool {
    path.strip_suffix(field)
        .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('.'))
}

/// `missing field \`password\`` / `unknown field \`foo\`, expected ...` → field name.
fn backtick_field_after<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = message.strip_prefix(prefix)?.strip_prefix('`')?;
    let end = rest.find('`')?;
    let field = rest.get(..end)?;
    if field.is_empty() { None } else { Some(field) }
}

fn first_validation_rejection(errors: validify::ValidationErrors) -> ParamsRejection {
    match errors.errors().first() {
        Some(err) => ParamsRejection::validation(err.code(), err.location().to_owned()),
        None => ParamsRejection::other("validation failed"),
    }
}

impl<S, T> FromRequestParts<S> for Params<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ParamsRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params = take_rpc_params(parts)?;
        let raw = params.as_deref().map(|r| r.get()).unwrap_or("null");
        deserialize_params(raw).map(Params)
    }
}

impl<S, T> FromRequestParts<S> for ValidatedParams<T>
where
    T: DeserializeOwned + Validate + Send,
    S: Send + Sync,
{
    type Rejection = ParamsRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params = take_rpc_params(parts)?;
        let raw = params.as_deref().map(|r| r.get()).unwrap_or("null");
        let parsed: T = deserialize_params(raw)?;
        parsed
            .validate()
            .map(|()| ValidatedParams(parsed))
            .map_err(first_validation_rejection)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod classify_tests {
    use serde::Deserialize;

    use super::{
        DeserializeErrorKind, InvalidParamsCause, classify_deserialize, deserialize_params,
    };

    #[test]
    fn missing_field_at_root() {
        let (path, kind) = classify_deserialize(".", "missing field `password` at line 4 column 3");
        assert_eq!(path, "password");
        assert_eq!(kind, DeserializeErrorKind::MissingField);
    }

    #[test]
    fn missing_field_nested() {
        let (path, kind) = classify_deserialize("user", "missing field `name`");
        assert_eq!(path, "user.name");
        assert_eq!(kind, DeserializeErrorKind::MissingField);
    }

    #[test]
    fn unknown_field_at_root() {
        let (path, kind) =
            classify_deserialize(".", "unknown field `foo`, expected `phone` or `password`");
        assert_eq!(path, "foo");
        assert_eq!(kind, DeserializeErrorKind::UnknownField);
    }

    #[test]
    fn unknown_field_path_already_has_key() {
        let (path, kind) = classify_deserialize("foo", "unknown field `foo`, expected `password`");
        assert_eq!(path, "foo");
        assert_eq!(kind, DeserializeErrorKind::UnknownField);
    }

    #[test]
    fn invalid_type_on_field() {
        let (path, kind) =
            classify_deserialize("password", "invalid type: integer `1`, expected a string");
        assert_eq!(path, "password");
        assert_eq!(kind, DeserializeErrorKind::InvalidType);
    }

    #[test]
    fn invalid_type_at_root() {
        let (path, kind) =
            classify_deserialize(".", "invalid type: sequence, expected struct LoginArgs");
        assert!(path.is_empty());
        assert_eq!(kind, DeserializeErrorKind::InvalidType);
    }

    #[test]
    fn invalid_length_classified() {
        let (path, kind) =
            classify_deserialize(".", "invalid length 1, expected a tuple of size 2");
        assert!(path.is_empty());
        assert_eq!(kind, DeserializeErrorKind::InvalidLength);
    }

    #[test]
    fn invalid_value_classified() {
        let (path, kind) = classify_deserialize(".", "invalid value: integer `-1`, expected u8");
        assert!(path.is_empty());
        assert_eq!(kind, DeserializeErrorKind::InvalidValue);
    }

    #[test]
    fn trailing_characters_classified() {
        let (path, kind) = classify_deserialize(".", "trailing characters at line 1 column 6");
        assert!(path.is_empty());
        assert_eq!(kind, DeserializeErrorKind::TrailingData);
    }

    #[test]
    fn unknown_variant_keeps_field_path_only() {
        let (path, kind) =
            classify_deserialize("mode", "unknown variant `Turbo`, expected `Fast` or `Slow`");
        assert_eq!(path, "mode");
        assert_eq!(kind, DeserializeErrorKind::UnknownVariant);
    }

    #[test]
    fn duplicate_field_at_root() {
        let (path, kind) = classify_deserialize(".", "duplicate field `password`");
        assert_eq!(path, "password");
        assert_eq!(kind, DeserializeErrorKind::DuplicateField);
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Login {
        password: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct StrictLogin {
        password: String,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    enum Mode {
        Fast,
        Slow,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct WithEnum {
        mode: Mode,
    }

    fn assert_deserialize(
        err: &super::ParamsRejection,
        want_path: &str,
        want_kind: DeserializeErrorKind,
    ) {
        match err.cause() {
            InvalidParamsCause::Deserialize { path, kind, .. } => {
                assert_eq!(path, want_path);
                assert_eq!(*kind, want_kind);
            }
            other => panic!("expected Deserialize cause, got {other:?}"),
        }
    }

    #[test]
    fn real_serde_missing_field() {
        let err = deserialize_params::<Login>(r#"{"phone":"+1"}"#).expect_err("password required");
        assert_deserialize(&err, "password", DeserializeErrorKind::MissingField);
    }

    #[test]
    fn real_serde_invalid_type() {
        let err = deserialize_params::<Login>(r#"{"password":1}"#).expect_err("type error");
        assert_deserialize(&err, "password", DeserializeErrorKind::InvalidType);
    }

    #[test]
    fn real_serde_unknown_field() {
        let err = deserialize_params::<StrictLogin>(r#"{"password":"x","evil":true}"#)
            .expect_err("unknown field");
        assert_deserialize(&err, "evil", DeserializeErrorKind::UnknownField);
    }

    #[test]
    fn real_serde_invalid_length_tuple() {
        let err = deserialize_params::<(i32, i32)>("[1]").expect_err("tuple length");
        assert_deserialize(&err, "", DeserializeErrorKind::InvalidLength);
    }

    #[test]
    fn real_serde_trailing_tuple() {
        let err = deserialize_params::<(i32, i32)>("[1,2,3]").expect_err("trailing");
        assert_deserialize(&err, "", DeserializeErrorKind::TrailingData);
    }

    #[test]
    fn real_serde_invalid_value_overflow() {
        let err = deserialize_params::<u8>("-1").expect_err("overflow");
        assert_deserialize(&err, "", DeserializeErrorKind::InvalidValue);
    }

    #[test]
    fn real_serde_unknown_variant() {
        let err =
            deserialize_params::<WithEnum>(r#"{"mode":"Turbo"}"#).expect_err("unknown variant");
        assert_deserialize(&err, "mode", DeserializeErrorKind::UnknownVariant);
    }

    #[test]
    fn real_serde_duplicate_field() {
        let err = deserialize_params::<Login>(r#"{"password":"a","password":"b"}"#)
            .expect_err("duplicate field");
        assert_deserialize(&err, "password", DeserializeErrorKind::DuplicateField);
    }
}
