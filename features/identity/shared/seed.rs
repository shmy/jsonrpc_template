//! 管理员 seed（仅 `MASTER=true`）：按配置手机号 upsert 一个特权账号。

use config::cli::{Cli, ExposeSecret};
use identity_contract::{Account, Id};
use modrun::{Error, Lifecycle, hook};
use toasty::Db;

use crate::shared::hashed_password::HashedPassword;

/// 注册启动 seed 钩子（非 master 时跳过）。
pub(crate) fn register_seed(lc: Lifecycle, cli: Cli, db: Db) -> modrun::Result<()> {
    if !cli.server.master {
        tracing::info!("not master, skipping admin seed");
        return Ok(());
    }
    let phone = cli.seed.admin_phone.clone();
    let password = cli.seed.admin_password.expose_secret().to_owned();
    lc.append(hook().name("seed_admin").on_start(move || {
        let mut db = db.clone();
        let phone = phone.clone();
        let password = password.clone();
        async move {
            // seed 是手机号形态校验的唯一绕过点（API create/login 均经
            // validate_phone_format）；非法形态 fail-fast，避免「seed 成功
            // 但管理员永远 -32602 无法登录」的静默锁死。
            if !identity_contract::is_cn_mobile(&phone) {
                return Err(Error::hook(
                    "SEED_ADMIN_PHONE must be a mainland China mobile number: ^1[3-9] then 9 digits",
                ));
            }
            seed_admin(&mut db, &phone, &password)
                .await
                .map_err(|e| Error::hook(e.to_string()))
        }
    }))?;
    Ok(())
}

async fn seed_admin(db: &mut Db, phone: &str, password: &str) -> rootcause::Result<()> {
    // toasty does not emit filter_by_bool, so seed is keyed by the configured phone.
    // NOTE: changing SEED_ADMIN_PHONE on a re-deploy seeds a NEW privileged account;
    // removal/demotion of the old one is an operator action.
    if let Some(existing) = Account::filter_by_phone(phone).first().exec(db).await? {
        if existing.privileged {
            tracing::info!(phone, "privileged seed account exists, skipping seed");
        } else {
            tracing::warn!(
                phone,
                "SEED_ADMIN_PHONE is occupied by a non-privileged account; \
                 no seed admin was created. Change SEED_ADMIN_PHONE or promote manually."
            );
        }
        return Ok(());
    }

    tracing::info!(phone, "creating seed admin");
    let hashed = HashedPassword::try_new(password).await?;
    let Some(_) = Account::upsert_by_phone(phone)
        .name("admin".to_string())
        .password(hashed.into_string())
        .privileged(true)
        .avatar(None::<String>)
        .remark(None::<String>)
        .on_create(|a| a.id(Id::default()))
        .or_ignore()
        .exec(db)
        .await?
    else {
        tracing::info!(phone, "seed admin phone already present, skipping seed");
        return Ok(());
    };
    Ok(())
}
