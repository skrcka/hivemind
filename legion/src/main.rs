//! `legion` — Hivemind on-drone agent binary.
//!
//! One instance per drone, running on the companion computer (a
//! Raspberry Pi 5 alongside the Pixhawk on v1). Delegated almost
//! entirely to the `legion` library crate — `main.rs` only handles
//! clap dispatch and the tokio runtime bootstrap.
//!
//! See `legion/README.md` for the full architecture.

use anyhow::Result;
use clap::Parser;
use legion::cli::{Cli, Command};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,legion=debug")),
        )
        .init();

    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async move {
        match cli.command {
            Command::Serve(args) => legion::cli::serve::run(args).await,
            Command::Debug(args) => legion::cli::debug::run(args).await,
        }
    })
}
