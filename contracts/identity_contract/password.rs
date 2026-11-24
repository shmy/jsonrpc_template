use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use validify::traits::Length;

/// 与下方 JSON Schema / validify 一致：8..=64 个字符，非控制字符（Unicode Cc）。
/// NIST 800-63B 风格——只限长度，不做字符集组合限制（避免拒绝含符号的强密码）。
pub const PASSWORD_MIN_LEN: usize = 8;
pub const PASSWORD_MAX_LEN: usize = 64;

/// 明文密码入参。形态校验在 request 的 `validify`（→ `-32602` + locale），
/// 不在 `Deserialize` 里拒绝；不提供 `Deref`，避免明文被当作普通字符串拼进日志/上下文。
///
/// `Serialize` 仅供 validify 把 `actual` 写进内部 `ValidationError.params`
/// （RPC 响应不用这些 params）；序列化为脱敏占位，避免明文进错误对象。
#[derive(Clone, Deserialize)]
pub struct Password(String);

impl Serialize for Password {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("<redacted>")
    }
}

impl Password {
    /// 显式构造（同样强校验）。RPC 路径由 `ValidatedParams` + validify 保证形态。
    pub fn try_new(password: &str) -> Result<Self, &'static str> {
        validate(password)?;
        Ok(Self(password.to_owned()))
    }

    /// 跳过校验直接包装（仅测试与已校验来源使用）。
    #[cfg(test)]
    pub fn new_unchecked(password: String) -> Self {
        Self(password)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 校验规则与 `JsonSchema` / validify 保持一致。
fn validate(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    if !(PASSWORD_MIN_LEN..=PASSWORD_MAX_LEN).contains(&len) {
        return Err("password must be 8-64 characters long");
    }
    // 控制字符拒绝入库/日志层面的隐患（如 Postgres TEXT 不接受 NUL 字节）。
    if password.chars().any(char::is_control) {
        return Err("password must not contain control characters");
    }
    Ok(())
}

impl AsRef<str> for Password {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Length for Password {
    fn length(&self) -> usize {
        self.0.chars().count()
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Password").field(&"<Redacted>").finish()
    }
}

impl JsonSchema for Password {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Password".into()
    }

    fn schema_id() -> Cow<'static, str> {
        "identity_contract::Password".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": PASSWORD_MIN_LEN,
            "maxLength": PASSWORD_MAX_LEN,
            // 与运行时 `char::is_control`（Unicode Cc）对称，须锚定（JSON Schema pattern 默认部分匹配）
            "pattern": "^[^\\u0000-\\u001F\\u007F-\\u009F]*$",
            "examples": ["password123"],
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{PASSWORD_MAX_LEN, PASSWORD_MIN_LEN, Password};
    use serde::Deserialize;
    use serde_json::{Value, from_value, json};
    use validify::Validate;

    #[derive(Debug, Deserialize, Validate)]
    struct Sample {
        #[validate(
            length(min = 8, max = 64, code = "password_invalid_length"),
            non_control_char(code = "password_control_chars")
        )]
        password: Password,
    }

    #[test]
    fn accepts_valid_password() {
        let p: Password = from_value(json!("password123")).unwrap();
        assert_eq!(p.as_str(), "password123");
        assert!(Password::try_new("password123").is_ok());
        assert!(
            Sample {
                password: p.clone()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn accepts_symbols_and_spaces() {
        // NIST 800-63B：不做字符集组合限制，含符号/空格的强密码必须可用。
        for good in ["p@ssw0rd!", "correct horse", "中文密码示例八位数"] {
            assert!(Password::try_new(good).is_ok(), "should accept `{good}`");
            let sample = Sample {
                password: from_value(json!(good)).unwrap(),
            };
            assert!(sample.validate().is_ok(), "should accept `{good}`");
        }
    }

    #[test]
    fn deserialize_allows_short_validify_rejects() {
        // Deserialize 不再拦；validify / try_new 负责形态。
        let short: Password = from_value(json!("short1")).unwrap();
        assert!(Password::try_new(short.as_str()).is_err());
        assert!(
            Sample { password: short }.validate().is_err(),
            "validify should reject short password"
        );
        for bad in [&"a".repeat(65), "pass\u{0}word\n\tlong"] {
            let p: Password = from_value(Value::String(bad.to_string())).unwrap();
            assert!(Password::try_new(bad).is_err(), "should reject `{bad:?}`");
            assert!(
                Sample { password: p }.validate().is_err(),
                "validify should reject `{bad:?}`"
            );
        }
    }

    #[test]
    fn accepts_length_boundaries() {
        // 闭区间边界：恰好 8 / 恰好 64 必须通过，两侧各退一步必须拒绝。
        assert!(Password::try_new(&"a".repeat(PASSWORD_MIN_LEN)).is_ok());
        assert!(Password::try_new(&"a".repeat(PASSWORD_MAX_LEN)).is_ok());
        assert!(Password::try_new(&"a".repeat(PASSWORD_MIN_LEN - 1)).is_err());
        assert!(Password::try_new(&"a".repeat(PASSWORD_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn debug_redacts_plaintext() {
        let p = Password::new_unchecked("supersecret".into());
        assert!(!format!("{p:?}").contains("supersecret"));
    }
}
