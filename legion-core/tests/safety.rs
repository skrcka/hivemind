//! Integration tests for the safety check.
//!
//! Each test builds a `LegionState`, a `MockPayload` with scripted
//! sensor readings, and a `MockMavlink`, then runs
//! `safety::check::safety_check` once and asserts both the returned
//! `SafetyOutcome` and the side effects on the hardware mocks.

mod common;

use common::{FakeClock, MockMavlink, MockPayload, MockPaintLevel, MockTof};
use legion_core::safety::check::safety_check;
use legion_core::safety::{SafetyConfig, SafetyOutcome, SafetyState};
use legion_core::traits::Pump;
use legion_core::{Clock, LegionState};

#[tokio::test]
async fn ok_when_everything_healthy() {
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    mavlink.set_battery(80.0);
    let clock = FakeClock::new();
    let mut state = LegionState::new("drone-01");
    state.last_oracle_contact_ms = clock.now_ms();
    let cfg = SafetyConfig::default();

    let outcome = safety_check(&mut payload, &mavlink, &clock, &mut state, &cfg).await;
    assert_eq!(outcome, SafetyOutcome::Ok);
}

#[tokio::test]
async fn trips_tof_avoidance_and_cuts_pump() {
    let mut payload = MockPayload::healthy();
    payload.tof = MockTof::new([20.0]); // below the 30 cm default
    payload.pump.on = true; // pretend the executor has the pump on
    let mavlink = MockMavlink::new();
    mavlink.set_battery(80.0);
    let clock = FakeClock::new();
    let mut state = LegionState::new("drone-01");
    state.last_oracle_contact_ms = clock.now_ms();
    let cfg = SafetyConfig::default();

    let outcome = safety_check(&mut payload, &mavlink, &clock, &mut state, &cfg).await;

    match outcome {
        SafetyOutcome::Tripped { state, action } => {
            assert!(matches!(state, SafetyState::TofAvoidance { .. }));
            assert_eq!(action, "emergency_pullback");
        }
        _ => panic!("expected TofAvoidance, got {outcome:?}"),
    }

    // Pump was cut, emergency_pullback was called.
    assert!(!payload.pump.is_on());
    use common::MavCall;
    assert!(mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::EmergencyPullback)));
}

#[tokio::test]
async fn trips_battery_critical_and_rtls() {
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    mavlink.set_battery(10.0); // below the 15% default
    let clock = FakeClock::new();
    let mut state = LegionState::new("drone-01");
    state.last_oracle_contact_ms = clock.now_ms();
    let cfg = SafetyConfig::default();

    let outcome = safety_check(&mut payload, &mavlink, &clock, &mut state, &cfg).await;

    match outcome {
        SafetyOutcome::Tripped { state, action } => {
            assert!(matches!(state, SafetyState::BatteryCritical { .. }));
            assert_eq!(action, "return_to_launch");
        }
        _ => panic!("expected BatteryCritical, got {outcome:?}"),
    }
}

#[tokio::test]
async fn trips_paint_empty_and_rtls() {
    let mut payload = MockPayload::healthy();
    payload.paint_level = MockPaintLevel::new([10.0]); // below 20 ml default
    let mavlink = MockMavlink::new();
    mavlink.set_battery(80.0);
    let clock = FakeClock::new();
    let mut state = LegionState::new("drone-01");
    state.last_oracle_contact_ms = clock.now_ms();
    let cfg = SafetyConfig::default();

    let outcome = safety_check(&mut payload, &mavlink, &clock, &mut state, &cfg).await;

    match outcome {
        SafetyOutcome::Tripped { state, action } => {
            assert!(matches!(state, SafetyState::PaintEmpty { .. }));
            assert_eq!(action, "return_to_launch");
        }
        _ => panic!("expected PaintEmpty, got {outcome:?}"),
    }
}

#[tokio::test]
async fn trips_oracle_silent_and_cuts_pump_only() {
    let mut payload = MockPayload::healthy();
    payload.pump.on = true;
    let mavlink = MockMavlink::new();
    mavlink.set_battery(80.0);
    let clock = FakeClock::new();
    clock.set_ms(10_000);
    let mut state = LegionState::new("drone-01");
    // Last contact was at t=0, clock is now at 10000ms → 10s silence
    // vs the 5s default.
    state.last_oracle_contact_ms = 0;
    let cfg = SafetyConfig::default();

    let outcome = safety_check(&mut payload, &mavlink, &clock, &mut state, &cfg).await;

    match outcome {
        SafetyOutcome::Tripped { state, action } => {
            assert!(matches!(state, SafetyState::OracleSilent { .. }));
            assert_eq!(action, "pump_off");
        }
        _ => panic!("expected OracleSilent, got {outcome:?}"),
    }

    // Pump was cut, but no flight command issued — that's the
    // executor's problem, not safety's.
    assert!(!payload.pump.is_on());
    use common::MavCall;
    assert!(!mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::Rtl | MavCall::EmergencyPullback)));
}

#[test]
fn predicates() {
    use legion_core::safety::checks::*;
    let cfg = SafetyConfig::default();
    assert!(tof_tripped(&cfg, 10.0));
    assert!(!tof_tripped(&cfg, 100.0));
    assert!(battery_tripped(&cfg, 10.0));
    assert!(!battery_tripped(&cfg, 50.0));
    // Unknown battery (0.0) does NOT trip — that's a safety-valve
    // against pre-telemetry bootup. Documented in checks.rs.
    assert!(!battery_tripped(&cfg, 0.0));
    assert!(paint_tripped(&cfg, 5.0));
    assert!(!paint_tripped(&cfg, 100.0));
    assert!(oracle_silent(&cfg, 10_000));
    assert!(!oracle_silent(&cfg, 1_000));
}
