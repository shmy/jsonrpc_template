//! 线格式 string、内存 u64 的账号 ID（避免 JS 大整数精度问题）。

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

/// JSON 里是十进制字符串；DB / toasty 侧当 [`u64`] 用（经 [`toasty::Embed`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, toasty::Embed)]
pub struct Id(u64);

impl Default for Id {
    fn default() -> Self {
        Self(tsid::create_tsid().number())
    }
}

impl Id {
    #[must_use]
    pub const fn new_unchecked(id: u64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for Id {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Id> for u64 {
    fn from(value: Id) -> Self {
        value.0
    }
}

impl Deref for Id {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <Cow<'_, str>>::deserialize(deserializer)?;
        s.parse().map(Self).map_err(D::Error::custom)
    }
}

impl JsonSchema for Id {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Id".into()
    }

    fn schema_id() -> Cow<'static, str> {
        "identity_contract::Id".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9]+$",
            "examples": ["1234567890"],
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::Id;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn roundtrip_as_string() {
        let id = Id(12_345_678_901_234_567_890);
        let value = to_value(id).unwrap();
        assert_eq!(value, json!("12345678901234567890"));
        let parsed: Id = from_value(value).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn rejects_number_and_garbage() {
        assert!(from_value::<Id>(json!(123)).is_err());
        assert!(from_value::<Id>(json!("abc")).is_err());
    }
}
