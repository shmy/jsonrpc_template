use blob_kit::Blob;
#[cfg(feature = "cos")]
use blob_kit::CosConfig;
#[cfg(all(feature = "fs", not(feature = "cos")))]
use blob_kit::FsConfig;
use config::cli::Cli;
use config::cli::ExposeSecret;
use modrun::{Module, Result};

pub fn module() -> Module {
    Module::builder("blob").provide_result_async(provide_blob)
}

#[allow(
    unused_variables,
    reason = "modrun provider 按互斥 feature 选用 cli 字段之一"
)]
async fn provide_blob(cli: Cli) -> Result<Blob> {
    // cos 优先：单开 cos、或 `--all-features` 并存时都走生产默认；仅 fs 独占时落盘。
    #[cfg(feature = "cos")]
    {
        Blob::try_new_cos(CosConfig {
            endpoint: &cli.blob.blob_s3_endpoint,
            domain: &cli.blob.blob_s3_domain,
            bucket: &cli.blob.blob_s3_bucket,
            secret_id: cli.blob.blob_s3_secret_id.expose_secret(),
            secret_key: cli.blob.blob_s3_secret_key.expose_secret(),
        })
        .await
        .map_err(|e| modrun::Error::hook(format!("blob init: {e:#}")))
    }
    #[cfg(all(feature = "fs", not(feature = "cos")))]
    {
        // fill / strip 共用去尾斜杠后的 domain，避免 `…/files/` 与 `…/files` 对不齐。
        // BLOB_FS_* 已在 clap（config/fs）层必填且校验长度。
        let domain = cli.blob.blob_fs_domain.trim_end_matches('/');
        let presign_put_base = format!("{domain}/presigned");
        let presign_secret = cli.blob.blob_fs_presign_secret.expose_secret();
        Blob::try_new_fs(FsConfig {
            root: &cli.blob.blob_fs_root,
            domain,
            presign_put_base: &presign_put_base,
            presign_secret: presign_secret.as_bytes(),
        })
        .await
        .map_err(|e| modrun::Error::hook(format!("blob init: {e:#}")))
    }
    #[cfg(not(any(feature = "cos", feature = "fs")))]
    compile_error!("blob wiring requires feature \"cos\" or \"fs\"");
}
