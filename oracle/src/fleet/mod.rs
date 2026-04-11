//! Fleet state + Layer 2 monitor.
//!
//! v1 with one drone has nothing to deconflict, so the monitor task is
//! literally a no-op tick. The architecture is wired in so the v2 multi-drone
//! path doesn't have to add a new task — it just fills in `detect_conflicts`.

pub mod monitor;
pub mod state;

pub use state::FleetState;
