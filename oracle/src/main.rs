//! `hivemind` binary entry point.

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use oracle::cli::{Cli, Command};
use oracle::config::OracleConfig;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let cfg = OracleConfig::load(cli.config.as_deref())
        .map_err(|e| anyhow::anyhow!("loading config: {e}"))?;

    match cli.command {
        Command::Serve(args) => oracle::cli::serve::run(cfg, args).await,
        Command::Plan(args) => oracle::cli::plan::run(cfg, args).await,
        Command::Apply(_)
        | Command::Status(_)
        | Command::Abort(_)
        | Command::Audit(_) => {
            anyhow::bail!(
                "this CLI subcommand is a v1 stub — see oracle/README.md for the planned design"
            );
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info,sqlx=warn,hyper=warn,tokio_util=warn"))
        .expect("valid tracing filter");
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
