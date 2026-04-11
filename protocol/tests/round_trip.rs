//! Round-trip tests: encode → strip delimiter → decode → assert equal.
//!
//! Covers every variant in `OracleToLegion` and `LegionToOracle`. If a new
//! variant is added, add a corresponding test here.

use hivemind_protocol::{
    decode_frame, encode_frame, Attitude, DronePhase, Envelope, GpsFixType, InProgressSortie,
    LegionToOracle, OracleToLegion, Position, RadioLossBehaviour, RadioLossPolicy,
    SafetyEventKind, Sortie, SortieStep, StepType, Telemetry, Waypoint,
};

const TS_MS: u64 = 1_700_000_000_000;

fn round_trip_oracle(msg: OracleToLegion) {
    let env = Envelope::new("drone-01", TS_MS, msg);
    let frame = encode_frame(&env).expect("encode");
    assert_eq!(frame.last(), Some(&0), "frame must end in 0x00 delimiter");
    let body = &frame[..frame.len() - 1];
    let decoded: Envelope<OracleToLegion> = decode_frame(body).expect("decode");
    assert_eq!(env, decoded);
}

fn round_trip_legion(msg: LegionToOracle) {
    let env = Envelope::new("drone-01", TS_MS, msg);
    let frame = encode_frame(&env).expect("encode");
    assert_eq!(frame.last(), Some(&0), "frame must end in 0x00 delimiter");
    let body = &frame[..frame.len() - 1];
    let decoded: Envelope<LegionToOracle> = decode_frame(body).expect("decode");
    assert_eq!(env, decoded);
}

// ─── OracleToLegion ────────────────────────────────────────────────

#[test]
fn oracle_hello() {
    round_trip_oracle(OracleToLegion::Hello {
        oracle_version: "0.1.0".into(),
        server_time_ms: TS_MS,
    });
}

#[test]
fn oracle_heartbeat() {
    round_trip_oracle(OracleToLegion::Heartbeat);
}

#[test]
fn oracle_upload_sortie() {
    round_trip_oracle(OracleToLegion::UploadSortie {
        sortie: sample_sortie(),
    });
}

#[test]
fn oracle_proceed() {
    round_trip_oracle(OracleToLegion::Proceed {
        sortie_id: "sortie-1".into(),
        expected_step_index: 3,
    });
}

#[test]
fn oracle_hold_step() {
    round_trip_oracle(OracleToLegion::HoldStep {
        sortie_id: "sortie-1".into(),
        reason: "operator paused".into(),
    });
}

#[test]
fn oracle_abort_sortie() {
    round_trip_oracle(OracleToLegion::AbortSortie {
        sortie_id: "sortie-1".into(),
        reason: "weather window closed".into(),
    });
}

#[test]
fn oracle_return_to_base() {
    round_trip_oracle(OracleToLegion::ReturnToBase {
        reason: "operator hard abort".into(),
    });
}

#[test]
fn oracle_cancel_sortie() {
    round_trip_oracle(OracleToLegion::CancelSortie {
        sortie_id: "sortie-1".into(),
    });
}

#[test]
fn oracle_rtk_correction() {
    round_trip_oracle(OracleToLegion::RtkCorrection {
        payload: vec![0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x00, 0xFF],
    });
}

// ─── LegionToOracle ────────────────────────────────────────────────

#[test]
fn legion_hello_no_in_progress() {
    round_trip_legion(LegionToOracle::Hello {
        drone_id: "drone-01".into(),
        legion_version: "0.1.0".into(),
        capabilities: vec!["spray".into(), "rtk".into(), "tof".into()],
        in_progress_sortie: None,
    });
}

#[test]
fn legion_hello_with_in_progress() {
    round_trip_legion(LegionToOracle::Hello {
        drone_id: "drone-01".into(),
        legion_version: "0.1.0".into(),
        capabilities: vec!["spray".into()],
        in_progress_sortie: Some(InProgressSortie {
            sortie_id: "sortie-1".into(),
            last_completed_step: Some(2),
        }),
    });
}

#[test]
fn legion_heartbeat() {
    round_trip_legion(LegionToOracle::Heartbeat);
}

#[test]
fn legion_telemetry() {
    round_trip_legion(LegionToOracle::Telemetry(sample_telemetry()));
}

#[test]
fn legion_telemetry_no_active_sortie() {
    let mut t = sample_telemetry();
    t.sortie_id = None;
    t.step_index = None;
    t.tof_distance_cm = None;
    t.drone_phase = DronePhase::Idle;
    round_trip_legion(LegionToOracle::Telemetry(t));
}

#[test]
fn legion_sortie_received() {
    round_trip_legion(LegionToOracle::SortieReceived {
        sortie_id: "sortie-1".into(),
    });
}

#[test]
fn legion_step_complete() {
    round_trip_legion(LegionToOracle::StepComplete {
        sortie_id: "sortie-1".into(),
        step_index: 2,
        position: Position {
            lat: 50.0,
            lon: 14.0,
            alt_m: 5.0,
        },
        battery_pct: 87.5,
        paint_remaining_ml: 480.0,
        duration_s: 47.3,
    });
}

#[test]
fn legion_sortie_complete() {
    round_trip_legion(LegionToOracle::SortieComplete {
        sortie_id: "sortie-1".into(),
    });
}

#[test]
fn legion_sortie_failed() {
    round_trip_legion(LegionToOracle::SortieFailed {
        sortie_id: "sortie-1".into(),
        step_index: 4,
        reason: "tof_avoidance during spray pass".into(),
    });
}

#[test]
fn legion_safety_event() {
    round_trip_legion(LegionToOracle::SafetyEvent {
        kind: SafetyEventKind::TofAvoidance,
        action: "emergency_pullback".into(),
        detail: "tof=22cm".into(),
    });
}

#[test]
fn legion_held() {
    round_trip_legion(LegionToOracle::Held {
        sortie_id: "sortie-1".into(),
        step_index: 3,
        reason: "fleet_conflict_with_drone-02".into(),
    });
}

#[test]
fn legion_error() {
    round_trip_legion(LegionToOracle::Error {
        code: "version_mismatch".into(),
        message: "expected 1, got 2".into(),
    });
}

// ─── Helpers ────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn sample_sortie() -> Sortie {
    Sortie {
        sortie_id: "sortie-1".into(),
        plan_id: "plan-1".into(),
        drone_id: "drone-01".into(),
        steps: vec![
            SortieStep {
                index: 0,
                step_type: StepType::Takeoff,
                waypoint: Waypoint {
                    lat: 50.0,
                    lon: 14.0,
                    alt_m: 5.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 1.0,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::HoldThenRtl,
                    silent_timeout_s: 5.0,
                    hold_then_rtl_after_s: Some(10.0),
                },
                expected_duration_s: 5,
            },
            SortieStep {
                index: 1,
                step_type: StepType::Transit,
                waypoint: Waypoint {
                    lat: 50.001,
                    lon: 14.001,
                    alt_m: 5.0,
                    yaw_deg: Some(90.0),
                },
                path: None,
                speed_m_s: 3.0,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 30.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 20,
            },
            SortieStep {
                index: 2,
                step_type: StepType::SprayPass,
                waypoint: Waypoint {
                    lat: 50.001,
                    lon: 14.001,
                    alt_m: 5.0,
                    yaw_deg: Some(90.0),
                },
                path: Some(vec![
                    Waypoint {
                        lat: 50.001,
                        lon: 14.001,
                        alt_m: 5.0,
                        yaw_deg: Some(90.0),
                    },
                    Waypoint {
                        lat: 50.002,
                        lon: 14.001,
                        alt_m: 5.0,
                        yaw_deg: Some(90.0),
                    },
                ]),
                speed_m_s: 0.5,
                spray: true,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 60.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 30,
            },
            SortieStep {
                index: 3,
                step_type: StepType::ReturnToBase,
                waypoint: Waypoint {
                    lat: 50.0,
                    lon: 14.0,
                    alt_m: 10.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 4.0,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 30.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 25,
            },
            SortieStep {
                index: 4,
                step_type: StepType::Land,
                waypoint: Waypoint {
                    lat: 50.0,
                    lon: 14.0,
                    alt_m: 0.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 0.5,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 30.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 10,
            },
        ],
        paint_volume_ml: 500.0,
        expected_duration_s: 90,
    }
}

fn sample_telemetry() -> Telemetry {
    Telemetry {
        ts_ms: TS_MS,
        position: Position {
            lat: 50.0,
            lon: 14.0,
            alt_m: 5.0,
        },
        attitude: Attitude {
            roll_deg: 0.0,
            pitch_deg: 0.0,
            yaw_deg: 90.0,
        },
        battery_pct: 87.5,
        voltage: 16.4,
        paint_remaining_ml: 480.0,
        tof_distance_cm: Some(45.0),
        gps_fix: GpsFixType::RtkFixed,
        sortie_id: Some("sortie-1".into()),
        step_index: Some(2),
        drone_phase: DronePhase::ExecutingStep,
    }
}
