use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use validify::traits::Length;

/// 正常 UUID 为 36 字符；上限防超大 token 滥用。空串 / 超长由 validify → `-32602`。
pub const REFRESH_TOKEN_MAX_LEN: usize = 128;

/// Opaque refresh token 入参。Deserialize 只 trim；长度交给 request 上的 validify。
///
/// `Serialize` 供 validify 内部 `actual` 参数使用；脱敏，不把 token 明文写入错误 params。
#[derive(Clone)]
pub struct RefreshToken(String);

impl Serialize for RefreshToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("<redacted>")
    }
}

impl RefreshToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RefreshToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Length for RefreshToken {
    fn length(&self) -> usize {
        self.0.chars().count()
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RefreshToken").field(&"<Redacted>").finish()
    }
}

impl Deref for RefreshToken {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RefreshToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self(s.trim().to_string()))
    }
}

impl JsonSchema for RefreshToken {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "RefreshToken".into()
    }

    fn schema_id() -> Cow<'static, str> {
        "identity_contract::RefreshToken".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": REFRESH_TOKEN_MAX_LEN,
            "examples": ["550e8400-e29b-41d4-a716-446655440000"],
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use serde_json::{Value, from_value, json};
    use validify::Validate;

    #[derive(Debug, Deserialize, Validate)]
    struct Sample {
        #[validate(length(min = 1, max = 128, code = "refresh_token_invalid_length"))]
        refresh_token: RefreshToken,
    }

    #[test]
    fn accepts_uuid() {
        let token: RefreshToken =
            from_value(json!("550e8400-e29b-41d4-a716-446655440000")).unwrap();
        assert_eq!(token.as_str(), "550e8400-e29b-41d4-a716-446655440000");
        assert!(
            Sample {
                refresh_token: token
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn deserialize_trims_validify_rejects_empty_and_too_long() {
        let empty: RefreshToken = from_value(json!("   ")).unwrap();
        assert_eq!(empty.as_str(), "");
        assert!(
            Sample {
                refresh_token: empty
            }
            .validate()
            .is_err()
        );

        let long: RefreshToken =
            from_value(Value::String("a".repeat(REFRESH_TOKEN_MAX_LEN + 1))).unwrap();
        assert!(
            Sample {
                refresh_token: long
            }
            .validate()
            .is_err()
        );
    }
}
