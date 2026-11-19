//! Method 内 Redis 限流（login/refresh）：IP 滑动窗 + 手机号固定窗。
//! 配额均为进程级配置（`app_state`），非硬编码。

use app_state::{AuthRateLimit, LoginPhoneRate};
use cache_kit::Cache;
use http_extractors::ClientIp;
use identity_contract::IdentityError;
use rootcause::Result;

use crate::shared::token_ops::map_cache_err;

fn login_phone_key(phone: &str) -> String {
    format!("identity:login:rate:{phone}")
}

/// auth 面 RPC 标识：拼进 IP 限流 cache key。
///
/// 用 enum 而非裸字符串：新增/改名由编译器兜底，typo 不会静默
/// 分裂出一个新的配额桶（字符串拼错只会让限流失效/降级）。
#[derive(Clone, Copy, Debug)]
pub(crate) enum AuthFace {
    Login,
    Refresh,
}

impl AuthFace {
    const fn key(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Refresh => "refresh",
        }
    }
}

fn auth_ip_key(face: AuthFace, ip: &ClientIp) -> String {
    format!("identity:{}:ip:{}", face.key(), ip.rate_limit_key())
}

fn enforce(allowed: Result<bool>, rejected: IdentityError) -> Result<()> {
    if !allowed.map_err(map_cache_err)? {
        return Err(rejected.into());
    }
    Ok(())
}

/// login/refresh：按客户端 IP（`RATE_LIMIT_AUTH_*`）。
pub(crate) async fn enforce_auth_ip(
    cache: &Cache,
    auth_rate: AuthRateLimit,
    face: AuthFace,
    ip: &ClientIp,
    rejected: IdentityError,
) -> Result<()> {
    let key = auth_ip_key(face, ip);
    enforce(auth_rate.allow(cache, &key).await, rejected)
}

/// login：按手机号固定窗（`RATE_LIMIT_LOGIN_PHONE_*`；成功也计数）。
pub(crate) async fn enforce_login_phone(
    cache: &Cache,
    limit: LoginPhoneRate,
    phone: &str,
    rejected: IdentityError,
) -> Result<()> {
    enforce(limit.allow(cache, &login_phone_key(phone)).await, rejected)
}
