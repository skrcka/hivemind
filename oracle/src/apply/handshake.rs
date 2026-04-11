//! The actual `StepComplete → validate → Proceed` loop. Pure async function
//! that takes a `Sortie`, a `Link`, and a gate; runs the handshake to
//! completion or returns an error.

use std::time::Duration;

use hivemind_protocol::{LegionToOracle, Sortie};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::domain::plan::PlanId;
use crate::legion_link::{AuthorityKind, CommandAuthority, Link, LinkError};
use crate::store::audit::AuditEntry;
use crate::store::sorties::SortieStatus;
use crate::store::steps::{GateDecision, StepCompletion};
use crate::store::Store;

use super::gate::{Gate, GateContext, GateEvaluator};

#[derive(Debug, Error)]
pub enum HandshakeError {
    #[error("link error: {0}")]
    Link(#[from] LinkError),
    #[error("legion event channel lagged behind ({0} messages dropped)")]
    EventLag(u64),
    #[error("legion event channel closed")]
    EventClosed,
    #[error("timeout waiting for {what}")]
    Timeout { what: &'static str },
    #[error("legion rejected sortie upload: {reason}")]
    SortieRejected { reason: String },
    #[error("legion reported sortie failed at step {step_index}: {reason}")]
    SortieFailed { step_index: u32, reason: String },
    #[error("legion reported safety event: {kind} → {action}")]
    SafetyAbort { kind: String, action: String },
    #[error("operator aborted: {reason}")]
    OperatorAborted { reason: String },
    #[error("db error: {0}")]
    Db(#[from] sqlx::Error),
}

const ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// Drive a single sortie end-to-end. Used by the Apply Supervisor for each
/// sortie in the plan.
pub async fn handshake_one_sortie(
    store: &Store,
    plan_id: PlanId,
    sortie: &Sortie,
    link: &Link,
    auth: &CommandAuthority,
    gate: &dyn GateEvaluator,
    operator_decisions: &mut Option<broadcast::Receiver<crate::apply::supervisor::OperatorEvent>>,
) -> Result<(), HandshakeError> {
    let mut events = link.subscribe();

    audit_event(
        store,
        AuditEntry {
            actor: "system:apply_supervisor".into(),
            event: "sortie_dispatch_start".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: Some(sortie.sortie_id.clone()),
            drone_id: Some(sortie.drone_id.clone()),
            payload: serde_json::json!({ "step_count": sortie.steps.len() }),
        },
    )
    .await;

    // 1. Upload.
    link.upload_sortie(&sortie.drone_id, sortie, auth).await?;
    store
        .set_sortie_status(&sortie.sortie_id, SortieStatus::Uploaded, None)
        .await?;
    wait_sortie_received(&mut events, &sortie.sortie_id).await?;
    info!(sortie_id = %sortie.sortie_id, "sortie received by legion");

    store
        .set_sortie_status(&sortie.sortie_id, SortieStatus::Executing, None)
        .await?;

    // 2. Walk the steps.
    for step in &sortie.steps {
        let ctx = GateContext {
            plan_id,
            sortie,
            step,
        };
        let decision = gate.evaluate(ctx);

        match &decision {
            Gate::AutoProceed => {
                store
                    .record_step_gated(
                        &sortie.sortie_id,
                        step.index,
                        GateDecision::AutoProceed,
                        None,
                    )
                    .await?;
                link.send_proceed(&sortie.drone_id, &sortie.sortie_id, step.index, auth)
                    .await?;
            }
            Gate::OperatorRequired { reason } => {
                store
                    .record_step_gated(
                        &sortie.sortie_id,
                        step.index,
                        GateDecision::OperatorRequired,
                        Some(reason),
                    )
                    .await?;
                link.send_hold(&sortie.drone_id, &sortie.sortie_id, reason.clone(), auth)
                    .await?;
                wait_for_operator_decision(
                    operator_decisions,
                    &sortie.sortie_id,
                    step.index,
                )
                .await?;
                link.send_proceed(&sortie.drone_id, &sortie.sortie_id, step.index, auth)
                    .await?;
            }
            Gate::FleetConflict { with } => {
                store
                    .record_step_gated(
                        &sortie.sortie_id,
                        step.index,
                        GateDecision::FleetConflict,
                        Some(with),
                    )
                    .await?;
                link.send_hold(
                    &sortie.drone_id,
                    &sortie.sortie_id,
                    format!("fleet_conflict:{with}"),
                    auth,
                )
                .await?;
                // v1 has one drone so this branch never fires; the conflict
                // resolver loop is a v2 deliverable.
                return Err(HandshakeError::OperatorAborted {
                    reason: format!("fleet conflict with {with} (v1 has no resolver)"),
                });
            }
            Gate::AbortSortie { reason } => {
                store
                    .record_step_gated(
                        &sortie.sortie_id,
                        step.index,
                        GateDecision::AbortSortie,
                        Some(reason),
                    )
                    .await?;
                link.send_abort(
                    &sortie.drone_id,
                    &sortie.sortie_id,
                    reason.clone(),
                    AuthorityKind::Plan(*auth),
                )
                .await?;
                store
                    .set_sortie_status(
                        &sortie.sortie_id,
                        SortieStatus::Aborted,
                        Some(reason),
                    )
                    .await?;
                return Err(HandshakeError::OperatorAborted {
                    reason: reason.clone(),
                });
            }
        }

        store
            .record_step_running(&sortie.sortie_id, step.index)
            .await?;

        // Wait for legion to report this step done.
        let completion =
            wait_step_complete(&mut events, &sortie.sortie_id, step.index).await?;
        store
            .record_step_complete(&sortie.sortie_id, step.index, &completion)
            .await?;
        debug!(sortie_id = %sortie.sortie_id, step_index = step.index, "step complete");
    }

    // 3. Wait for the SortieComplete frame.
    wait_sortie_complete(&mut events, &sortie.sortie_id).await?;
    store
        .set_sortie_status(&sortie.sortie_id, SortieStatus::Complete, None)
        .await?;

    audit_event(
        store,
        AuditEntry {
            actor: "system:apply_supervisor".into(),
            event: "sortie_complete".into(),
            plan_id: Some(plan_id.to_string()),
            sortie_id: Some(sortie.sortie_id.clone()),
            drone_id: Some(sortie.drone_id.clone()),
            payload: serde_json::json!({}),
        },
    )
    .await;

    Ok(())
}

async fn wait_sortie_received(
    events: &mut broadcast::Receiver<crate::legion_link::LegionEvent>,
    sortie_id: &str,
) -> Result<(), HandshakeError> {
    poll_with_timeout(events, ACK_TIMEOUT, "sortie_received", |msg| match msg {
        LegionToOracle::SortieReceived { sortie_id: sid } if sid == sortie_id => Some(Ok(())),
        LegionToOracle::Error { code, message } => Some(Err(HandshakeError::SortieRejected {
            reason: format!("{code}: {message}"),
        })),
        _ => None,
    })
    .await
}

async fn wait_step_complete(
    events: &mut broadcast::Receiver<crate::legion_link::LegionEvent>,
    sortie_id: &str,
    step_index: u32,
) -> Result<StepCompletion, HandshakeError> {
    poll_with_timeout(events, Duration::from_secs(600), "step_complete", |msg| {
        match msg {
            LegionToOracle::StepComplete {
                sortie_id: sid,
                step_index: idx,
                position,
                battery_pct,
                paint_remaining_ml,
                duration_s,
            } if sid == sortie_id && *idx == step_index => Some(Ok(StepCompletion {
                position_lat: position.lat,
                position_lon: position.lon,
                position_alt_m: position.alt_m,
                battery_pct: *battery_pct,
                paint_remaining_ml: *paint_remaining_ml,
                duration_s: *duration_s,
            })),
            LegionToOracle::SortieFailed {
                sortie_id: sid,
                step_index: idx,
                reason,
            } if sid == sortie_id => Some(Err(HandshakeError::SortieFailed {
                step_index: *idx,
                reason: reason.clone(),
            })),
            LegionToOracle::SafetyEvent { kind, action, .. } => {
                Some(Err(HandshakeError::SafetyAbort {
                    kind: format!("{kind:?}"),
                    action: action.clone(),
                }))
            }
            _ => None,
        }
    })
    .await
}

async fn wait_sortie_complete(
    events: &mut broadcast::Receiver<crate::legion_link::LegionEvent>,
    sortie_id: &str,
) -> Result<(), HandshakeError> {
    poll_with_timeout(events, ACK_TIMEOUT, "sortie_complete", |msg| match msg {
        LegionToOracle::SortieComplete { sortie_id: sid } if sid == sortie_id => Some(Ok(())),
        _ => None,
    })
    .await
}

async fn poll_with_timeout<T, F>(
    events: &mut broadcast::Receiver<crate::legion_link::LegionEvent>,
    dur: Duration,
    what: &'static str,
    mut matcher: F,
) -> Result<T, HandshakeError>
where
    F: FnMut(&LegionToOracle) -> Option<Result<T, HandshakeError>>,
{
    let outcome = timeout(dur, async {
        loop {
            match events.recv().await {
                Ok(evt) => {
                    if let Some(res) = matcher(&evt.msg) {
                        return res;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(lagged = n, "broadcast receiver lagged");
                    return Err(HandshakeError::EventLag(n));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(HandshakeError::EventClosed);
                }
            }
        }
    })
    .await;

    match outcome {
        Ok(inner) => inner,
        Err(_) => Err(HandshakeError::Timeout { what }),
    }
}

async fn wait_for_operator_decision(
    decisions: &mut Option<broadcast::Receiver<crate::apply::supervisor::OperatorEvent>>,
    sortie_id: &str,
    step_index: u32,
) -> Result<(), HandshakeError> {
    let Some(rx) = decisions else {
        return Err(HandshakeError::OperatorAborted {
            reason: "operator gate hit but no operator channel attached".into(),
        });
    };
    loop {
        match rx.recv().await {
            Ok(evt) => {
                if evt.sortie_id == sortie_id && evt.step_index == step_index {
                    return Ok(());
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(lagged = n, "operator channel lagged");
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Err(HandshakeError::OperatorAborted {
                    reason: "operator channel closed".into(),
                });
            }
        }
    }
}

async fn audit_event(store: &Store, entry: AuditEntry) {
    if let Err(e) = store.audit(entry).await {
        warn!(error = ?e, "audit insert failed");
    }
}
