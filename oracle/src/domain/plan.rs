//! HivemindPlan — the slicer's frozen output. A plan is a complete,
//! inspectable description of everything that will happen.

use hivemind_protocol::Sortie;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::fleet::FleetSnapshot;
use super::intent::Intent;

/// A plan id. Backed by a UUID v7 so it's sortable by creation time and
/// monotonic across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanId(pub Uuid);

impl PlanId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for PlanId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PlanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PlanId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// The slicer's frozen output. Persisted as JSON in the `plans.body` column;
/// indexed summary metrics live in dedicated columns alongside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivemindPlan {
    pub id: PlanId,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub status: PlanStatus,

    pub intent: Intent,
    pub coverage: CoveragePlan,
    pub sorties: Vec<Sortie>,
    pub schedule: FleetSchedule,
    pub resources: ResourceEstimate,

    pub warnings: Vec<PlanWarning>,
    pub errors: Vec<PlanError>,

    /// The slicer's input snapshot, frozen so plans are reproducible.
    pub fleet_snapshot: FleetSnapshot,
}

impl HivemindPlan {
    /// `true` if the plan is fit for `Approve` (no errors).
    pub fn is_approvable(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    Draft,
    Proposed,
    Approved,
    Executing,
    Paused,
    Aborted,
    Complete,
    Failed,
}

impl PlanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Proposed => "Proposed",
            Self::Approved => "Approved",
            Self::Executing => "Executing",
            Self::Paused => "Paused",
            Self::Aborted => "Aborted",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
        }
    }
}

impl std::str::FromStr for PlanStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Draft" => Ok(Self::Draft),
            "Proposed" => Ok(Self::Proposed),
            "Approved" => Ok(Self::Approved),
            "Executing" => Ok(Self::Executing),
            "Paused" => Ok(Self::Paused),
            "Aborted" => Ok(Self::Aborted),
            "Complete" => Ok(Self::Complete),
            "Failed" => Ok(Self::Failed),
            other => Err(format!("unknown plan status: {other}")),
        }
    }
}

/// Surface coverage summary computed by the slicer's coverage stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoveragePlan {
    pub total_area_m2: f64,
    pub overlap_pct: f32,
    pub estimated_coats: u32,
    pub pass_count: u32,
}

/// Wall-clock view of the plan's execution timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSchedule {
    pub total_sorties: u32,
    pub total_duration_s: u64,
    pub peak_concurrent_drones: u32,
    pub refill_cycles: u32,
}

/// Resource consumption estimate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEstimate {
    pub paint_ml: f64,
    pub battery_cycles: u32,
    pub total_flight_time_s: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanWarningSeverity {
    Info,
    Warn,
    Critical,
}

impl PlanWarningSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanWarningCode {
    LongDuration,
    NarrowWeatherWindow,
    LowPaintMargin,
}

impl PlanWarningCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LongDuration => "long_duration",
            Self::NarrowWeatherWindow => "narrow_weather_window",
            Self::LowPaintMargin => "low_paint_margin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanWarning {
    pub severity: PlanWarningSeverity,
    pub code: PlanWarningCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanErrorCode {
    NonPlanarRegion,
    NoRegions,
    NotGeoreferenced,
    NoDronesAvailable,
    UnreachableRegion,
}

impl PlanErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonPlanarRegion => "non_planar_region",
            Self::NoRegions => "no_regions",
            Self::NotGeoreferenced => "not_georeferenced",
            Self::NoDronesAvailable => "no_drones_available",
            Self::UnreachableRegion => "unreachable_region",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanError {
    pub code: PlanErrorCode,
    pub message: String,
    /// Region id this error refers to, if applicable.
    pub region_id: Option<String>,
}
