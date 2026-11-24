//! 手机号形态校验与出站脱敏。
//!
//! 放契约层：与 `Password` 同类，是 schema 形态规则而非业务语义。

use validify::ValidationError;

/// 中国大陆手机号固定 11 位。
pub const CN_MOBILE_DIGITS: usize = 11;

/// 手机号形态：中国大陆 11 位手机号（`1` 开头、第二位 `3-9`，即 `^1[3-9]\d{9}$`）。
///
/// **严格拒绝**一切变体（`+86` 前缀 / 空白 / 连字符 / 字母），而非静默归一：
/// - 变体会拆分按手机号的限流配额窗（`identity:login:rate:{phone}`）
/// - 变体会绕过 phone 唯一键，形成「同号」多行脏数据
///
/// 校验在 `ValidatedParams` 提取阶段执行（`-32602` + locale），
/// 因此 handler 拿到的 phone 已是规范形态，无需再归一。
pub fn validate_phone_format(phone: &str) -> Result<(), ValidationError> {
    if is_cn_mobile(phone) {
        Ok(())
    } else {
        Err(ValidationError::new_field("phone_invalid_format"))
    }
}

/// 中国大陆手机号形态断言（`^1[3-9]\d{9}$`，无 `+86` 前缀）。
#[must_use]
pub fn is_cn_mobile(phone: &str) -> bool {
    let bytes = phone.as_bytes();
    bytes.len() == CN_MOBILE_DIGITS
        && bytes.first() == Some(&b'1')
        && bytes.get(1).is_some_and(|b| (b'3'..=b'9').contains(b))
        && bytes.iter().skip(2).all(|b| b.is_ascii_digit())
}

/// RPC 出站脱敏：保留前 3 / 后 4，中间 `****`（例 `138****8888`）。
///
/// 仅 [`is_cn_mobile`] 形态脱敏；其它（含非法第二位的 11 位脏数据）整段 `***`，
/// 避免半截明文泄漏。由 [`crate::AccountView`] 的 o2o 映射调用；库内仍存明文。
#[must_use]
pub fn mask_for_wire(phone: &str) -> String {
    if is_cn_mobile(phone)
        && let (Some(prefix), Some(suffix)) = (phone.get(..3), phone.get(7..))
    {
        format!("{prefix}****{suffix}")
    } else {
        "***".to_owned()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn accepts_mainland_mobiles() {
        for good in ["13888888888", "15912345678", "19900000000", "18600000000"] {
            assert!(validate_phone_format(good).is_ok(), "accept `{good}`");
        }
    }

    #[test]
    fn rejects_plus_prefix_and_separator_variants() {
        // `+86` / 空白 / 连字符变体不得进入限流 key 或唯一键
        for bad in [
            "+8613888888888",
            " 13888888888",
            "13888888888 ",
            "138 8888 8888",
            "138-8888-8888",
        ] {
            assert!(validate_phone_format(bad).is_err(), "rejected `{bad}`");
        }
    }

    #[test]
    fn rejects_wrong_prefix_and_length() {
        for bad in [
            "23888888888",  // 非 1 开头
            "12888888888",  // 第二位不在 3-9
            "10888888888",  // 同上（0）
            "1234567890",   // 10 位
            "138888888888", // 12 位
            "",
        ] {
            assert!(validate_phone_format(bad).is_err(), "rejected `{bad}`");
        }
    }

    #[test]
    fn rejects_letters_and_fullwidth_digits() {
        for bad in ["138a888888", "１３８８８８８８８８８"] {
            assert!(validate_phone_format(bad).is_err(), "rejected `{bad}`");
        }
    }

    #[test]
    fn masks_cn_mobile_for_wire() {
        assert_eq!(mask_for_wire("13888888888"), "138****8888");
        assert_eq!(mask_for_wire("15912345678"), "159****5678");
    }

    #[test]
    fn masks_non_canonical_as_opaque() {
        assert_eq!(mask_for_wire(""), "***");
        assert_eq!(mask_for_wire("138"), "***");
        assert_eq!(mask_for_wire("+8613888888888"), "***");
        // 11 位但非大陆号段：不得按「前 3 / 后 4」半脱敏
        assert_eq!(mask_for_wire("12888888888"), "***");
        assert_eq!(mask_for_wire("23888888888"), "***");
    }
}
