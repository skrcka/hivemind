//! Layer 2 deconfliction monitor. Ticks at the configured rate (5 Hz by
//! default) and would normally check pairwise distances + issue holds. v1
//! has one drone, so the conflict-detection function returns empty and
//! the loop is a no-op tick.

use std::sync::Arc;
use std::time::Duration;

use tracing::debug;

use crate::config::SafetyConfig;

use super::state::FleetState;

/// Spawn the fleet monitor task. Returns immediately; the task lives for
/// the lifetime of the process.
pub fn spawn(state: FleetState, cfg: &SafetyConfig) -> tokio::task::JoinHandle<()> {
    let interval_ms = if cfg.fleet_monitor_hz == 0 {
        200
    } else {
        1000 / cfg.fleet_monitor_hz.max(1)
    };
    let min_safe_distance_m = cfg.min_safe_distance_m;
    let arc_state = Arc::new(state);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(u64::from(interval_ms)));
        loop {
            tick.tick().await;
            let positions = arc_state.snapshot_positions().await;
            let conflicts = detect_conflicts(&positions, min_safe_distance_m);
            if !conflicts.is_empty() {
                debug!(conflict_count = conflicts.len(), "fleet monitor detected conflicts (v1: no resolver)");
            }
        }
    })
}

/// Stub conflict detector. Returns empty for any input where there are
/// fewer than 2 drones (the v1 case). v2 will replace this with the actual
/// pairwise distance check.
pub fn detect_conflicts(
    positions: &[(String, hivemind_protocol::Position)],
    _min_distance_m: f32,
) -> Vec<(String, String)> {
    if positions.len() < 2 {
        return Vec::new();
    }
    // v2 deliverable.
    Vec::new()
}
