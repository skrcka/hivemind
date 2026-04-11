//! Local safety logic. The *check* is a pure async function over the
//! hardware traits; the *loop* that calls it at 10 Hz lives in the
//! hosting binary (`legion/src/safety_loop.rs` on the Pi). Splitting it
//! this way keeps `tokio::time::interval` and `embassy_time::Timer` out
//! of `legion-core`.
//!
//! The four checks, in priority order:
//!
//! 1. **ToF wall avoidance** — if the forward sensor reports less than
//!    `tof_min_cm`, command `emergency_pullback` and hold.
//! 2. **Battery critical** — if battery drops below `battery_critical_pct`,
//!    cut the pump and RTL.
//! 3. **Paint empty** — if the level sensor reports less than
//!    `paint_empty_ml`, cut the pump and RTL.
//! 4. **Oracle silent** — if the last contact with oracle is older than
//!    `oracle_silent_ms`, cut the pump only (the executor's per-step
//!    radio-loss policy decides the *flight* outcome). This is the
//!    only check that *doesn't* touch the autopilot — two layers, not
//!    redundant.

pub mod check;
pub mod checks;

pub use check::{safety_check, SafetyOutcome};

use alloc::string::String;

/// Configuration for the safety loop. Loaded from `/etc/legion/config.toml`
/// on the Pi, or hardcoded on an MCU.
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    /// Forward ToF minimum clearance, cm. Below this, trip TofAvoidance.
    pub tof_min_cm: f32,
    /// Below this battery percentage, trip BatteryCritical.
    pub battery_critical_pct: f32,
    /// Below this paint level (ml), trip PaintEmpty.
    pub paint_empty_ml: f32,
    /// Above this many milliseconds since the last oracle frame, trip
    /// OracleSilent. The safety loop cuts the pump; the executor's
    /// per-step radio-loss policy decides what to do with the flight.
    pub oracle_silent_ms: u64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            tof_min_cm: 30.0,
            battery_critical_pct: 15.0,
            paint_empty_ml: 20.0,
            oracle_silent_ms: 5_000,
        }
    }
}

/// Published on the watch channel the binary exposes to the executor so
/// that a step-level `tokio::select!` can react to a safety trip without
/// polling. `Ok` is the steady state.
#[derive(Debug, Clone, PartialEq)]
pub enum SafetyState {
    Ok,
    TofAvoidance { tof_cm: f32 },
    BatteryCritical { battery_pct: f32 },
    PaintEmpty { paint_ml: f32 },
    OracleSilent { silence_ms: u64 },
    /// Catch-all for driver/sensor errors the safety loop couldn't
    /// interpret. The binary logs these loudly.
    SensorError { detail: String },
}

impl SafetyState {
    /// Is anything non-`Ok` latched?
    pub fn is_tripped(&self) -> bool {
        !matches!(self, Self::Ok)
    }
}
