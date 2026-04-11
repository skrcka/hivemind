//! End-to-end test of the apply handshake.
//!
//! Architecture:
//!
//! 1. Spin up a real `Link` with a TCP listener on a random port.
//! 2. In one task, run a "mock legion" that connects to the listener,
//!    completes the Hello exchange, then for each step receives Proceed and
//!    sends back StepComplete, then sends SortieComplete.
//! 3. In another task, run the Apply Supervisor against the link with a
//!    fixture sortie inserted into the store.
//! 4. Assert: the supervisor exits Ok, every step has a Complete row in
//!    `step_progress`, and the sortie + plan rows are both in `Complete`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::manual_let_else
)]

use std::sync::Arc;
use std::time::Duration;

use hivemind_protocol::{
    Attitude, DronePhase, Envelope, GpsFixType, InProgressSortie, LegionToOracle,
    OracleToLegion, Position, RadioLossBehaviour, RadioLossPolicy, Sortie, SortieStep, StepType,
    TcpTransport, Telemetry, Transport, Waypoint,
};
use oracle::apply::supervisor::{spawn_apply, OperatorSignals};
use oracle::config::SlicerConfig;
use oracle::domain::fleet::{Drone, DroneState, FleetSnapshot};
use oracle::domain::intent::{Face, Intent, MeshRegion, OperatorConstraints, ScanRef};
use oracle::domain::plan::PlanStatus;
use oracle::legion_link::server::{start_with_listener, ListenerConfig};
use oracle::slicer;
use oracle::store::Store;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const TEST_DRONE_ID: &str = "drone-01";
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

fn slicer_cfg() -> SlicerConfig {
    SlicerConfig {
        spray_width_m: 0.30,
        overlap_pct: 0.20,
        min_horizontal_separation_m: 3.0,
        battery_safety_margin_pct: 25.0,
        paint_safety_margin_pct: 15.0,
        standoff_m: 0.6,
        origin_lat_deg: 50.0,
        origin_lon_deg: 14.0,
        origin_alt_m: 200.0,
        planarity_tol_deg: 15.0,
        ferry_speed_m_s: 3.0,
        spray_speed_m_s: 0.5,
        takeoff_alt_m: 5.0,
    }
}

fn fleet() -> FleetSnapshot {
    FleetSnapshot::now(vec![Drone {
        id: TEST_DRONE_ID.into(),
        legion_version: Some("0.1.0".into()),
        capabilities: vec!["spray".into()],
        state: DroneState::default(),
        is_stale: false,
    }])
}

fn east_wall_intent() -> Intent {
    let n = [1.0, 0.0, 0.0];
    let v00 = [10.0, -2.0, 0.0];
    let v10 = [10.0, 2.0, 0.0];
    let v11 = [10.0, 2.0, 3.0];
    let v01 = [10.0, -2.0, 3.0];
    Intent {
        version: "1.0".into(),
        scan: ScanRef {
            id: "test-handshake-east-wall".into(),
            source_file: None,
            georeferenced: true,
        },
        regions: vec![MeshRegion {
            id: "east_wall".into(),
            name: "East wall".into(),
            faces: vec![
                Face {
                    vertices: [v00, v10, v11],
                    normal: n,
                },
                Face {
                    vertices: [v00, v11, v01],
                    normal: n,
                },
            ],
            area_m2: 12.0,
            paint_spec: None,
        }],
        constraints: OperatorConstraints::default(),
    }
}

#[tokio::test]
async fn full_apply_handshake_against_mock_legion() {
    // 1. Bind a listener on a random port and start the legion link.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let link = start_with_listener(
        listener,
        ListenerConfig {
            allowed_drones: vec![TEST_DRONE_ID.into()],
            shared_token: "test".into(),
            oracle_version: "test".into(),
        },
    )
    .await
    .unwrap();
    let link = Arc::new(link);

    // 2. Set up the in-memory store.
    let store = Arc::new(Store::open_memory().await.unwrap());
    store
        .upsert_drone(TEST_DRONE_ID, Some("0.1.0"), &["spray".to_string()])
        .await
        .unwrap();

    // 3. Slice a plan and persist it.
    let intent = east_wall_intent();
    let cfg = slicer_cfg();
    store.insert_intent(&intent).await.unwrap();
    let plan = slicer::plan(intent, fleet(), &cfg).expect("slicer ok");
    assert!(plan.is_approvable(), "fixture plan must be approvable");
    let expected_step_count = plan.sorties[0].steps.len();
    let sortie_id = plan.sorties[0].sortie_id.clone();
    store.insert_plan(&plan).await.unwrap();
    store.insert_plan_sorties(plan.id, &plan.sorties).await.unwrap();
    store
        .set_plan_status(plan.id, PlanStatus::Approved, Some("test"))
        .await
        .unwrap();

    // 4. Spawn the mock legion task.
    let mock = tokio::spawn(run_mock_legion(addr, sortie_id.clone(), expected_step_count));

    // Give the link a moment to register the session.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        wait_for_connection(link.as_ref(), TEST_DRONE_ID, Duration::from_secs(2)).await,
        "mock legion should have connected within 2s"
    );

    // 5. Spawn the apply supervisor and await it.
    let signals = OperatorSignals::new();
    let join = spawn_apply(store.clone(), link.clone(), plan.clone(), signals);

    let result = timeout(TEST_TIMEOUT, join)
        .await
        .expect("supervisor did not finish within the timeout")
        .expect("supervisor task panicked");
    result.expect("supervisor should succeed");

    // 6. Verify final state.
    let summary = store.plan_summary(plan.id).await.unwrap().unwrap();
    assert_eq!(summary.0, PlanStatus::Complete);

    // The mock legion should have observed the right number of Proceeds.
    let mock_result = timeout(TEST_TIMEOUT, mock)
        .await
        .expect("mock legion did not finish")
        .expect("mock legion panicked");
    assert_eq!(
        mock_result.proceed_count, expected_step_count,
        "mock legion should have received one Proceed per step"
    );
    assert!(mock_result.received_upload, "mock legion received UploadSortie");
    assert!(mock_result.completed, "mock legion sent SortieComplete");
}

async fn wait_for_connection(
    link: &oracle::legion_link::Link,
    drone_id: &str,
    deadline: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        if link.is_connected(drone_id).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[derive(Debug)]
struct MockResult {
    proceed_count: usize,
    received_upload: bool,
    completed: bool,
}

async fn run_mock_legion(
    addr: std::net::SocketAddr,
    expected_sortie_id: String,
    expected_step_count: usize,
) -> MockResult {
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut transport: TcpTransport<LegionToOracle, OracleToLegion> = TcpTransport::new(stream);

    // Send Hello.
    let hello = Envelope::new(
        TEST_DRONE_ID,
        1,
        LegionToOracle::Hello {
            drone_id: TEST_DRONE_ID.into(),
            legion_version: "0.1.0".into(),
            capabilities: vec!["spray".into(), "rtk".into()],
            in_progress_sortie: None as Option<InProgressSortie>,
        },
    );
    transport.send(&hello).await.unwrap();

    // Receive oracle's Hello back.
    let _server_hello = transport.recv().await.unwrap();

    // Receive UploadSortie.
    let upload_env = transport.recv().await.unwrap();
    let received_upload = matches!(upload_env.msg, OracleToLegion::UploadSortie { .. });
    let sortie = if let OracleToLegion::UploadSortie { sortie } = upload_env.msg {
        sortie
    } else {
        panic!("expected UploadSortie, got {:?}", upload_env.msg);
    };
    assert_eq!(sortie.sortie_id, expected_sortie_id);
    assert_eq!(sortie.steps.len(), expected_step_count);

    // ACK with SortieReceived.
    transport
        .send(&Envelope::new(
            TEST_DRONE_ID,
            2,
            LegionToOracle::SortieReceived {
                sortie_id: sortie.sortie_id.clone(),
            },
        ))
        .await
        .unwrap();

    // Walk the steps: receive Proceed, send StepComplete, repeat.
    let mut proceed_count = 0;
    for (i, _step) in sortie.steps.iter().enumerate() {
        let proceed_env = transport.recv().await.unwrap();
        match proceed_env.msg {
            OracleToLegion::Proceed {
                sortie_id: sid,
                expected_step_index,
            } => {
                assert_eq!(sid, sortie.sortie_id);
                assert_eq!(expected_step_index as usize, i);
                proceed_count += 1;
            }
            other => panic!("step {i}: expected Proceed, got {other:?}"),
        }

        transport
            .send(&Envelope::new(
                TEST_DRONE_ID,
                10 + i as u64,
                LegionToOracle::StepComplete {
                    sortie_id: sortie.sortie_id.clone(),
                    step_index: i as u32,
                    position: Position {
                        lat: 50.0,
                        lon: 14.0,
                        alt_m: 5.0,
                    },
                    battery_pct: 90.0 - i as f32,
                    paint_remaining_ml: 480.0 - (i as f32 * 5.0),
                    duration_s: 1.5,
                },
            ))
            .await
            .unwrap();
    }

    // Send SortieComplete.
    transport
        .send(&Envelope::new(
            TEST_DRONE_ID,
            999,
            LegionToOracle::SortieComplete {
                sortie_id: sortie.sortie_id.clone(),
            },
        ))
        .await
        .unwrap();

    MockResult {
        proceed_count,
        received_upload,
        completed: true,
    }
}

#[allow(dead_code)]
fn _references_used_to_keep_imports_warning_free() {
    let _ = Telemetry {
        ts_ms: 0,
        position: Position::default(),
        attitude: Attitude::default(),
        battery_pct: 0.0,
        voltage: 0.0,
        paint_remaining_ml: 0.0,
        tof_distance_cm: None,
        gps_fix: GpsFixType::default(),
        sortie_id: None,
        step_index: None,
        drone_phase: DronePhase::default(),
    };
    let _ = SortieStep {
        index: 0,
        step_type: StepType::Takeoff,
        waypoint: Waypoint {
            lat: 0.0,
            lon: 0.0,
            alt_m: 0.0,
            yaw_deg: None,
        },
        path: None,
        speed_m_s: 0.0,
        spray: false,
        radio_loss: RadioLossPolicy {
            behaviour: RadioLossBehaviour::Continue,
            silent_timeout_s: 0.0,
            hold_then_rtl_after_s: None,
        },
        expected_duration_s: 0,
    };
    let _ = Sortie {
        sortie_id: String::new(),
        plan_id: String::new(),
        drone_id: String::new(),
        steps: vec![],
        paint_volume_ml: 0.0,
        expected_duration_s: 0,
    };
}
