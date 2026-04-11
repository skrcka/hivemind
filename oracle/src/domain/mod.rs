//! Oracle-side domain types: Intent (the pantheon contract), HivemindPlan
//! (the slicer's output), FleetSnapshot (the slicer's input), and amendments.
//!
//! Note that the wire types `Sortie`, `SortieStep`, `RadioLossPolicy`, etc.
//! live in the `hivemind-protocol` crate, not here. The slicer produces them
//! directly so the Apply Supervisor can ship them to legion without conversion.

pub mod amendment;
pub mod fleet;
pub mod intent;
pub mod plan;

pub use amendment::PlanAmendment;
pub use fleet::{Drone, DroneState, FleetSnapshot};
pub use intent::{Face, Intent, MeshRegion, OperatorConstraints, PaintSpec, ScanRef};
pub use plan::{
    CoveragePlan, FleetSchedule, HivemindPlan, PlanError, PlanErrorCode, PlanId, PlanStatus,
    PlanWarning, PlanWarningCode, PlanWarningSeverity, ResourceEstimate,
};
