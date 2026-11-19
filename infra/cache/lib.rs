//! Process-wide Redis [`Cache`](cache_kit::Cache) via modrun.

use cache_kit::{Cache, Pool, RedisConnectionManager};
use config::cli::Cli;
use modrun::Module;

pub fn module() -> Module {
    Module::builder("cache").provide_result_async(provide_cache)
}

async fn provide_cache(cli: Cli) -> rootcause::Result<Cache> {
    let manager = RedisConnectionManager::new(cli.cache.cache_redis_url.clone())?;
    let redis_pool = Pool::builder().max_size(16).build(manager).await?;
    Cache::try_new_redis(redis_pool).await
}
