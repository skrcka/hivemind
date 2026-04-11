//! Telemetry, status, and event types — the legion → oracle status stream.

use serde::{Deserialize, Serialize};

use crate::sortie::SortieId;

/// Periodic state snapshot from a single drone. Sent at 2 Hz inside the
/// [`LegionToOracle::Telemetry`] variant. The single source of truth for
/// fleet state on the oracle side.
///
/// [`LegionToOracle::Telemetry`]: crate::messages::LegionToOracle::Telemetry
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Telemetry {
    /// Sender's monotonic milliseconds since boot — for jitter analysis.
    pub ts_ms: u64,
    pub position: Position,
    pub attitude: Attitude,
    pub battery_pct: f32,
    pub voltage: f32,
    pub paint_remaining_ml: f32,
    /// Forward-facing time-of-flight reading. `None` if the sensor is not
    /// installed or has no current reading.
    pub tof_distance_cm: Option<f32>,
    pub gps_fix: GpsFixType,
    /// The sortie this drone is currently executing, if any.
    pub sortie_id: Option<SortieId>,
    /// The step within the active sortie, if any.
    pub step_index: Option<u32>,
    pub drone_phase: DronePhase,
}

/// Observed 3D position. Distinct from [`Waypoint`] because it has no yaw
/// field — it's where the drone *is*, not where it should go.
///
/// [`Waypoint`]: crate::sortie::Waypoint
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Attitude {
    pub roll_deg: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpsFixType {
    None,
    Fix2d,
    Fix3d,
    RtkFloat,
    RtkFixed,
}

/// The drone's high-level lifecycle phase. Updated by legion's executor and
/// safety loop, surfaced in every `Telemetry` frame so oracle's fleet view is
/// always current.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DronePhase {
    Idle,
    Armed,
    InAir,
    ExecutingStep,
    Holding,
    Landing,
}

/// Per-sortie events legion publishes alongside `StepComplete` for finer-
/// grained progress reporting (spray segment boundaries, etc.).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortieEventKind {
    Started,
    SprayOn,
    SprayOff,
    SegmentDone,
    Completed,
    Failed,
}

/// Reason a `SafetyEvent` was raised by legion's local safety loop.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyEventKind {
    /// Forward ToF reading dropped below the configured minimum.
    TofAvoidance,
    /// Battery percentage dropped below the configured critical level.
    BatteryCritical,
    /// Paint level dropped below the configured minimum.
    PaintEmpty,
    /// Oracle has not been heard from for `oracle_silent_s`.
    OracleSilent,
}

