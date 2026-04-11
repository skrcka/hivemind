//! Smoke test of the HTTP API up to plan creation + abort. Doesn't approve
//! a plan because that requires a connected legion (covered by handshake.rs).

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use oracle::api::{router, AppState};
use oracle::apply::supervisor::OperatorSignals;
use oracle::config::OracleConfig;
use oracle::domain::intent::{Face, Intent, MeshRegion, OperatorConstraints, ScanRef};
use oracle::fleet::FleetState;
use oracle::legion_link::server::{start_with_listener, ListenerConfig};
use oracle::store::Store;
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceExt;

const TEST_DRONE_ID: &str = "drone-01";

async fn build_state() -> AppState {
    let store = Store::open_memory().await.unwrap();
    store
        .upsert_drone(TEST_DRONE_ID, Some("0.1.0"), &["spray".to_string()])
        .await
        .unwrap();

    // Real link bound to a random port (we never connect to it in this test).
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
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

    AppState {
        store: Arc::new(store),
        link: Arc::new(link),
        fleet: FleetState::new(),
        config: Arc::new(OracleConfig::default()),
        operator_signals: OperatorSignals::new(),
    }
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
            id: "api-smoke-east-wall".into(),
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
async fn intent_then_plan_then_abort_round_trip() {
    let state = build_state().await;
    let app = router(state.clone());

    // 1. POST /v1/intents
    let intent = east_wall_intent();
    let intent_body = serde_json::to_string(&intent).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/intents")
                .header("content-type", "application/json")
                .body(Body::from(intent_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "POST /v1/intents");

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let intent_id = receipt["id"].as_str().unwrap().to_string();
    assert_eq!(intent_id, "api-smoke-east-wall");

    // 2. POST /v1/plans
    let create = json!({ "intent_id": intent_id });
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/plans")
                .header("content-type", "application/json")
                .body(Body::from(create.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "POST /v1/plans");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let plan_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let plan_id = plan_json["id"].as_str().unwrap().to_string();

    // 3. GET /v1/plans/:id
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/v1/plans/{plan_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "GET /v1/plans/:id");

    // 4. GET /v1/plans?status=Proposed
    let response = app
        .clone()
        .oneshot(
            Request::get("/v1/plans?status=Proposed")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "GET /v1/plans?status=…");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let listing: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        listing["plans"].as_array().unwrap().iter().any(|p| p["id"].as_str() == Some(&plan_id)),
        "newly created plan should appear in the Proposed list"
    );

    // 5. POST /v1/plans/:id/abort
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/v1/plans/{plan_id}/abort"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "POST /v1/plans/:id/abort");

    // 6. GET /v1/audit
    let response = app
        .clone()
        .oneshot(
            Request::get("/v1/audit?limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "GET /v1/audit");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let audit: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let entries = audit["entries"].as_array().unwrap();
    assert!(
        entries.iter().any(|e| e["event"].as_str() == Some("plan_proposed")),
        "audit log must contain plan_proposed"
    );
    assert!(
        entries.iter().any(|e| e["event"].as_str() == Some("plan_aborted")),
        "audit log must contain plan_aborted"
    );
}

#[tokio::test]
async fn missing_intent_returns_404() {
    let state = build_state().await;
    let app = router(state);

    let response = app
        .oneshot(
            Request::get("/v1/intents/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn replan_returns_501() {
    let state = build_state().await;
    let app = router(state);

    let response = app
        .oneshot(
            Request::post("/v1/plans/00000000-0000-0000-0000-000000000000/replan")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}
