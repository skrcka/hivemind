//! The single-tick safety check. Called once per 10 Hz loop iteration
//! by the hosting binary's wrapper.
//!
//! The function is pure aside from the side effects of the hardware
//! trait calls it makes (cutting the pump, sending `emergency_pullback`,
//! reading sensors). It returns a [`SafetyOutcome`] describing what
//! happened; the binary turns that into a `SafetyState` on the watch
//! channel and into a `SafetyEvent` frame out to oracle.

use alloc::string::String;
use alloc::string::ToString;

use crate::error::PayloadError;
use crate::safety::{SafetyConfig, SafetyState};
use crate::state::LegionState;
use crate::traits::{Clock, MavlinkBackend, PaintLevel, Payload, Pump, Tof};

/// What the single-tick check decided.
#[derive(Debug, Clone, PartialEq)]
pub enum SafetyOutcome {
    /// Nothing to do this tick.
    Ok,
    /// A trip occurred. The corresponding side effects have already been
    /// issued to the hardware (pump off, emergency pullback, RTL, etc.);
    /// the caller should publish the new state and emit a
    /// `SafetyEvent` to oracle.
    Tripped {
        state: SafetyState,
        /// Short, machine-stable identifier for the reaction taken. Used
        /// as the `action` field of the outbound `SafetyEvent` frame.
        action: &'static str,
    },
}

impl SafetyOutcome {
    pub fn tripped(state: SafetyState, action: &'static str) -> Self {
        Self::Tripped { state, action }
    }
}

/// Run one tick of the safety check.
///
/// Priority order (first hit wins): ToF → battery → paint → oracle silent.
/// The caller is expected to feed `state` with fresh values for
/// `battery_pct`, `paint_remaining_ml`, and `last_oracle_contact_ms`
/// before calling — the check reads sensors directly only for ToF and
/// paint, since battery comes from the MAVLink decoder upstream.
pub async fn safety_check<P, M, C>(
    payload: &mut P,
    mavlink: &M,
    clock: &C,
    state: &mut LegionState,
    cfg: &SafetyConfig,
) -> SafetyOutcome
where
    P: Payload,
    M: MavlinkBackend,
    C: Clock,
{
    // 1. ToF wall avoidance. Read directly — this is the hottest sensor.
    match payload.tof().read_cm().await {
        Ok(tof) => {
            state.tof_distance_cm = Some(tof);
            if tof < cfg.tof_min_cm {
                let _ = payload.pump().off().await;
                let _ = mavlink.emergency_pullback().await;
                return SafetyOutcome::tripped(
                    SafetyState::TofAvoidance { tof_cm: tof },
                    "emergency_pullback",
                );
            }
        }
        Err(PayloadError::NotInstalled) => {
            // No ToF on this drone. Skip the check.
        }
        Err(e) => {
            return SafetyOutcome::tripped(
                SafetyState::SensorError {
                    detail: "tof: ".to_string() + &e.to_string(),
                },
                "sensor_error_tof",
            );
        }
    }

    // 2. Battery critical. Use the MAVLink decoder's cached value, kept
    //    fresh by the telemetry task and copied into state.
    let battery = mavlink.battery_pct();
    state.battery_pct = battery;
    if battery > 0.0 && battery < cfg.battery_critical_pct {
        let _ = payload.pump().off().await;
        let _ = mavlink.return_to_launch().await;
        return SafetyOutcome::tripped(
            SafetyState::BatteryCritical {
                battery_pct: battery,
            },
            "return_to_launch",
        );
    }

    // 3. Paint empty. Read directly.
    match payload.paint_level().read_ml().await {
        Ok(paint) => {
            state.paint_remaining_ml = paint;
            if paint < cfg.paint_empty_ml {
                let _ = payload.pump().off().await;
                let _ = mavlink.return_to_launch().await;
                return SafetyOutcome::tripped(
                    SafetyState::PaintEmpty { paint_ml: paint },
                    "return_to_launch",
                );
            }
        }
        Err(PayloadError::NotInstalled) => {}
        Err(e) => {
            return SafetyOutcome::tripped(
                SafetyState::SensorError {
                    detail: "paint_level: ".to_string() + &e.to_string(),
                },
                "sensor_error_paint_level",
            );
        }
    }

    // 4. Oracle silent. Cut the pump only — the executor's per-step
    //    radio-loss policy decides the flight outcome.
    let silence = clock.elapsed_ms(state.last_oracle_contact_ms);
    if silence > cfg.oracle_silent_ms {
        let _ = payload.pump().off().await;
        return SafetyOutcome::tripped(
            SafetyState::OracleSilent {
                silence_ms: silence,
            },
            "pump_off",
        );
    }

    SafetyOutcome::Ok
}

/// Convenience: turn a [`SafetyState`] into the
/// `hivemind_protocol::SafetyEventKind` and human detail the Pi binary
/// needs to emit an outbound `SafetyEvent` frame.
pub fn outbound_event_fields(
    state: &SafetyState,
) -> Option<(hivemind_protocol::SafetyEventKind, String)> {
    use hivemind_protocol::SafetyEventKind as K;
    let (kind, detail) = match state {
        SafetyState::Ok => return None,
        SafetyState::TofAvoidance { tof_cm } => (K::TofAvoidance, fmt_tof(*tof_cm)),
        SafetyState::BatteryCritical { battery_pct } => {
            (K::BatteryCritical, fmt_battery(*battery_pct))
        }
        SafetyState::PaintEmpty { paint_ml } => (K::PaintEmpty, fmt_paint(*paint_ml)),
        SafetyState::OracleSilent { silence_ms } => (K::OracleSilent, fmt_silence(*silence_ms)),
        SafetyState::SensorError { detail } => return Some((K::TofAvoidance, detail.clone())),
    };
    Some((kind, detail))
}

fn fmt_tof(cm: f32) -> String {
    // Manual rather than `format!` because no_std + alloc lacks `format!`
    // on older toolchains. We do have it on 1.75+; using it is fine.
    alloc::format!("tof={cm:.0}cm")
}

fn fmt_battery(pct: f32) -> String {
    alloc::format!("battery={pct:.0}%")
}

fn fmt_paint(ml: f32) -> String {
    alloc::format!("paint={ml:.0}ml")
}

fn fmt_silence(ms: u64) -> String {
    alloc::format!("silence={ms}ms")
}
