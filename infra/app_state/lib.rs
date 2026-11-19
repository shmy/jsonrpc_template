//! Shared application state for HTTP / JSON-RPC handlers.
//!
//! Keep this crate light so features can depend on it without pulling
//! `http_server` / routing tags. Grow with pools/caches; expose slices via
//! `axum::extract::FromRef`.
//!
//! # State ownership
//!
//! Features build `RpcRouter::<AppState>::new()` / `Router::<AppState>::new()`
//! (tables only). `persistence` provides process [`Db`](toasty::Db);
//! `cache` provides [`Cache`](cache_kit::Cache); `blob` provides
//! [`Blob`](blob_kit::Blob); `http_server` wraps them with
//! [`TokenHelper`](jwt_kit::TokenHelper) into [`AppState`], merges route groups,
//! then attaches state.

use std::fmt::{self, Debug};
use std::time::Duration;

use axum::extract::FromRef;
use blob_kit::Blob;
use cache_kit::Cache;
use config::TrustedProxies;
use http_extractors::HasTrustedProxies;
use jwt_kit::TokenHelper;
use rootcause::Result;
use toasty::Db;

/// Redis sliding-window quota（`per_second` + `burst` → 窗长 / 上限）。
///
/// Method 内限流：[`Self::allow`]；固定窗直接 [`Cache::allow_sliding_window`]。
/// 全站 IP 限流放网关，应用不挂中间件。
#[derive(Clone, Copy, Debug)]
pub struct SlidingWindowRate {
    pub per_second: u32,
    pub burst: u32,
}

impl SlidingWindowRate {
    /// `(window, max_hits)` for [`Cache::allow_sliding_window`].
    ///
    /// `burst` 为窗内硬上限，且至少抬到 `per_second`（否则高 rate + 低 burst
    /// 会静默达不到标定速率）。0 降级为 1（Cli 已校验 >0）。
    pub fn window_params(self) -> (Duration, usize) {
        let per_second = self.per_second.max(1);
        let burst = self.burst.max(per_second);
        let window_secs = u64::from(burst.div_ceil(per_second).max(1));
        (Duration::from_secs(window_secs), burst as usize)
    }

    /// `true` = under quota；缓存错误上抛，由 caller 映域错误。
    pub async fn allow(self, cache: &Cache, key: &str) -> Result<bool> {
        let (window, max_hits) = self.window_params();
        cache.allow_sliding_window(key, window, max_hits).await
    }
}

/// login/refresh 等 auth 面 Redis IP 配额（`RATE_LIMIT_AUTH_*`）。
///
/// 放在 `app_state`：handler 经 [`FromRef`] 提取；若放 `http_auth` 会与
/// `http_auth → app_state` 成环。
#[derive(Clone, Copy, Debug)]
pub struct AuthRateLimit {
    pub sliding_window: SlidingWindowRate,
}

impl AuthRateLimit {
    /// 见 [`SlidingWindowRate::allow`]。
    pub async fn allow(self, cache: &Cache, key: &str) -> Result<bool> {
        self.sliding_window.allow(cache, key).await
    }
}

/// login 按手机号固定窗配额（`RATE_LIMIT_LOGIN_PHONE_*`；成功也计数，防单号撞库）。
///
/// 放在 `app_state`：与 [`AuthRateLimit`] 同理，handler 经 [`FromRef`] 提取，
/// 避免把配置读取散进 feature。
#[derive(Clone, Copy, Debug)]
pub struct LoginPhoneRate {
    pub window: Duration,
    pub max_attempts: usize,
}

impl LoginPhoneRate {
    /// `true` = under quota；缓存错误上抛，由 caller 映域错误。
    pub async fn allow(self, cache: &Cache, key: &str) -> Result<bool> {
        cache
            .allow_sliding_window(key, self.window, self.max_attempts)
            .await
    }
}

/// Process-wide state cloned into each RPC/HTTP handler invocation.
#[derive(Clone)]
pub struct AppState {
    db: Db,
    cache: Cache,
    blob: Blob,
    token_helper: TokenHelper,
    trusted_proxies: TrustedProxies,
    auth_rate_limit: AuthRateLimit,
    login_phone_rate: LoginPhoneRate,
}

impl Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState")
            .field("db", &"<Db>")
            .field("cache", &"<Cache>")
            .field("blob", &"<Blob>")
            .field("token_helper", &self.token_helper)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("auth_rate_limit", &self.auth_rate_limit)
            .field("login_phone_rate", &self.login_phone_rate)
            .finish()
    }
}

impl AppState {
    #[inline]
    pub fn new(
        db: Db,
        cache: Cache,
        blob: Blob,
        token_helper: TokenHelper,
        trusted_proxies: TrustedProxies,
        auth_rate_limit: AuthRateLimit,
        login_phone_rate: LoginPhoneRate,
    ) -> Self {
        Self {
            db,
            cache,
            blob,
            token_helper,
            trusted_proxies,
            auth_rate_limit,
            login_phone_rate,
        }
    }

    #[inline]
    pub fn db(&self) -> &Db {
        &self.db
    }

    #[inline]
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    #[inline]
    pub fn blob(&self) -> &Blob {
        &self.blob
    }

    #[inline]
    pub fn token_helper(&self) -> &TokenHelper {
        &self.token_helper
    }

    #[inline]
    pub fn trusted_proxies(&self) -> &TrustedProxies {
        &self.trusted_proxies
    }

    #[inline]
    pub fn auth_rate_limit(&self) -> AuthRateLimit {
        self.auth_rate_limit
    }

    #[inline]
    pub fn login_phone_rate(&self) -> LoginPhoneRate {
        self.login_phone_rate
    }
}

impl FromRef<AppState> for Db {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AppState> for Cache {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.cache.clone()
    }
}

impl FromRef<AppState> for Blob {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.blob.clone()
    }
}

impl FromRef<AppState> for TokenHelper {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.token_helper.clone()
    }
}

impl FromRef<AppState> for AuthRateLimit {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.auth_rate_limit
    }
}

impl FromRef<AppState> for LoginPhoneRate {
    #[inline]
    fn from_ref(state: &AppState) -> Self {
        state.login_phone_rate
    }
}

impl HasTrustedProxies for AppState {
    #[inline]
    fn trusted_proxies(&self) -> &TrustedProxies {
        &self.trusted_proxies
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::SlidingWindowRate;
    use std::time::Duration;

    #[test]
    fn window_params_derives_from_quota() {
        // Default 5/s burst 20 → 20 hits per 4 s window (sustained 5/s).
        let rate = SlidingWindowRate {
            per_second: 5,
            burst: 20,
        };
        assert_eq!(rate.window_params(), (Duration::from_secs(4), 20_usize));
    }

    #[test]
    fn burst_below_rate_is_clamped_up_to_honor_rate() {
        // 100/s with burst 10 would under-deliver; clamped to 100 hits / 1 s.
        let rate = SlidingWindowRate {
            per_second: 100,
            burst: 10,
        };
        assert_eq!(rate.window_params(), (Duration::from_secs(1), 100_usize));
    }

    #[test]
    fn zero_values_degrade_instead_of_panicking() {
        let rate = SlidingWindowRate {
            per_second: 0,
            burst: 0,
        };
        assert_eq!(rate.window_params(), (Duration::from_secs(1), 1_usize));
    }
}
