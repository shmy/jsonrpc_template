use app_state::AppState;
use axum::{Router, routing::get};
use http_routing::ProbeRouter;
use modrun::Module;

mod endpoints;

pub fn module() -> Module {
    Module::builder("probe").provide_group(provide_endpoints)
}

fn provide_endpoints() -> ProbeRouter {
    ProbeRouter(
        Router::<AppState>::new()
            .route("/livez", get(endpoints::livez::livez))
            .route("/healthz", get(endpoints::healthz::healthz))
            .route("/readyz", get(endpoints::readyz::readyz)),
    )
}
