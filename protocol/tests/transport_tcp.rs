//! Integration tests for `TcpTransport`. Behind the `tcp` feature.

#![cfg(feature = "tcp")]

use hivemind_protocol::{
    Envelope, InProgressSortie, LegionToOracle, OracleToLegion, TcpTransport, Transport,
};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn hello_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Legion side: accept, recv Hello, send back Hello.
    let legion = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut t: TcpTransport<LegionToOracle, OracleToLegion> = TcpTransport::new(stream);

        let env = t.recv().await.unwrap();
        assert!(matches!(env.msg, OracleToLegion::Hello { .. }));

        let resp = Envelope::new(
            "drone-01",
            1,
            LegionToOracle::Hello {
                drone_id: "drone-01".into(),
                legion_version: "0.1.0".into(),
                capabilities: vec!["spray".into(), "rtk".into()],
                in_progress_sortie: None,
            },
        );
        t.send(&resp).await.unwrap();
    });

    // Oracle side: connect, send Hello, recv Hello.
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut t: TcpTransport<OracleToLegion, LegionToOracle> = TcpTransport::new(stream);

    let hello = Envelope::new(
        "drone-01",
        1,
        OracleToLegion::Hello {
            oracle_version: "0.1.0".into(),
            server_time_ms: 1_700_000_000_000,
        },
    );
    t.send(&hello).await.unwrap();

    let resp = t.recv().await.unwrap();
    match resp.msg {
        LegionToOracle::Hello {
            drone_id,
            capabilities,
            ..
        } => {
            assert_eq!(drone_id, "drone-01");
            assert_eq!(capabilities, vec!["spray", "rtk"]);
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    legion.await.unwrap();
}

#[tokio::test]
async fn many_messages_in_a_row() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let legion = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut t: TcpTransport<LegionToOracle, OracleToLegion> = TcpTransport::new(stream);
        for i in 0..50 {
            let env = t.recv().await.unwrap();
            match env.msg {
                OracleToLegion::Proceed {
                    expected_step_index,
                    ..
                } => {
                    assert_eq!(expected_step_index, i);
                }
                other => panic!("unexpected msg at i={i}: {other:?}"),
            }
        }
    });

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut t: TcpTransport<OracleToLegion, LegionToOracle> = TcpTransport::new(stream);

    for i in 0..50u32 {
        let env = Envelope::new(
            "drone-01",
            u64::from(i),
            OracleToLegion::Proceed {
                sortie_id: "sortie-1".into(),
                expected_step_index: i,
            },
        );
        t.send(&env).await.unwrap();
    }

    legion.await.unwrap();
}

#[tokio::test]
async fn closed_peer_returns_closed_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let legion = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // Drop the stream immediately, simulating a closed peer.
        drop(stream);
    });

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut t: TcpTransport<OracleToLegion, LegionToOracle> = TcpTransport::new(stream);
    legion.await.unwrap();

    // The peer is gone; recv should report Closed (or an Io error if the peer
    // closed before we read anything).
    let result = t.recv().await;
    assert!(result.is_err(), "expected error after peer close");
}

#[tokio::test]
async fn telemetry_with_optional_fields_round_trips_over_tcp() {
    use hivemind_protocol::{Attitude, DronePhase, GpsFixType, Position, Telemetry};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let expected_msg = LegionToOracle::Hello {
        drone_id: "drone-01".into(),
        legion_version: "0.1.0".into(),
        capabilities: vec!["spray".into()],
        in_progress_sortie: Some(InProgressSortie {
            sortie_id: "interrupted".into(),
            last_completed_step: Some(7),
        }),
    };

    let send_msg = expected_msg.clone();
    let legion = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut t: TcpTransport<LegionToOracle, OracleToLegion> = TcpTransport::new(stream);

        let env = Envelope::new("drone-01", 1, send_msg);
        t.send(&env).await.unwrap();

        // Then a telemetry frame to exercise the Telemetry path.
        let tele = Telemetry {
            ts_ms: 99,
            position: Position {
                lat: 50.0,
                lon: 14.0,
                alt_m: 5.0,
            },
            attitude: Attitude {
                roll_deg: 1.0,
                pitch_deg: 2.0,
                yaw_deg: 3.0,
            },
            battery_pct: 75.0,
            voltage: 16.2,
            paint_remaining_ml: 410.0,
            tof_distance_cm: None,
            gps_fix: GpsFixType::Fix3d,
            sortie_id: None,
            step_index: None,
            drone_phase: DronePhase::Idle,
        };
        let env2 = Envelope::new("drone-01", 100, LegionToOracle::Telemetry(tele));
        t.send(&env2).await.unwrap();
    });

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut t: TcpTransport<OracleToLegion, LegionToOracle> = TcpTransport::new(stream);

    let first = t.recv().await.unwrap();
    assert_eq!(first.msg, expected_msg);

    let second = t.recv().await.unwrap();
    assert!(matches!(second.msg, LegionToOracle::Telemetry(_)));

    legion.await.unwrap();
}
