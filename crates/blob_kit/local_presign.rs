//! Fs 本地预签名：签发 / 校验指向本服务的 `PUT` URL（HMAC-SHA256）。
//!
//! 与 COS `presign_write` 对齐客户端流程：拿到 `upload_url` 后直接 PUT 字节流。
//! 签名串：`PUT\n{path}\n{exp}\n{content_type}`。

use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use rootcause::Result;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::BlobError;

type HmacSha256 = Hmac<Sha256>;

/// 签发本地预签名 PUT URL。
///
/// `put_base` 例：`http://localhost:8080/files/presigned`（不含 query）。
pub fn mint_put_url(
    put_base: &str,
    secret: &[u8],
    path: &str,
    content_type: &str,
    expire: Duration,
) -> Result<String> {
    if put_base.is_empty() || secret.is_empty() {
        return Err(BlobError::PresignUnsupported.into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let exp = now.saturating_add(expire.as_secs().max(1));
    let sig = sign_hex(secret, path, exp, content_type)?;
    let mut url = String::with_capacity(put_base.len() + path.len() + 96);
    url.push_str(put_base.trim_end_matches('/'));
    url.push('?');
    let _ = write!(
        url,
        "path={}&exp={exp}&ct={}&sig={sig}",
        query_escape(path),
        query_escape(content_type),
    );
    Ok(url)
}

/// 校验 query 参数（MAC 比较为常量时间）。
pub fn verify_put_query(
    secret: &[u8],
    path: &str,
    exp: u64,
    content_type: &str,
    sig_hex: &str,
) -> Result<()> {
    if secret.is_empty() {
        return Err(BlobError::PresignUnsupported.into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if exp < now {
        return Err(BlobError::PresignUnsupported.into());
    }
    let expected = sign_raw(secret, path, exp, content_type)?;
    let Some(actual) = decode_hex32(sig_hex) else {
        return Err(BlobError::PresignUnsupported.into());
    };
    if !bool::from(expected.ct_eq(&actual)) {
        return Err(BlobError::PresignUnsupported.into());
    }
    Ok(())
}

fn sign_raw(secret: &[u8], path: &str, exp: u64, content_type: &str) -> Result<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| BlobError::PresignUnsupported)?;
    mac.update(b"PUT\n");
    mac.update(path.as_bytes());
    mac.update(b"\n");
    mac.update(exp.to_string().as_bytes());
    mac.update(b"\n");
    mac.update(content_type.as_bytes());
    let out = mac.finalize().into_bytes();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&out);
    Ok(raw)
}

fn sign_hex(secret: &[u8], path: &str, exp: u64, content_type: &str) -> Result<String> {
    Ok(to_hex(sign_raw(secret, path, exp, content_type)?))
}

fn to_hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let mut bytes = out.iter_mut();
    for chunk in s.as_bytes().chunks(2) {
        let hi = from_hex_nibble(*chunk.first()?)?;
        let lo = from_hex_nibble(*chunk.get(1)?)?;
        *bytes.next()? = (hi << 4) | lo;
    }
    Some(out)
}

fn from_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::str::from_utf8;

    #[test]
    fn mint_and_verify_roundtrip() {
        let secret = b"test-presign-secret-at-least-32b!!";
        let url = mint_put_url(
            "http://localhost:8080/files/presigned",
            secret,
            "staging/2026/01/a.webp",
            "image/webp",
            Duration::from_secs(600),
        )
        .unwrap();
        assert!(url.starts_with("http://localhost:8080/files/presigned?"));
        let q = url.split_once('?').unwrap().1;
        let mut path = None;
        let mut exp = None;
        let mut ct = None;
        let mut sig = None;
        for part in q.split('&') {
            let (k, v) = part.split_once('=').unwrap();
            match k {
                "path" => path = Some(query_unescape(v)),
                "exp" => exp = Some(v.parse::<u64>().unwrap()),
                "ct" => ct = Some(query_unescape(v)),
                "sig" => sig = Some(v.to_string()),
                _ => {}
            }
        }
        verify_put_query(
            secret,
            path.as_deref().unwrap(),
            exp.unwrap(),
            ct.as_deref().unwrap(),
            sig.as_deref().unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn reject_tampered_sig() {
        let secret = b"test-presign-secret-at-least-32b!!";
        let bad = "0".repeat(64);
        assert!(
            verify_put_query(
                secret,
                "staging/2026/01/a.webp",
                u64::MAX,
                "image/webp",
                &bad
            )
            .is_err()
        );
    }

    #[test]
    fn reject_expired_even_with_valid_mac() {
        let secret = b"test-presign-secret-at-least-32b!!";
        let path = "staging/2026/01/a.webp";
        let ct = "image/webp";
        let past_exp = 1_u64; // 1970-01-01
        let sig = sign_hex(secret, path, past_exp, ct).unwrap();
        assert!(verify_put_query(secret, path, past_exp, ct, &sig).is_err());
    }

    fn query_unescape(s: &str) -> String {
        let mut out = Vec::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                let h = u8::from_str_radix(from_utf8(&b[i + 1..i + 3]).unwrap(), 16).unwrap();
                out.push(h);
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }
}
