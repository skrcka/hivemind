//! Fleet endpoints — GET /v1/fleet, GET /v1/fleet/:drone_id.

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;

use crate::error::ApiError;
use crate::store::drones::DroneRow;

use super::AppState;

#[derive(Debug, Serialize)]
pub struct FleetResponse {
    pub drones: Vec<DroneRow>,
}

pub async fn get_fleet(State(app): State<AppState>) -> Result<Json<FleetResponse>, ApiError> {
    let drones = app.store.list_drones().await?;
    Ok(Json(FleetResponse { drones }))
}

pub async fn get_drone(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DroneRow>, ApiError> {
    let drones = app.store.list_drones().await?;
    drones
        .into_iter()
        .find(|d| d.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("drone {id}")))
}
