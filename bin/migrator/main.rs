use persistence::models;
use rootcause::{Result, report};
use toasty::Db;
use toasty_cli::{Config, ToastyCli};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let database_url = dotenvy::var("DATABASE_URL")?;
    let config = Config::load().map_err(|e| report!(e.to_string()))?;

    let db = Db::builder()
        .models(models())
        .connect(&database_url)
        .await?;

    let cli = ToastyCli::with_config(db, config);
    cli.parse_and_run()
        .await
        .map_err(|e| report!(e.to_string()))?;

    Ok(())
}
