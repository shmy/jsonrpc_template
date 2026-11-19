//! 进程内缓存（单测用）。
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rootcause::Result;
use std::sync::MutexGuard;

#[derive(Clone)]
pub struct MemoryCache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

struct Entry {
    value: String,
    expires_at: Instant,
}

impl MemoryCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let mut map = self.lock();
        Ok(match map.get(key) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.value.clone()),
            Some(_) => {
                map.remove(key);
                None
            }
            None => None,
        })
    }

    pub async fn set_ex_raw(&self, key: &str, value: &str, period: Duration) -> Result<()> {
        let mut map = self.lock();
        map.insert(
            key.to_string(),
            Entry {
                value: value.to_string(),
                expires_at: Instant::now() + period,
            },
        );
        Ok(())
    }

    pub async fn take_raw(&self, key: &str) -> Result<Option<String>> {
        let mut map = self.lock();
        Ok(match map.remove(key) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.value),
            _ => None,
        })
    }

    pub async fn del_raw(&self, key: &str) -> Result<()> {
        self.lock().remove(key);
        Ok(())
    }

    /// 简化滑动窗口：窗口内累计计数（非精确 ZSET，仅供单测）。
    pub async fn allow_sliding_window(
        &self,
        key: &str,
        window: Duration,
        max_hits: usize,
    ) -> Result<bool> {
        let count_key = format!("swc:{key}");
        let mut map = self.lock();
        let now = Instant::now();
        let count = match map.get(&count_key) {
            Some(entry) if entry.expires_at > now => entry.value.parse::<usize>().unwrap_or(0),
            Some(_) => {
                map.remove(&count_key);
                0
            }
            None => 0,
        };
        if count >= max_hits {
            return Ok(false);
        }
        map.insert(
            count_key,
            Entry {
                value: (count + 1).to_string(),
                expires_at: now + window,
            },
        );
        Ok(true)
    }
}
