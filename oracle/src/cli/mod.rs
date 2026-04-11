//! `hivemind` clap CLI.
//!
//! v1 ships `serve` (the daemon) and `plan` (a thin client that POSTs an
//! intent file and prints the resulting proposal). Other subcommands return
//! 501-style messages until they're implemented.

pub mod plan;
pub mod serve;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "hivemind", version, about = "Hivemind oracle — truck-side runtime")]
pub struct Cli {
    /// Path to the oracle.toml config file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the oracle daemon (HTTP+WS API + legion link + fleet monitor).
    Serve(serve::ServeArgs),

    /// Upload an intent.json and create a plan.
    Plan(plan::PlanArgs),

    /// Approve a plan and start the apply phase.
    Apply(StubArgs),

    /// Show fleet status + active plan summary.
    Status(StubArgs),

    /// Abort the active plan.
    Abort(StubArgs),

    /// Tail the audit log.
    Audit(StubArgs),
}

#[derive(Debug, clap::Args)]
pub struct StubArgs {
    /// Daemon URL (defaults to ORACLE_URL or http://127.0.0.1:7345).
    #[arg(long, default_value = "http://127.0.0.1:7345")]
    pub daemon_url: String,
}
