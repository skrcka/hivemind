//! The plan engine — a pure function from `(Intent, FleetSnapshot, SlicerConfig)`
//! to `HivemindPlan`. Determinism is the design goal: same inputs, byte-
//! identical output (modulo timestamps).
//!
//! Stages:
//!
//! 1. [`coverage`] — turn each region into a set of straight-line spray passes.
//! 2. (lanes — no-op at v1; one drone, no conflicts.)
//! 3. [`sortie_pack`] — pack passes into sorties honouring per-drone capacity.
//! 4. [`steps`] — wrap each sortie in a typed `Takeoff → Transit → SprayPass …`
//!    sequence that legion's executor can drive.
//! 5. [`radio_loss`] — stamp a default `RadioLossPolicy` onto every step.
//! 6. [`schedule`] — lay sorties on a wall-clock timeline.
//! 7. [`resources`] — estimate paint volume and battery cycles.
//! 8. [`validate`] — collect warnings and errors.

pub mod coverage;
pub mod geometry;
pub mod radio_loss;
pub mod resources;
pub mod schedule;
pub mod sortie_pack;
pub mod steps;
pub mod validate;

use thiserror::Error;
use time::OffsetDateTime;

use crate::config::SlicerConfig;
use crate::domain::{
    fleet::FleetSnapshot,
    intent::Intent,
    plan::{
        HivemindPlan, PlanError, PlanErrorCode, PlanId, PlanStatus, PlanWarning,
    },
};

/// Errors the slicer can hit *before* it has produced a plan. Distinct from
/// [`PlanError`], which is something the slicer found in the input but still
/// produced a (non-approvable) plan for.
#[derive(Debug, Error)]
pub enum SlicerError {
    #[error("intent has no regions")]
    NoRegions,
    #[error("region {region_id}: {reason}")]
    RegionGeometry { region_id: String, reason: String },
    #[error("intent is not georeferenced; v1 requires georeferenced intents")]
    NotGeoreferenced,
}

/// Run the slicer. Pure function — runs on a `tokio::task::spawn_blocking`
/// in production because lane-packing and area subdivision are CPU-bound and
/// we don't want to stall the runtime.
pub fn plan(
    intent: Intent,
    fleet: FleetSnapshot,
    cfg: &SlicerConfig,
) -> Result<HivemindPlan, SlicerError> {
    if !intent.scan.georeferenced {
        return Err(SlicerError::NotGeoreferenced);
    }
    if intent.regions.is_empty() {
        return Err(SlicerError::NoRegions);
    }

    let plan_id = PlanId::new();
    let now = OffsetDateTime::now_utc();

    // 1. Coverage: regions → spray passes (auto-subdividing non-planar
    //    regions by clustering face normals).
    let coverage_result = coverage::generate_passes(&intent, cfg);
    let mut errors: Vec<PlanError> = coverage_result.errors;
    let mut coverage_warnings: Vec<PlanWarning> = coverage_result.warnings;
    let coverage_plan = coverage_result.coverage_plan;
    let passes = coverage_result.passes;

    // 2. Lanes — no-op for v1 (one drone). Architecturally we'd assign each
    //    pass to a lane and verify min separation; v1 just trusts the slicer
    //    to keep the single drone's passes sequential.

    // 3. Sortie packing: distribute passes across the available drones in
    //    contiguous chunks. If the fleet has zero drones, emit an error
    //    (but still produce a plan so the operator sees the geometry).
    if fleet.drones.is_empty() {
        errors.push(PlanError {
            code: PlanErrorCode::NoDronesAvailable,
            message: "no drones in the fleet snapshot".into(),
            region_id: None,
        });
    }

    let sorties_built = sortie_pack::pack(plan_id, &fleet.drones, &passes, cfg);

    // 4. Step assembly + 5. radio-loss policy stamping happens inside
    //    `steps::assemble` and `radio_loss::assign_defaults`.
    let mut sorties = Vec::with_capacity(sorties_built.len());
    for raw in sorties_built {
        let sortie_with_steps = steps::assemble(raw, cfg);
        let sortie_with_policies = radio_loss::assign_defaults(sortie_with_steps, cfg);
        sorties.push(sortie_with_policies);
    }

    // 6. Schedule.
    let schedule = schedule::build(&sorties);

    // 7. Resources.
    let resources = resources::estimate(&sorties, cfg);

    // 8. Validation — non-fatal checks that produce warnings. Coverage's
    //    own warnings (e.g. region subdivision) are merged in.
    let mut warnings: Vec<PlanWarning> = validate::run(&schedule, &resources, cfg);
    warnings.append(&mut coverage_warnings);

    Ok(HivemindPlan {
        id: plan_id,
        created_at: now,
        status: PlanStatus::Proposed,
        intent,
        coverage: coverage_plan,
        sorties,
        schedule,
        resources,
        warnings,
        errors,
        fleet_snapshot: fleet,
    })
}
