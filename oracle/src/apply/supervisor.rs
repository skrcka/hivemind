//! Apply Supervisor — one tokio task per executing plan. Drives every
//! sortie in the plan through the step-confirmation handshake.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::domain::plan::{HivemindPlan, PlanId, PlanStatus};
use crate::legion_link::Link;
use crate::store::audit::AuditEntry;
use crate::store::sorties::SortieStatus;
use crate::store::Store;

use super::gate::{AutoProceedEvaluator, GateEvaluator};
use super::handshake::{handshake_one_sortie, HandshakeError};

/// One operator action on a gated step. Sent on the supervisor's
/// `OperatorSignals` broadcast.
#[derive(Debug, Clone)]
pub struct OperatorEvent {
    pub sortie_id: String,
    pub step_index: u32,
    pub action: OperatorAction,
}

#[derive(Debug, Clone)]
pub enum OperatorAction {
    Proceed,
    Hold,
    Abort { reason: String },
}

/// Multiplexer the API uses to fan operator clicks into the supervisor task.
/// Cloneable; one is held by the AppState, one is given to each running
/// supervisor.
#[derive(Debug, Clone)]
pub struct OperatorSignals {
    tx: broadcast::Sender<OperatorEvent>,
}

impl Default for OperatorSignals {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorSignals {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx }
    }

    pub fn send(&self, evt: OperatorEvent) {
        let _ = self.tx.send(evt);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OperatorEvent> {
        self.tx.subscribe()
    }
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("plan not found: {0}")]
    PlanNotFound(PlanId),
    #[error("plan {plan_id} not in Approved status (saw {actual:?})")]
    NotApproved {
        plan_id: PlanId,
        actual: PlanStatus,
    },
    #[error("plan has no sorties")]
    EmptyPlan,
    #[error("handshake failed: {0}")]
    Handshake(#[from] HandshakeError),
    #[error("db error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Spawn a fresh Apply Supervisor task for an Approved plan. Returns the
/// JoinHandle so the caller can await completion if it wants to.
pub fn spawn_apply(
    store: Arc<Store>,
    link: Arc<Link>,
    plan: HivemindPlan,
    operator_signals: OperatorSignals,
) -> JoinHandle<Result<(), ApplyError>> {
    let evaluator: Arc<dyn GateEvaluator> = Arc::new(AutoProceedEvaluator);
    tokio::spawn(async move {
        run_supervisor(store, link, plan, operator_signals, evaluator).await
    })
}

async fn run_supervisor(
    store: Arc<Store>,
    link: Arc<Link>,
    plan: HivemindPlan,
    operator_signals: OperatorSignals,
    gate: Arc<dyn GateEvaluator>,
) -> Result<(), ApplyError> {
    let plan_id = plan.id;
    info!(plan_id = %plan_id, sortie_count = plan.sorties.len(), "apply supervisor starting");

    if plan.sorties.is_empty() {
        return Err(ApplyError::EmptyPlan);
    }

    store
        .set_plan_status(plan_id, PlanStatus::Executing, None)
        .await?;
    store
        .audit(AuditEntry {
            actor: "system:apply_supervisor".into(),
            event: "plan_executing".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: None,
            drone_id: None,
            payload: serde_json::json!({
                "sortie_count": plan.sorties.len(),
            }),
        })
        .await?;

    // Mint an authority token for this plan. Only this task can use it.
    let auth = link.authority_for_plan(plan_id);

    let mut operator_decisions: Option<broadcast::Receiver<OperatorEvent>> =
        Some(operator_signals.subscribe());

    for sortie in &plan.sorties {
        let result = handshake_one_sortie(
            &store,
            plan_id,
            sortie,
            &link,
            &auth,
            gate.as_ref(),
            &mut operator_decisions,
        )
        .await;

        if let Err(err) = result {
            error!(plan_id = %plan_id, sortie_id = %sortie.sortie_id, error = ?err, "sortie handshake failed");
            store
                .set_sortie_status(
                    &sortie.sortie_id,
                    SortieStatus::Failed,
                    Some(&err.to_string()),
                )
                .await
                .ok();
            store
                .set_plan_status(plan_id, PlanStatus::Failed, None)
                .await
                .ok();
            store
                .audit(AuditEntry {
                    actor: "system:apply_supervisor".into(),
                    event: "plan_failed".into(),
                    plan_id: Some(plan_id.to_string()),
                    sortie_id: Some(sortie.sortie_id.clone()),
                    drone_id: Some(sortie.drone_id.clone()),
                    payload: serde_json::json!({ "error": err.to_string() }),
                })
                .await
                .ok();
            return Err(err.into());
        }
    }

    store
        .set_plan_status(plan_id, PlanStatus::Complete, None)
        .await?;
    store
        .audit(AuditEntry {
            actor: "system:apply_supervisor".into(),
            event: "plan_complete".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: None,
            drone_id: None,
            payload: serde_json::json!({}),
        })
        .await?;

    info!(plan_id = %plan_id, "apply supervisor finished");
    Ok(())
}
