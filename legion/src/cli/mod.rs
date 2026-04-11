//! Clap-driven CLI for the legion binary. Two subcommands:
//!
//! - `legion serve` — the production daemon (what systemd runs).
//! - `legion debug …` — a tree of hand-testing helpers for operating
//!   legion without oracle.

use clap::{Parser, Subcommand};

pub mod debug;
pub mod serve;

#[derive(Parser, Debug)]
#[command(
    name = "legion",
    version,
    about = "Hivemind on-drone agent (Pi-side)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the production daemon.
    Serve(serve::ServeArgs),
    /// Debug / hand-testing subcommands.
    Debug(debug::DebugArgs),
}
