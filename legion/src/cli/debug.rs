//! `legion debug …` — hand-testing helpers that skip oracle entirely.
//!
//! All subcommands operate against the same backends the production
//! daemon uses (`MockPayload` + `StubMavlinkDriver` + `FileSortieStore`
//! on dev builds). This keeps the debug path honest: whatever it
//! exercises is what `legion serve` would do.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::config::Config;

#[derive(Args, Debug)]
pub struct DebugArgs {
    /// Path to config.toml. Same rules as `legion serve`.
    #[arg(long, default_value = "/etc/legion/config.toml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: DebugCommand,
}

#[derive(Subcommand, Debug)]
pub enum DebugCommand {
    /// Print a snapshot of the loaded config + runtime state.
    Status,
    /// Load a sortie JSON file and print its summary.
    LoadSortie {
        /// Path to a sortie .json file (same schema as the wire type).
        file: PathBuf,
    },
}

pub async fn run(args: DebugArgs) -> Result<()> {
    let config = Config::load(Some(&args.config))?;

    match args.command {
        DebugCommand::Status => {
            println!("drone_id       = {}", config.drone.id);
            println!("capabilities   = {:?}", config.drone.capabilities);
            println!("mavlink.address= {}", config.mavlink.address);
            println!("transport      = {:?}", config.transport);
            println!("sortie_dir     = {}", config.storage.sortie_dir.display());
            println!("safety         = {:?}", config.safety);
        }
        DebugCommand::LoadSortie { file } => {
            let body = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let sortie: hivemind_protocol::Sortie = serde_json::from_str(&body)
                .with_context(|| format!("parsing {}", file.display()))?;
            println!("sortie_id    = {}", sortie.sortie_id);
            println!("plan_id      = {}", sortie.plan_id);
            println!("drone_id     = {}", sortie.drone_id);
            println!("steps        = {}", sortie.steps.len());
            for step in &sortie.steps {
                println!(
                    "  [{}] {:?} alt={} spray={} policy={:?}",
                    step.index,
                    step.step_type,
                    step.waypoint.alt_m,
                    step.spray,
                    step.radio_loss.behaviour
                );
            }
        }
    }

    Ok(())
}
