use authn_kit::AuthnError;
use axum::http::{HeaderMap, header::AUTHORIZATION};

/// 从 `Authorization: Bearer <token>` 提取 token（借用 header，零拷贝）。
pub fn access_token_from_headers(headers: &HeaderMap) -> Result<&str, AuthnError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthnError::AccessTokenMissing)?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or(AuthnError::AccessTokenMissing)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(AuthnError::AccessTokenMissing);
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AuthnError::AccessTokenMissing);
    }
    Ok(token)
}

/// 有 `Authorization` 头才尝试解析；无头返回 `None`（软鉴权用）。
pub fn access_token_from_headers_optional(headers: &HeaderMap) -> Option<Result<&str, AuthnError>> {
    if !headers.contains_key(AUTHORIZATION) {
        return None;
    }
    Some(access_token_from_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn accepts_capitalized_bearer() {
        let headers = headers("Bearer abc.def");
        let token = access_token_from_headers(&headers);
        assert!(matches!(token, Ok(value) if value == "abc.def"));
    }

    #[test]
    fn accepts_lowercase_scheme() {
        let headers = headers("bearer abc.def");
        let token = access_token_from_headers(&headers);
        assert!(matches!(token, Ok(value) if value == "abc.def"));
    }

    #[test]
    fn rejects_missing_or_empty_token() {
        let empty = HeaderMap::new();
        let blank = headers("Bearer ");
        let basic = headers("Basic abc");
        assert!(access_token_from_headers(&empty).is_err());
        assert!(access_token_from_headers(&blank).is_err());
        assert!(access_token_from_headers(&basic).is_err());
    }

    #[test]
    fn optional_skips_missing_header() {
        let empty = HeaderMap::new();
        let bearer = headers("Bearer x");
        assert!(access_token_from_headers_optional(&empty).is_none());
        assert!(access_token_from_headers_optional(&bearer).is_some());
    }
}
