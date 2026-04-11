//! `hivemind plan --intent file.json` — load an intent from disk, run the
//! slicer locally, print the resulting plan summary. Standalone — does not
//! require the daemon to be running.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::config::OracleConfig;
use crate::domain::fleet::{Drone, DroneState, FleetSnapshot};
use crate::domain::intent::Intent;
use crate::domain::plan::HivemindPlan;
use crate::slicer;

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Intent JSON file produced by pantheon.
    #[arg(long)]
    pub intent: PathBuf,

    /// Number of drones available to plan against. Drones are stubbed with
    /// ids `drone-01`, `drone-02`, … `drone-NN` and idle state. The slicer
    /// distributes spray passes across them in contiguous chunks.
    #[arg(long, default_value_t = 1)]
    pub drones: u32,

    /// ID prefix used when stubbing drones. The first drone is
    /// `<prefix>-01`, the second `<prefix>-02`, …
    #[arg(long, default_value = "drone")]
    pub drone_prefix: String,

    /// Print the full HivemindPlan as pretty JSON to stdout (after the
    /// summary). Useful for piping into `jq` or saving manually.
    #[arg(long)]
    pub json: bool,

    /// Write the full HivemindPlan as pretty JSON to this file path.
    /// Combine with `--json` to also print to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// Suppress the human-readable summary; useful with `--json` for clean
    /// pipe output.
    #[arg(long)]
    pub quiet: bool,
}

#[allow(clippy::unused_async)] // kept async to match the dispatch in main.rs
pub async fn run(cfg: OracleConfig, args: PlanArgs) -> Result<()> {
    let bytes = std::fs::read(&args.intent)
        .with_context(|| format!("reading intent {}", args.intent.display()))?;
    let intent: Intent = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing intent {}", args.intent.display()))?;

    if args.drones == 0 {
        anyhow::bail!("--drones must be at least 1");
    }

    // Stub the requested number of idle drones.
    let drones: Vec<Drone> = (1..=args.drones)
        .map(|n| Drone {
            id: format!("{}-{n:02}", args.drone_prefix),
            legion_version: None,
            capabilities: vec!["spray".into()],
            state: DroneState::default(),
            is_stale: false,
        })
        .collect();
    let snapshot = FleetSnapshot::now(drones);

    let plan = slicer::plan(intent, snapshot, &cfg.slicer)?;

    if !args.quiet {
        print_summary(&plan);
    }

    if let Some(out_path) = &args.out {
        let pretty = serde_json::to_string_pretty(&plan)
            .context("serialising plan to JSON")?;
        std::fs::write(out_path, &pretty)
            .with_context(|| format!("writing plan to {}", out_path.display()))?;
        if !args.quiet {
            eprintln!("\n→ wrote {} ({} bytes)", out_path.display(), pretty.len());
        }
    }

    if args.json {
        let pretty = serde_json::to_string_pretty(&plan)
            .context("serialising plan to JSON")?;
        println!("{pretty}");
    }

    Ok(())
}

fn print_summary(plan: &HivemindPlan) {
    println!("Plan {}", plan.id);
    println!("  status: {:?}", plan.status);
    println!("  coverage:");
    println!("    total area: {:.2} m²", plan.coverage.total_area_m2);
    println!("    overlap:    {:.0}%", f64::from(plan.coverage.overlap_pct) * 100.0);
    println!("    passes:     {}", plan.coverage.pass_count);
    println!("  resources:");
    println!("    paint:      {:.0} ml", plan.resources.paint_ml.max(0.0));
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

    if !plan.sorties.is_empty() {
        println!("  sorties:");
        for sortie in &plan.sorties {
            println!(
                "    {} ({} steps, {:.0} ml, {} s) → drone {}",
                sortie.sortie_id,
                sortie.steps.len(),
                sortie.paint_volume_ml,
                sortie.expected_duration_s,
                sortie.drone_id,
            );
        }
    }
}
