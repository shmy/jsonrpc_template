//! Redis 后端（bb8 连接池）。
use std::time::Duration;

use bb8_redis::RedisConnectionManager;
use bb8_redis::bb8::Pool;
use jiff::Zoned;
use redis::AsyncCommands;
use redis::Script;
use rootcause::Result;
use uuid::Uuid;

#[derive(Clone)]
pub struct RedisCache {
    pool: Pool<RedisConnectionManager>,
}

impl RedisCache {
    pub(crate) async fn try_new(pool: Pool<RedisConnectionManager>) -> Result<Self> {
        Ok(Self { pool })
    }

    pub async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.pool.get().await?;
        Ok(conn.get(key).await?)
    }

    pub async fn set_ex_raw(&self, key: &str, value: &str, period: Duration) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let _: () = conn.set_ex(key, value, period.as_secs()).await?;
        Ok(())
    }

    pub async fn take_raw(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.pool.get().await?;
        let value: Option<String> = redis::cmd("GETDEL")
            .arg(key)
            .query_async(&mut *conn)
            .await?;
        Ok(value)
    }

    pub async fn del_raw(&self, key: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let _: () = conn.del(key).await?;
        Ok(())
    }

    pub async fn allow_sliding_window(
        &self,
        key: &str,
        window: Duration,
        max_hits: usize,
    ) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let now_ms = Zoned::now().timestamp().as_millisecond();
        let window_ms = window.as_millis() as i64;
        let member = format!("{now_ms}:{}", Uuid::new_v4());
        let allowed: i32 = Script::new(
            r"
            local key = KEYS[1]
            local now = tonumber(ARGV[1])
            local window = tonumber(ARGV[2])
            local max_hits = tonumber(ARGV[3])
            local member = ARGV[4]
            redis.call('ZREMRANGEBYSCORE', key, 0, now - window)
            local count = redis.call('ZCARD', key)
            if count >= max_hits then
                return 0
            end
            redis.call('ZADD', key, now, member)
            redis.call('PEXPIRE', key, window)
            return 1
            ",
        )
        .key(key)
        .arg(now_ms)
        .arg(window_ms)
        .arg(max_hits as i64)
        .arg(member)
        .invoke_async(&mut *conn)
        .await?;
        Ok(allowed == 1)
    }
}
