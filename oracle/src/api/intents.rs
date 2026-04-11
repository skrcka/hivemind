//! Intent endpoints — POST /v1/intents, GET /v1/intents/:id.

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;

use crate::domain::intent::Intent;
use crate::error::ApiError;

use super::AppState;

#[derive(Debug, Serialize)]
pub struct IntentReceipt {
    pub id: String,
    pub region_count: usize,
    pub total_area_m2: f64,
}

pub async fn post_intent(
    State(app): State<AppState>,
    Json(intent): Json<Intent>,
) -> Result<Json<IntentReceipt>, ApiError> {
    if intent.regions.is_empty() {
        return Err(ApiError::BadRequest("intent has no regions".into()));
    }
    let id = app.store.insert_intent(&intent).await?;
    let total_area_m2 = intent.regions.iter().map(|r| r.area_m2).sum();
    Ok(Json(IntentReceipt {
        id,
        region_count: intent.regions.len(),
        total_area_m2,
    }))
}

pub async fn get_intent(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Intent>, ApiError> {
    let Some(intent) = app.store.get_intent(&id).await? else {
        return Err(ApiError::NotFound(format!("intent {id}")));
    };
    Ok(Json(intent))
}
