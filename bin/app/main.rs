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
        // 表面在 module() 同步段注册 FTL/mapper / attachment in-use；
        // http_server 的 invoke 里 seal。check_module_order 要求注册
        // locale 的表面排在 http_server 前。identity 习惯上先于 file。
        .module(identity::module())
        .module(file::module())
        .module(probe::module())
        .module(http_server::module())
        .run()
        .await
}
