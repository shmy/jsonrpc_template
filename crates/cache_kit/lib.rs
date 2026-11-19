//! 缓存门面：生产默认 Redis；`memory` feature 供单测。

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "redis")]
mod redis;

#[cfg(feature = "redis")]
pub use bb8_redis::RedisConnectionManager;
#[cfg(feature = "redis")]
pub use bb8_redis::bb8::Pool;

use std::time::Duration;

use rootcause::Result;
use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "memory")]
pub use memory::MemoryCache;
#[cfg(feature = "redis")]
pub use redis::RedisCache;

#[cfg(not(any(feature = "redis", feature = "memory")))]
compile_error!("cache_kit requires feature \"redis\" and/or \"memory\"");

#[derive(Clone)]
pub enum Cache {
    #[cfg(feature = "redis")]
    Redis(RedisCache),
    #[cfg(feature = "memory")]
    Memory(MemoryCache),
}

impl Cache {
    #[cfg(feature = "redis")]
    pub async fn try_new_redis(pool: Pool<RedisConnectionManager>) -> Result<Self> {
        Ok(Self::Redis(RedisCache::try_new(pool).await?))
    }

    #[cfg(feature = "memory")]
    #[must_use]
    pub fn new_memory() -> Self {
        Self::Memory(MemoryCache::new())
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw = match self {
            #[cfg(feature = "redis")]
            Self::Redis(inner) => inner.get_raw(key).await?,
            #[cfg(feature = "memory")]
            Self::Memory(inner) => inner.get_raw(key).await?,
        };
        Ok(raw.map(|r| serde_json::from_str(&r)).transpose()?)
    }

    pub async fn set_ex<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
        period: Duration,
    ) -> Result<()> {
        let raw = serde_json::to_string(value)?;
        match self {
            #[cfg(feature = "redis")]
            Self::Redis(inner) => inner.set_ex_raw(key, &raw, period).await,
            #[cfg(feature = "memory")]
            Self::Memory(inner) => inner.set_ex_raw(key, &raw, period).await,
        }
    }

    pub async fn take<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw = match self {
            #[cfg(feature = "redis")]
            Self::Redis(inner) => inner.take_raw(key).await?,
            #[cfg(feature = "memory")]
            Self::Memory(inner) => inner.take_raw(key).await?,
        };
        Ok(raw.map(|r| serde_json::from_str(&r)).transpose()?)
    }

    pub async fn del(&self, key: &str) -> Result<()> {
        match self {
            #[cfg(feature = "redis")]
            Self::Redis(inner) => inner.del_raw(key).await,
            #[cfg(feature = "memory")]
            Self::Memory(inner) => inner.del_raw(key).await,
        }
    }

    pub async fn allow_sliding_window(
        &self,
        key: &str,
        window: Duration,
        max_hits: usize,
    ) -> Result<bool> {
        match self {
            #[cfg(feature = "redis")]
            Self::Redis(inner) => inner.allow_sliding_window(key, window, max_hits).await,
            #[cfg(feature = "memory")]
            Self::Memory(inner) => inner.allow_sliding_window(key, window, max_hits).await,
        }
    }

    /// 轻量可用性探针（供 `/readyz`）：读一个不存在的 key 验证后端可连通。
    pub async fn probe(&self) -> Result<()> {
        self.get::<()>("__health_probe__").await.map(|_| ())
    }
}
