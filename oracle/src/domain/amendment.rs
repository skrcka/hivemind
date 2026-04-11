//! Plan amendments — modifications to a plan that's already been approved
//! and is executing.

use hivemind_protocol::SortieId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::intent::MeshRegion;

/// A modification to an executing plan. Some are oracle-initiated (drone
/// down, weather hold), others are operator-initiated. v1 stubs out the
/// machinery — every variant is persistable but the apply path doesn't yet
/// react to most of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanAmendment {
    /// Oracle-initiated: a drone went stale; remaining sorties were
    /// reassigned.
    DroneDown {
        drone_id: String,
        reassigned_to: Vec<String>,
    },
    /// Oracle-initiated: weather forecast tripped a hold on a set of sorties.
    WeatherHold {
        affected_sorties: Vec<SortieId>,
        #[serde(with = "time::serde::rfc3339")]
        resume_estimate: OffsetDateTime,
    },
    /// Oracle-initiated: a sortie failed; oracle proposes a retry.
    SortieFailed {
        sortie_id: SortieId,
        reason: String,
    },
    /// Operator-initiated: pause the entire plan at the next gate.
    PauseAll,
    /// Operator-initiated: skip a region.
    SkipRegion { region_id: String },
    /// Operator-initiated: hard abort + RTL.
    AbortAndLand,
    /// Operator-initiated: add a new region. Triggers a full replan of
    /// remaining work. Not implemented in v1.
    AddRegion { region: MeshRegion },
}

impl PlanAmendment {
    /// `true` for amendments that change the *intent* and need explicit
    /// operator approval before being applied. `false` for routine
    /// adjustments that oracle handles autonomously.
    pub fn requires_approval(&self) -> bool {
        match self {
            // Oracle-initiated routine adjustments
            Self::DroneDown { .. } | Self::SortieFailed { .. } => false,
            // Anything that changes the intent or stops the swarm
            Self::WeatherHold { .. }
            | Self::PauseAll
            | Self::SkipRegion { .. }
            | Self::AbortAndLand
            | Self::AddRegion { .. } => true,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::DroneDown { .. } => "drone_down",
            Self::WeatherHold { .. } => "weather_hold",
            Self::SortieFailed { .. } => "sortie_failed",
            Self::PauseAll => "pause_all",
            Self::SkipRegion { .. } => "skip_region",
            Self::AbortAndLand => "abort_and_land",
            Self::AddRegion { .. } => "add_region",
        }
    }
}
