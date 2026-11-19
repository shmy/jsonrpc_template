#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

use std::time::Duration;

use app_state::AppState;
#[cfg(feature = "blob-fs")]
use axum::Router;
#[cfg(feature = "blob-fs")]
use axum::middleware::from_fn;
#[cfg(feature = "blob-fs")]
use axum::routing::get_service;
#[cfg(feature = "blob-fs")]
use axum::routing::put;
use axum_jsonrpc::{RegisterError, RpcRouter};
use blob_kit::Blob;
use config::cli::Cli;
use http_routing::PrivateRpc;
#[cfg(feature = "blob-fs")]
use http_routing::PublicRouter;
use modrun::{Lifecycle, Module, task};
use toasty::Db;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;
#[cfg(feature = "blob-fs")]
use tower_http::services::ServeDir;

#[cfg(feature = "blob-fs")]
mod endpoints;
mod methods;
mod shared;

/// GC 周期（无 job_queue；仅 `MASTER=true` 挂 Lifecycle 间隔任务）。
const GC_INTERVAL: Duration = Duration::from_secs(3600);

pub fn module() -> Module {
    shared::l10n::register_file_locale_bundle();
    let module = Module::builder("file")
        .provide_group_result(provide_private_rpc)
        .invoke(register_gc_tasks);
    #[cfg(feature = "blob-fs")]
    let module = module.provide_group(public_routes);
    module
}

/// 本地 fs：`ServeDir(BLOB_FS_ROOT)` + 路径白名单（dated staging/正式）；
/// 不可信扩展强制 `Content-Disposition: attachment`。`PUT /files/presigned` 仍走 HMAC。
#[cfg(feature = "blob-fs")]
fn public_routes(cli: Cli) -> PublicRouter {
    let root = cli.blob.blob_fs_root.clone();
    let static_files = get_service(ServeDir::new(root))
        .layer(from_fn(endpoints::file_serve_guard::blob_fs_static_guard));

    PublicRouter(
        Router::<AppState>::new().nest(
            "/files",
            Router::new()
                .route("/presigned", put(endpoints::file_presigned_put::handler))
                .fallback_service(static_files),
        ),
    )
}

fn provide_private_rpc(cli: Cli) -> Result<PrivateRpc, RegisterError> {
    shared::allowed_extensions::init_from_config(&cli.blob.allowed_extensions);
    Ok(PrivateRpc(
        RpcRouter::<AppState>::new().register(methods::presign::presign_rpc)?,
    ))
}

fn log_gc_result(label: &str, result: rootcause::Result<u64>) {
    match result {
        Ok(n) if n > 0 => tracing::info!(deleted = n, "{label} purged objects"),
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, "{label} failed"),
    }
}

fn register_gc_tasks(lc: Lifecycle, cli: Cli, db: Db, blob: Blob) -> modrun::Result<()> {
    if !cli.server.master {
        tracing::info!("not master, skipping blob GC tasks");
        return Ok(());
    }

    let blob_for_staging = blob.clone();
    lc.append(task("blob_staging_gc", move |mut stopped| async move {
        let mut ticker = interval(GC_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = &mut stopped => break,
                _ = ticker.tick() => {
                    log_gc_result(
                        "staging gc",
                        shared::staging_gc::purge_expired(
                            &blob_for_staging,
                            shared::staging_gc::STAGING_RETENTION_DAYS,
                        )
                        .await,
                    );
                }
            }
        }
        Ok(())
    }))?;

    lc.append(task("blob_attachment_gc", move |mut stopped| async move {
        let mut db = db;
        let mut ticker = interval(GC_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // 启动时跳过首 tick，避免与 migrate / seed 抢库。
        ticker.tick().await;
        loop {
            tokio::select! {
                () = &mut stopped => break,
                _ = ticker.tick() => {
                    log_gc_result(
                        "attachment gc",
                        shared::attachment_gc::run_once(&mut db, &blob).await,
                    );
                }
            }
        }
        Ok(())
    }))?;
    Ok(())
}
