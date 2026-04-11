//! Step assembly: turn a `RawSortie` (bare spray passes) into a typed
//! `Sortie` whose `steps` legion's executor can drive directly.
//!
//! Sequence: `Takeoff → Transit(to first pass start) → SprayPass → Transit
//! → SprayPass → ... → ReturnToBase → Land`.
//!
//! Duration arithmetic does positive-finite f32 → u32 conversions intentionally.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use glam::{DVec3, Vec3};
use hivemind_protocol::{
    RadioLossBehaviour, RadioLossPolicy, Sortie, SortieStep, StepType, Waypoint,
};

use crate::config::SlicerConfig;

use super::coverage::SprayPass;
use super::geometry;
use super::sortie_pack::RawSortie;

/// Wrap a raw sortie in the typed step sequence. The radio-loss policy on
/// each step is left as a placeholder here and stamped properly in the
/// `radio_loss::assign_defaults` stage.
pub fn assemble(raw: RawSortie, cfg: &SlicerConfig) -> Sortie {
    let mut steps = Vec::new();
    let mut step_idx: u32 = 0;
    let mut total_duration_s: u32 = 0;

    let placeholder_policy = || RadioLossPolicy {
        behaviour: RadioLossBehaviour::Continue,
        silent_timeout_s: 30.0,
        hold_then_rtl_after_s: None,
    };

    // Truck origin in lat/lon (we'll come back to land here).
    let origin_wp = Waypoint {
        lat: cfg.origin_lat_deg,
        lon: cfg.origin_lon_deg,
        alt_m: cfg.origin_alt_m,
        yaw_deg: None,
    };

    // 1. Takeoff at the origin.
    let takeoff_wp = Waypoint {
        alt_m: cfg.origin_alt_m + cfg.takeoff_alt_m,
        ..origin_wp
    };
    let takeoff_dur = (cfg.takeoff_alt_m / cfg.ferry_speed_m_s.max(0.1)).ceil() as u32;
    steps.push(SortieStep {
        index: step_idx,
        step_type: StepType::Takeoff,
        waypoint: takeoff_wp,
        path: None,
        speed_m_s: cfg.ferry_speed_m_s,
        spray: false,
        radio_loss: placeholder_policy(),
        expected_duration_s: takeoff_dur,
    });
    step_idx += 1;
    total_duration_s += takeoff_dur;

    // 2. For each spray pass, emit a Transit (to start) and a SprayPass.
    let mut prev_pos: Option<Vec3> = None;
    for pass in &raw.passes {
        let approach_pos = pass.start_enu + pass.normal * cfg.standoff_m;
        let exit_pos = pass.end_enu + pass.normal * cfg.standoff_m;

        // Transit (skip the first one if we're flying directly from takeoff
        // to the first pass approach — the takeoff already put us at altitude
        // above the origin, so we still need a transit).
        let transit_dist = match prev_pos {
            Some(prev) => (approach_pos - prev).length(),
            None => (approach_pos - origin_enu_at_alt(cfg)).length(),
        };
        let transit_dur = (transit_dist / cfg.ferry_speed_m_s.max(0.1)).ceil() as u32;

        let approach_wgs = enu_waypoint(approach_pos.into(), cfg, None);
        steps.push(SortieStep {
            index: step_idx,
            step_type: StepType::Transit,
            waypoint: approach_wgs,
            path: None,
            speed_m_s: cfg.ferry_speed_m_s,
            spray: false,
            radio_loss: placeholder_policy(),
            expected_duration_s: transit_dur,
        });
        step_idx += 1;
        total_duration_s += transit_dur;

        // Spray pass: yaw points along the surface, in the direction the
        // drone moves. For v1 we leave yaw unconstrained (None) — legion will
        // hold whatever yaw it had on entry.
        let pass_dist = (exit_pos - approach_pos).length();
        let pass_dur = (pass_dist / cfg.spray_speed_m_s.max(0.1)).ceil() as u32;
        let exit_wgs = enu_waypoint(exit_pos.into(), cfg, None);
        let path = vec![approach_wgs, exit_wgs];
        steps.push(SortieStep {
            index: step_idx,
            step_type: StepType::SprayPass,
            waypoint: approach_wgs,
            path: Some(path),
            speed_m_s: cfg.spray_speed_m_s,
            spray: true,
            radio_loss: placeholder_policy(),
            expected_duration_s: pass_dur,
        });
        step_idx += 1;
        total_duration_s += pass_dur;

        prev_pos = Some(exit_pos);
    }

    // 3. Return to base.
    let last_pos = prev_pos.unwrap_or_else(|| origin_enu_at_alt(cfg));
    let rtb_dist = (origin_enu_at_alt(cfg) - last_pos).length();
    let rtb_dur = (rtb_dist / cfg.ferry_speed_m_s.max(0.1)).ceil() as u32;
    steps.push(SortieStep {
        index: step_idx,
        step_type: StepType::ReturnToBase,
        waypoint: takeoff_wp,
        path: None,
        speed_m_s: cfg.ferry_speed_m_s,
        spray: false,
        radio_loss: placeholder_policy(),
        expected_duration_s: rtb_dur,
    });
    step_idx += 1;
    total_duration_s += rtb_dur;

    // 4. Land at the origin.
    let land_dur = (cfg.takeoff_alt_m / cfg.ferry_speed_m_s.max(0.1)).ceil() as u32;
    steps.push(SortieStep {
        index: step_idx,
        step_type: StepType::Land,
        waypoint: origin_wp,
        path: None,
        speed_m_s: cfg.ferry_speed_m_s,
        spray: false,
        radio_loss: placeholder_policy(),
        expected_duration_s: land_dur,
    });
    total_duration_s += land_dur;

    // Estimate paint volume — caller (resources) will recompute, but the
    // Sortie struct needs a value here too.
    let total_pass_length: f32 = raw.passes.iter().map(SprayPass::length_m).sum();
    let paint_volume_ml =
        total_pass_length * cfg.spray_width_m * paint_ml_per_m2();

    Sortie {
        sortie_id: raw.sortie_id,
        plan_id: raw.plan_id.to_string(),
        drone_id: raw.drone_id,
        steps,
        paint_volume_ml,
        expected_duration_s: total_duration_s,
    }
}

/// Origin in ENU at takeoff altitude (the point we reach after the Takeoff
/// step). Used as the starting position for transit-distance calculations.
fn origin_enu_at_alt(cfg: &SlicerConfig) -> Vec3 {
    Vec3::new(0.0, 0.0, cfg.takeoff_alt_m)
}

/// Convert an ENU position to a `Waypoint` with WGS84 lat/lon.
fn enu_waypoint(enu: DVec3, cfg: &SlicerConfig, yaw_deg: Option<f32>) -> Waypoint {
    let (lat, lon, alt_m) = geometry::enu_to_wgs84(enu, cfg);
    Waypoint {
        lat,
        lon,
        alt_m,
        yaw_deg,
    }
}

/// Reference paint coverage rate (ml per m²). Industrial coatings are
/// roughly 100–300 g/m² wet film thickness; we pick a v1 placeholder.
fn paint_ml_per_m2() -> f32 {
    150.0
}
