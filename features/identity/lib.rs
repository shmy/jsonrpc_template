use app_state::AppState;
use axum_jsonrpc::{RegisterError, RpcRouter};
use http_routing::{PrivateRpc, PublicRpc};
use modrun::Module;

mod methods;
mod shared;

pub fn module() -> Module {
    shared::l10n::register_identity_locale_bundle();
    shared::avatar_in_use::register();
    Module::builder("identity")
        .provide_group_result(provide_public_methods)
        .provide_group_result(provide_private_methods)
        .invoke(shared::seed::register_seed)
}

fn provide_public_methods() -> Result<PublicRpc, RegisterError> {
    Ok(PublicRpc(
        RpcRouter::<AppState>::new()
            .register(methods::login_account::login_account_rpc)?
            .register(methods::refresh_account::refresh_account_rpc)?,
    ))
}

fn provide_private_methods() -> Result<PrivateRpc, RegisterError> {
    Ok(PrivateRpc(
        RpcRouter::<AppState>::new()
            .register(methods::create_account::create_account_rpc)?
            .register(methods::change_password::change_password_rpc)?
            .register(methods::delete_account::delete_account_rpc)?
            .register(methods::logout_account::logout_account_rpc)?
            .register(methods::read_account::read_account_rpc)?
            .register(methods::update_account::update_account_rpc)?
            .register(methods::search_account::search_account_rpc)?
            .register(methods::account_profile::account_profile_rpc)?,
    ))
}
