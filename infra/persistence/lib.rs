use config::cli::{Cli, ExposeSecret as _};
use modrun::Module;
use rootcause::Result;
use toasty::migration::{MigrationReport, MigrationSet};
use toasty::{Db, ModelSet};
use tracing::info;

static MIGRATIONS: MigrationSet = toasty::embed_migrations!("../../toasty");

pub fn module() -> Module {
    Module::builder("persistence")
        .provide_result_async(provide_db)
        .invoke_async(migrate)
}

async fn provide_db(cli: Cli) -> Result<Db> {
    let db = Db::builder()
        .models(models())
        .max_pool_size(cli.database.max_connections as usize)
        .connect(cli.database.url.expose_secret())
        .await?;
    Ok(db)
}

/// Apply embedded migrations. Used by the api master process and by tests.
pub async fn apply_migrations(db: &Db) -> Result<MigrationReport> {
    Ok(MIGRATIONS.apply(db).await?)
}

async fn migrate(cli: Cli, db: Db) -> Result<()> {
    if !cli.server.master {
        info!("skipping migrations (MASTER=false)");
        return Ok(());
    }

    let report = apply_migrations(&db).await?;
    info!(
        applied = report.applied(),
        skipped = report.skipped(),
        "migrations applied"
    );
    Ok(())
}

pub fn models() -> ModelSet {
    toasty::models!(identity_contract::*, file_contract::*)
}
