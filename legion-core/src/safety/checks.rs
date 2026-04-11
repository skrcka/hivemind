//! Individual predicate helpers, exposed for unit tests.
//!
//! The full check pipeline lives in `check.rs`; this module just exposes
//! the "is this tripped?" predicates as pure functions so tests can
//! validate the thresholds without mocking the hardware traits.

use crate::safety::SafetyConfig;

pub fn tof_tripped(cfg: &SafetyConfig, tof_cm: f32) -> bool {
    tof_cm < cfg.tof_min_cm
}

pub fn battery_tripped(cfg: &SafetyConfig, battery_pct: f32) -> bool {
    // 0.0 means "unknown / not yet received a BATTERY_STATUS frame". We
    // deliberately don't trip on unknown — the MAVLink heartbeat wait at
    // boot ensures we have a real value before the first step runs.
    battery_pct > 0.0 && battery_pct < cfg.battery_critical_pct
}

pub fn paint_tripped(cfg: &SafetyConfig, paint_ml: f32) -> bool {
    paint_ml < cfg.paint_empty_ml
}

pub fn oracle_silent(cfg: &SafetyConfig, silence_ms: u64) -> bool {
    silence_ms > cfg.oracle_silent_ms
}
