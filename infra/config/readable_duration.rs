//! 可读时长（`"15min"` / `"5s"` / `"30d"`）clap 参数类型。
//!
//! 曾用 `humantime::parse_duration`；该 crate 已停止维护（RUSTSEC-2026-0542），
//! 此处为等价语法子集的本地实现，避免为此保留失修的直接依赖。
//!
//! 语法：`数字 + 单位` 序列，可空格分隔组合（`"1h 30m"`），支持小数（`"1.5h"`）。

use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReadableDuration(Duration);

impl ReadableDuration {
    pub fn duration(&self) -> Duration {
        self.0
    }
}

impl FromStr for ReadableDuration {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_duration(s).map(ReadableDuration)
    }
}

/// 解析 `number unit` 序列并累加为总 [`Duration`]。
fn parse_duration(input: &str) -> Result<Duration, String> {
    let mut total_nanos: u128 = 0;
    let mut rest = input.trim();
    if rest.is_empty() {
        return Err(format!("invalid readable duration '{input}': empty"));
    }
    while !rest.is_empty() {
        let (num_str, after_num) = split_while(rest, |c| c.is_ascii_digit() || c == '.');
        let (unit, after_unit) = split_while(after_num, |c| c.is_ascii_alphabetic());
        let nanos_per_unit = unit_nanos(unit).ok_or_else(|| unknown_unit_err(input, unit))?;
        total_nanos = total_nanos
            .checked_add(number_nanos(num_str, nanos_per_unit, input)?)
            .ok_or_else(|| overflow_err(input))?;
        rest = after_unit.trim_start();
    }
    let nanos = u64::try_from(total_nanos).map_err(|_| overflow_err(input))?;
    Ok(Duration::from_nanos(nanos))
}

/// `数字`（可带小数）× 单位纳秒值，纳秒精度截断。
fn number_nanos(num_str: &str, nanos_per_unit: u64, input: &str) -> Result<u128, String> {
    let (int_part, frac_part) = num_str.split_once('.').unwrap_or((num_str, ""));
    // 拒绝 `"min"`（数字部分为空）与 `"."`（既无整数也无小数）；
    // 前导小数 `".5s"` 仍然合法。
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(format!(
            "invalid readable duration '{input}': expected a number before the unit"
        ));
    }
    // `split_while` 保证此处只有数字与 `.`；空串仅出现在 `.5` 这类前导小数里。
    let int_val: u128 = if int_part.is_empty() {
        0
    } else {
        int_part
            .parse()
            .map_err(|_| format!("invalid readable duration '{input}': bad number `{num_str}`"))?
    };
    let base = int_val * u128::from(nanos_per_unit);
    if frac_part.is_empty() {
        return Ok(base);
    }
    let frac_val: u128 = frac_part
        .parse()
        .map_err(|_| format!("invalid readable duration '{input}': bad number `{num_str}`"))?;
    let frac_len = u32::try_from(frac_part.len()).map_err(|_| overflow_err(input))?;
    let denom = 10u128
        .checked_pow(frac_len)
        .ok_or_else(|| overflow_err(input))?;
    Ok(base + frac_val * u128::from(nanos_per_unit) / denom)
}

fn unit_nanos(unit: &str) -> Option<u64> {
    const NANOS: u64 = 1;
    const SEC: u64 = NANOS * 1_000_000_000;
    Some(match unit {
        "nsec" | "ns" => NANOS,
        "usec" | "us" => NANOS * 1_000,
        "msec" | "ms" => NANOS * 1_000_000,
        "s" | "sec" | "secs" | "second" | "seconds" => SEC,
        "m" | "min" | "mins" | "minute" | "minutes" => SEC * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => SEC * 3_600,
        "d" | "day" | "days" => SEC * 86_400,
        "w" | "week" | "weeks" => SEC * 86_400 * 7,
        "y" | "year" | "years" => SEC * 86_400 * 365,
        _ => return None,
    })
}

fn split_while(s: &str, mut pred: impl FnMut(char) -> bool) -> (&str, &str) {
    let end = s.find(|c: char| !pred(c)).unwrap_or(s.len());
    s.split_at(end)
}

fn unknown_unit_err(input: &str, unit: &str) -> String {
    if unit.is_empty() {
        format!("invalid readable duration '{input}': expected a unit after the number")
    } else {
        format!("invalid readable duration '{input}': unknown unit `{unit}`")
    }
}

fn overflow_err(input: &str) -> String {
    format!("invalid readable duration '{input}': value overflows")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::ReadableDuration;
    use std::time::Duration;

    #[test]
    fn parses_readable_durations() {
        assert_eq!(
            "15min".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(15 * 60)
        );
        assert_eq!(
            "5s".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(5)
        );
        assert_eq!(
            "30d".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(30 * 86_400)
        );
    }

    #[test]
    fn parses_compound_and_fractional() {
        // humantime 兼容：空格/紧连组合、小数。
        assert_eq!(
            "1h 30m".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(5_400)
        );
        assert_eq!(
            "1h30m".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(5_400)
        );
        assert_eq!(
            "1.5h".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_secs(5_400)
        );
        assert_eq!(
            "0.5s".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_millis(500)
        );
        assert_eq!(
            "1ns".parse::<ReadableDuration>().unwrap().duration(),
            Duration::from_nanos(1)
        );
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["abc", "min", "5x", "", "  ", "1.2.3s", "99999999999y"] {
            let err = bad.parse::<ReadableDuration>().unwrap_err();
            // 错误信息必须回显原始输入，便于定位是哪个环境变量写错。
            assert!(
                err.contains(bad),
                "error must echo input `{bad}`: got {err}"
            );
        }
    }

    #[test]
    fn rejects_number_without_unit() {
        let err = "5".parse::<ReadableDuration>().unwrap_err();
        assert!(err.contains("unit"), "got: {err}");
    }
}
