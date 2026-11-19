use crate::cli::Cli;
use clap::Parser;
use std::process::exit;
pub mod cli;
pub mod readable_duration;
pub mod trusted_proxies;

pub use trusted_proxies::TrustedProxies;

pub fn load() -> Cli {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    if let Err(err) = cli.validate() {
        eprintln!("error: {err}");
        exit(2);
    }
    cli
}
