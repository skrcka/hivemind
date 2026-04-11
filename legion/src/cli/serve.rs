//! `legion serve` — the production daemon entrypoint. Loads config,
//! hands off to `runtime::run`.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::config::Config;
use crate::runtime;

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Path to `config.toml`. Defaults to `/etc/legion/config.toml`;
    /// if absent, legion runs with built-in defaults (useful for dev).
    #[arg(long, default_value = "/etc/legion/config.toml")]
    pub config: PathBuf,
}

pub async fn run(args: ServeArgs) -> Result<()> {
    let config = Config::load(Some(&args.config))?;
    tracing::info!(?config, "legion serve: loaded config");
    runtime::run(config).await?;
    Ok(())
}
