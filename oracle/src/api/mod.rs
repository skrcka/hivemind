//! axum HTTP+WS API. Pantheon-facing only — the legion link is a separate
//! transport (see `src/legion_link/`).

pub mod intents;
pub mod plans;
pub mod fleet;
pub mod ws;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::apply::supervisor::OperatorSignals;
use crate::config::OracleConfig;
use crate::fleet::FleetState;
use crate::legion_link::Link;
use crate::store::Store;

/// Shared application state passed to every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub link: Arc<Link>,
    pub fleet: FleetState,
    pub config: Arc<OracleConfig>,
    pub operator_signals: OperatorSignals,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        // Intents
        .route("/v1/intents", post(intents::post_intent))
        .route("/v1/intents/{id}", get(intents::get_intent))
        // Plans
        .route("/v1/plans", post(plans::post_plan).get(plans::list_plans))
        .route("/v1/plans/{id}", get(plans::get_plan))
        .route("/v1/plans/{id}/approve", post(plans::approve_plan))
        .route("/v1/plans/{id}/abort", post(plans::abort_plan))
        .route("/v1/plans/{id}/amendments", post(plans::amendments_not_implemented))
        .route("/v1/plans/{id}/replan", post(plans::replan_not_implemented))
        // Per-step gate operations
        .route(
            "/v1/plans/{plan_id}/sorties/{sortie_id}/steps/{step_index}/proceed",
            post(plans::step_proceed),
        )
        .route(
            "/v1/plans/{plan_id}/sorties/{sortie_id}/steps/{step_index}/hold",
            post(plans::step_hold),
        )
        .route(
            "/v1/plans/{plan_id}/sorties/{sortie_id}/abort",
            post(plans::abort_sortie),
        )
        // Fleet
        .route("/v1/fleet", get(fleet::get_fleet))
        .route("/v1/fleet/{drone_id}", get(fleet::get_drone))
        // Audit
        .route("/v1/audit", get(plans::audit))
        // WebSocket telemetry stream for pantheon subscribers
        .route("/ws/telemetry", get(ws::ws_telemetry))
        // Health check
        .route("/v1/healthz", get(plans::healthz))
        // Plumbing
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
