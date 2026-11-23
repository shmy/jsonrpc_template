//! 最小 feature：`module()` 只做注册，业务细节放 `methods/` / `shared/`。

use app_state::AppState;
use axum_jsonrpc::{RegisterError, RpcRouter};
use http_routing::PrivateRpc;
use modrun::Module;

mod methods;
mod shared; // 有 locale / 跨 method 逻辑时保留；见 shared/l10n.rs

pub fn module() -> Module {
    shared::l10n::register_FEATURE_locale_bundle();
    Module::builder("FEATURE_NAME").provide_group_result(provide_private_methods)
}

fn provide_private_methods() -> Result<PrivateRpc, RegisterError> {
    // 默认进 PrivateRpc；匿名方法另写 provide_public_methods → PublicRpc。
    let router = RpcRouter::<AppState>::new();
    // let router = router.register(methods::example::example_rpc)?;
    Ok(PrivateRpc(router))
}
