//! Core drone-side state. The Pi binary wraps this in
//! `Arc<tokio::sync::RwLock<LegionState>>`; an MCU binary would wrap it in
//! `embassy_sync::blocking_mutex::CriticalSectionMutex`. Either way, the
//! bare struct lives here so both ends share it.

use alloc::string::String;
use hivemind_protocol::{Attitude, DronePhase, GpsFixType, Position, Sortie};

/// Everything legion knows about *itself* at any moment. Updated by the
/// MAVLink telemetry decoder, the payload sensor polling, the executor,
/// and the safety loop.
///
/// The timing fields are expressed as milliseconds since boot (from the
/// [`crate::Clock`] trait) — this is deliberately *not* `std::time::Instant`
/// so the type works on an MCU with no wall clock.
#[derive(Debug, Clone)]
pub struct LegionState {
    /// The sortie currently loaded into memory. `Some` once oracle has
    /// sent `UploadSortie` *and* legion has validated + persisted it.
    /// Cleared on `SortieComplete`, `SortieFailed`, or `CancelSortie`.
    pub current_sortie: Option<Sortie>,
    /// 0-based step index into `current_sortie.steps`. Meaningful only
    /// when `current_sortie.is_some()`.
    pub current_step_index: u32,
    /// The highest step index legion has reported `StepComplete` for
    /// within the current sortie. Persisted to disk via `SortieStore`.
    pub last_completed_step: Option<u32>,
    /// The drone's high-level lifecycle phase. Surfaced in every
    /// outbound `Telemetry` frame so oracle's fleet view is current.
    pub drone_phase: DronePhase,
    /// Last-known position (from the MAVLink `GLOBAL_POSITION_INT` decoder).
    pub position: Position,
    pub attitude: Attitude,
    pub gps_fix: GpsFixType,
    pub battery_pct: f32,
    pub voltage: f32,
    pub paint_remaining_ml: f32,
    /// Last forward-facing ToF reading, in cm. `None` if the sensor is
    /// not installed or has never been read.
    pub tof_distance_cm: Option<f32>,
    /// `Clock::now_ms()` at the last frame received from oracle (any
    /// frame — heartbeat, command, anything). The safety loop watches
    /// this against `SafetyConfig::oracle_silent_ms`.
    pub last_oracle_contact_ms: u64,
    /// The drone's identity, as configured and reported in the initial
    /// `Hello` frame.
    pub drone_id: String,
}

impl LegionState {
    /// Create a fresh state. `drone_id` is the only configuration input.
    pub fn new(drone_id: impl Into<String>) -> Self {
        Self {
            current_sortie: None,
            current_step_index: 0,
            last_completed_step: None,
            drone_phase: DronePhase::Idle,
            position: Position {
                lat: 0.0,
                lon: 0.0,
                alt_m: 0.0,
            },
            attitude: Attitude {
                roll_deg: 0.0,
                pitch_deg: 0.0,
                yaw_deg: 0.0,
            },
            gps_fix: GpsFixType::None,
            battery_pct: 0.0,
            voltage: 0.0,
            paint_remaining_ml: 0.0,
            tof_distance_cm: None,
            last_oracle_contact_ms: 0,
            drone_id: drone_id.into(),
        }
    }

    /// Called by the safety loop when any frame arrives from oracle. Keeps
    /// the watchdog fed.
    pub fn note_oracle_contact(&mut self, now_ms: u64) {
        self.last_oracle_contact_ms = now_ms;
    }
}
