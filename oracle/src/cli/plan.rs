//! `hivemind plan --intent file.json` — load an intent from disk, run the
//! slicer locally, print the resulting plan summary. Standalone — does not
//! require the daemon to be running.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::config::OracleConfig;
use crate::domain::fleet::{Drone, DroneState, FleetSnapshot};
use crate::domain::intent::Intent;
use crate::slicer;

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Intent JSON file produced by pantheon.
    #[arg(long)]
    pub intent: PathBuf,

    /// Drone id to assign the plan to (overrides config defaults).
    #[arg(long, default_value = "drone-01")]
    pub drone: String,
}

#[allow(clippy::unused_async)] // kept async to match the dispatch in main.rs
pub async fn run(cfg: OracleConfig, args: PlanArgs) -> Result<()> {
    let bytes = std::fs::read(&args.intent)
        .with_context(|| format!("reading intent {}", args.intent.display()))?;
    let intent: Intent = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing intent {}", args.intent.display()))?;

    // Stub a fleet snapshot with one idle drone.
    let snapshot = FleetSnapshot::now(vec![Drone {
        id: args.drone.clone(),
        legion_version: None,
        capabilities: vec!["spray".into()],
        state: DroneState::default(),
        is_stale: false,
    }]);

    let plan = slicer::plan(intent, snapshot, &cfg.slicer)?;

    println!("Plan {}", plan.id);
    println!("  status: {:?}", plan.status);
    println!("  coverage:");
    println!("    total area: {:.2} m²", plan.coverage.total_area_m2);
    println!("    overlap:    {:.0}%", plan.coverage.overlap_pct * 100.0);
    println!("    passes:     {}", plan.coverage.pass_count);
    println!("  resources:");
    println!("    paint:      {:.0} ml", plan.resources.paint_ml);
    println!("    flight:     {} s", plan.resources.total_flight_time_s);
    println!("    batteries:  {}", plan.resources.battery_cycles);
    println!("  schedule:");
    println!("    sorties:    {}", plan.schedule.total_sorties);
    println!("    duration:   {} s", plan.schedule.total_duration_s);
    println!("  warnings:    {}", plan.warnings.len());
    for w in &plan.warnings {
        println!("    [{:?}] {}: {}", w.severity, w.code.as_str(), w.message);
    }
    println!("  errors:      {}", plan.errors.len());
    for e in &plan.errors {
        println!("    [{}] {}", e.code.as_str(), e.message);
    }
    println!("  approvable:  {}", plan.is_approvable());

    Ok(())
}
