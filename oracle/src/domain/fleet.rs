//! Fleet types — the slicer's input snapshot and the live fleet roster.

use hivemind_protocol::{Attitude, DronePhase, GpsFixType, Position};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A point-in-time copy of the fleet, fed into the slicer when a plan is
/// created. Inlined into `HivemindPlan.fleet_snapshot` so plans are
/// reproducible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSnapshot {
    #[serde(with = "time::serde::rfc3339")]
    pub captured_at: OffsetDateTime,
    pub drones: Vec<Drone>,
}

impl Default for FleetSnapshot {
    fn default() -> Self {
        Self {
            captured_at: OffsetDateTime::UNIX_EPOCH,
            drones: Vec::new(),
        }
    }
}

impl FleetSnapshot {
    pub fn now(drones: Vec<Drone>) -> Self {
        Self {
            captured_at: OffsetDateTime::now_utc(),
            drones,
        }
    }

    /// Drones currently online and idle (i.e. eligible for sortie assignment).
    pub fn idle_drones(&self) -> impl Iterator<Item = &Drone> {
        self.drones
            .iter()
            .filter(|d| !d.is_stale && d.state.phase == DronePhase::Idle)
    }
}

/// One drone known to oracle. Combines roster identity (id, capabilities)
/// with current observed state (battery, paint, position).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drone {
    pub id: String,
    pub legion_version: Option<String>,
    pub capabilities: Vec<String>,
    pub state: DroneState,
    pub is_stale: bool,
}

/// Current observed state of a single drone, derived from the latest
/// `Telemetry` frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DroneState {
    pub battery_pct: f32,
    pub paint_remaining_ml: f32,
    pub voltage: f32,
    pub position: Position,
    pub attitude: Attitude,
    pub gps_fix: GpsFixType,
    pub phase: DronePhase,
    pub tof_distance_cm: Option<f32>,
}
