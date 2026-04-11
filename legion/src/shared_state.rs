//! `LegionState` wrapped in an `Arc<RwLock<_>>` so the safety loop,
//! executor, comms client, and telemetry pumper can all observe and
//! update it without fighting over ownership.

use std::sync::Arc;

use legion_core::LegionState;
use tokio::sync::RwLock;

/// Thin alias — just the lock primitive. Use `.read().await` /
/// `.write().await` to access.
pub type SharedState = Arc<RwLock<LegionState>>;

pub fn new(drone_id: impl Into<String>) -> SharedState {
    Arc::new(RwLock::new(LegionState::new(drone_id)))
}
