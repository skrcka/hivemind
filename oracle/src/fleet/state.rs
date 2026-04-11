//! In-memory live fleet state — keyed by `drone_id`, updated on every
//! incoming Telemetry frame, queried by the API and the slicer.

use std::collections::HashMap;
use std::sync::Arc;

use hivemind_protocol::{Telemetry, Position, DronePhase};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct DroneStateSnapshot {
    pub last_telemetry: Option<Telemetry>,
    pub is_stale: bool,
}

impl DroneStateSnapshot {
    pub fn position(&self) -> Option<Position> {
        self.last_telemetry.as_ref().map(|t| t.position)
    }

    pub fn phase(&self) -> DronePhase {
        self.last_telemetry
            .as_ref()
            .map(|t| t.drone_phase)
            .unwrap_or_default()
    }
}

/// Cheap-cloneable handle to the live fleet state.
#[derive(Clone, Default)]
pub struct FleetState {
    inner: Arc<RwLock<HashMap<String, DroneStateSnapshot>>>,
}

impl FleetState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record_telemetry(&self, drone_id: &str, t: Telemetry) {
        let mut g = self.inner.write().await;
        let entry = g.entry(drone_id.to_string()).or_default();
        entry.last_telemetry = Some(t);
        entry.is_stale = false;
    }

    pub async fn mark_stale(&self, drone_id: &str) {
        let mut g = self.inner.write().await;
        if let Some(entry) = g.get_mut(drone_id) {
            entry.is_stale = true;
        }
    }

    pub async fn snapshot(&self) -> Vec<(String, DroneStateSnapshot)> {
        let g = self.inner.read().await;
        g.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub async fn snapshot_positions(&self) -> Vec<(String, Position)> {
        let g = self.inner.read().await;
        g.iter()
            .filter_map(|(k, v)| v.position().map(|p| (k.clone(), p)))
            .collect()
    }
}
