//! login / refresh 共享的令牌响应 DTO。
//!
//! 两个 method 复用同一出参形态，放 `shared/` 避免method 文件横向耦合
//! （目录约定：跨 method 逻辑一律进 `shared/`）。

use crate::shared::token_ops::TokenPair;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
pub struct TokenResponse {
    /// 访问令牌
    pub access_token: String,
    /// 刷新令牌
    pub refresh_token: String,
    /// 令牌类型
    pub token_type: String,
    /// 过期时间
    pub expires_in: u64,
}

impl TokenResponse {
    pub fn bearer(pair: TokenPair) -> Self {
        Self {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: pair.expires_in,
        }
    }
}
