use http_routing::{PrivateRouter, PrivateRpc, ProbeRouter, PublicRouter, PublicRpc};
use modrun::{Modrun, Result};
use trace_kit::{TraceConfig, init_tracing};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = config::load();
    #[cfg(feature = "otlp")]
    let _guard = {
        use config::cli::ExposeSecret;
        init_tracing(
            TraceConfig::new(
                &cli.log.level,
                &cli.otlp.service_name,
                &cli.otlp.endpoint,
                cli.otlp.metadata.expose_secret(),
                cli.otlp.sampler_ratio,
            ),
            vec![],
        )
    };
    #[cfg(not(feature = "otlp"))]
    let _guard = init_tracing(TraceConfig::new(&cli.log.level), vec![]);

    Modrun::builder()
        .supply(cli)
        // OpenRPC info.version 取 app 二进制自身的版本，而非 http_server 壳 crate 的版本
        .supply(http_server::OpenRpcVersion(env!("CARGO_PKG_VERSION")))
        // Contribution groups: app surfaces append via provide_group*; empty groups stay valid.
        .init_group::<PublicRouter>()
        .init_group::<PrivateRouter>()
        .init_group::<ProbeRouter>()
        .init_group::<PublicRpc>()
        .init_group::<PrivateRpc>()
        .module(persistence::module())
        .module(cache::module())
        .module(blob::module())
        // Features register FTL + domain mappers / attachment in-use hooks
        // here (before http_server seals at invoke). identity 先于 file，
        // 以便 AttachmentGc 启动前 in-use 钩子已注册。
        .module(identity::module())
        .module(file::module())
        .module(probe::module())
        .module(http_server::module())
        .run()
        .await
}
