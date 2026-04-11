//! Plan endpoints — create / list / get / approve / abort / per-step gates / audit.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::apply::supervisor::{spawn_apply, OperatorAction, OperatorEvent};
use crate::domain::plan::{HivemindPlan, PlanId, PlanStatus};
use crate::error::ApiError;
use crate::slicer;
use crate::store::audit::{AuditEntry, AuditRow};
use crate::store::plans::PlanListEntry;

use super::AppState;

// ─── POST /v1/plans ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreatePlanRequest {
    pub intent_id: String,
}

pub async fn post_plan(
    State(app): State<AppState>,
    Json(req): Json<CreatePlanRequest>,
) -> Result<Json<HivemindPlan>, ApiError> {
    let Some(intent) = app.store.get_intent(&req.intent_id).await? else {
        return Err(ApiError::NotFound(format!("intent {}", req.intent_id)));
    };

    let snapshot = app.store.fleet_snapshot().await?;

    let cfg = app.config.slicer.clone();
    let plan = tokio::task::spawn_blocking(move || slicer::plan(intent, snapshot, &cfg))
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("slicer task panicked: {e}")))??;

    app.store.insert_plan(&plan).await?;

    let _ = app
        .store
        .audit(AuditEntry {
            actor: "operator:local".into(),
            event: "plan_proposed".into(),
            plan_id: Some(plan.id.to_string()),
            sortie_id: None,
            drone_id: None,
            payload: serde_json::json!({
                "sortie_count": plan.sorties.len(),
                "warning_count": plan.warnings.len(),
                "error_count": plan.errors.len(),
            }),
        })
        .await;

    Ok(Json(plan))
}

// ─── GET /v1/plans ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListPlansQuery {
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct ListPlansResponse {
    pub plans: Vec<PlanListEntry>,
}

pub async fn list_plans(
    State(app): State<AppState>,
    Query(q): Query<ListPlansQuery>,
) -> Result<Json<ListPlansResponse>, ApiError> {
    let status = q
        .status
        .as_deref()
        .map(str::parse::<PlanStatus>)
        .transpose()
        .map_err(ApiError::BadRequest)?;
    let plans = app.store.list_plans(status, q.limit).await?;
    Ok(Json(ListPlansResponse { plans }))
}

// ─── GET /v1/plans/:id ───────────────────────────────────────

pub async fn get_plan(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<HivemindPlan>, ApiError> {
    let plan_id: PlanId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid plan id: {id}")))?;
    let Some(plan) = app.store.get_plan(plan_id).await? else {
        return Err(ApiError::NotFound(format!("plan {id}")));
    };
    Ok(Json(plan))
}

// ─── POST /v1/plans/:id/approve ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApproveResponse {
    pub plan_id: String,
    pub status: PlanStatus,
}

pub async fn approve_plan(
    State(app): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApproveResponse>, ApiError> {
    let plan_id: PlanId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid plan id: {id}")))?;

    // Validate If-Match header.
    let summary = app
        .store
        .plan_summary(plan_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("plan {id}")))?;
    let (current_status, body_hash) = summary;

    if let Some(if_match) = headers.get("If-Match").and_then(|v| v.to_str().ok()) {
        let trimmed = if_match.trim_matches('"');
        if trimmed != body_hash {
            return Err(ApiError::PreconditionFailed(format!(
                "If-Match mismatch: header={trimmed}, current={body_hash}"
            )));
        }
    }

    if current_status != PlanStatus::Proposed {
        return Err(ApiError::Conflict(format!(
            "plan {id} is in {current_status:?}, can only approve from Proposed"
        )));
    }

    let plan = app
        .store
        .get_plan(plan_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("plan {id}")))?;

    if !plan.is_approvable() {
        return Err(ApiError::BadRequest(format!(
            "plan {id} has {} blocking errors",
            plan.errors.len()
        )));
    }

    // Persist sorties + steps so the supervisor can record progress.
    app.store.insert_plan_sorties(plan_id, &plan.sorties).await?;

    // Make sure every drone in the plan has a roster row (the FK requires it).
    for sortie in &plan.sorties {
        app.store
            .upsert_drone(&sortie.drone_id, None, &[])
            .await?;
    }

    app.store
        .set_plan_status(plan_id, PlanStatus::Approved, Some("operator:local"))
        .await?;

    let _ = app
        .store
        .audit(AuditEntry {
            actor: "operator:local".into(),
            event: "plan_approved".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: None,
            drone_id: None,
            payload: serde_json::json!({ "body_hash": body_hash }),
        })
        .await;

    let store = app.store.clone();
    let link = app.link.clone();
    let signals = app.operator_signals.clone();
    let _join = spawn_apply(store, link, plan, signals);

    Ok(Json(ApproveResponse {
        plan_id: plan_id.to_string(),
        status: PlanStatus::Approved,
    }))
}

// ─── POST /v1/plans/:id/abort ────────────────────────────────

pub async fn abort_plan(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApproveResponse>, ApiError> {
    let plan_id: PlanId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid plan id: {id}")))?;

    app.store
        .set_plan_status(plan_id, PlanStatus::Aborted, None)
        .await?;
    let _ = app
        .store
        .audit(AuditEntry {
            actor: "operator:local".into(),
            event: "plan_aborted".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: None,
            drone_id: None,
            payload: serde_json::json!({}),
        })
        .await;
    Ok(Json(ApproveResponse {
        plan_id: plan_id.to_string(),
        status: PlanStatus::Aborted,
    }))
}

// ─── Per-step gate operations ─────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct StepGateBody {
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn step_proceed(
    State(app): State<AppState>,
    Path((_plan_id, sortie_id, step_index)): Path<(String, String, u32)>,
    Json(_body): Json<StepGateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    app.operator_signals.send(OperatorEvent {
        sortie_id,
        step_index,
        action: OperatorAction::Proceed,
    });
    Ok(Json(serde_json::json!({ "status": "proceed_signalled" })))
}

pub async fn step_hold(
    State(app): State<AppState>,
    Path((_plan_id, sortie_id, step_index)): Path<(String, String, u32)>,
    Json(_body): Json<StepGateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    app.operator_signals.send(OperatorEvent {
        sortie_id,
        step_index,
        action: OperatorAction::Hold,
    });
    Ok(Json(serde_json::json!({ "status": "hold_signalled" })))
}

pub async fn abort_sortie(
    State(app): State<AppState>,
    Path((_plan_id, sortie_id)): Path<(String, String)>,
    Json(body): Json<StepGateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let reason = body.reason.unwrap_or_else(|| "operator abort".into());
    app.operator_signals.send(OperatorEvent {
        sortie_id,
        step_index: u32::MAX,
        action: OperatorAction::Abort { reason },
    });
    Ok(Json(serde_json::json!({ "status": "abort_signalled" })))
}

// ─── 501 stubs (v1 doesn't ship these) ───────────────────────

pub async fn amendments_not_implemented(
    State(_app): State<AppState>,
    Path(_id): Path<String>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::NotImplemented(
        "plan amendments — see oracle/README.md → v1 explicitly does not",
    ))
}

pub async fn replan_not_implemented(
    State(_app): State<AppState>,
    Path(_id): Path<String>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::NotImplemented(
        "plan replan — see oracle/README.md → v1 explicitly does not",
    ))
}

// ─── Audit + healthz ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub since: Option<String>,
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
}

fn default_audit_limit() -> i64 {
    200
}

#[derive(Debug, Serialize)]
pub struct AuditResponse {
    pub entries: Vec<AuditRow>,
}

pub async fn audit(
    State(app): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, ApiError> {
    let entries = app.store.list_audit(q.since.as_deref(), q.limit).await?;
    Ok(Json(AuditResponse { entries }))
}

pub async fn healthz(State(_app): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
