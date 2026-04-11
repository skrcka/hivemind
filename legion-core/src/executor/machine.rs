//! The executor state machine. `run_sortie` is the entry point called
//! by the hosting binary for each uploaded sortie.
//!
//! The shape:
//!
//! 1. Walk the steps in order.
//! 2. For each step, wait for `Proceed { expected_step_index = step.index }`
//!    from oracle, up to `step.radio_loss.silent_timeout_s`. While
//!    waiting, handle `HoldStep` / `AbortSortie` / `ReturnToBase` /
//!    `CancelSortie` inline.
//! 3. On timeout, apply the per-step radio-loss policy.
//! 4. On successful Proceed, run the step handler, checkpoint, emit
//!    `StepComplete`, loop.
//! 5. After the last step, emit `SortieComplete` and mark the store.
//!
//! The executor owns no `&mut` state shared with other tasks — it walks
//! the sortie linearly and hands control back after every step through
//! the `link.recv_executor_event` call. The hosting binary runs the
//! executor future inside a `tokio::select!` with the safety watch so a
//! safety trip can cancel mid-step.

use alloc::string::{String, ToString};
use core::time::Duration;

use crate::error::{CoreError, LinkError};
use crate::executor::{radio_loss, steps};
use crate::state::LegionState;
use crate::traits::link::ExecutorEvent;
use crate::traits::store::SortieProgress;
use crate::traits::{Clock, Link, MavlinkBackend, Payload, Pump, SortieStore};
use hivemind_protocol::{DronePhase, LegionToOracle, Sortie, SortieStep};

/// Stateless executor entry point. All state is passed in per call.
pub struct Executor;

impl Executor {
    /// Run one full sortie.
    ///
    /// Returns on `SortieComplete`, an oracle-commanded abort/RTL,
    /// a radio-loss-policy-driven RTL, or an unrecoverable error. On
    /// a safety-loop preemption the caller is expected to cancel
    /// (drop) the future this returns — the executor's commands will
    /// already have been overridden by the safety loop's direct
    /// MAVLink calls.
    pub async fn run_sortie<P, M, S, C, L>(
        sortie: Sortie,
        payload: &mut P,
        mavlink: &M,
        store: &S,
        clock: &C,
        link: &mut L,
        state: &mut LegionState,
    ) -> Result<(), CoreError>
    where
        P: Payload,
        M: MavlinkBackend,
        S: SortieStore,
        C: Clock,
        L: Link,
    {
        state.current_sortie = Some(sortie.clone());
        state.current_step_index = 0;
        state.last_completed_step = None;
        state.drone_phase = DronePhase::Armed;

        let total_steps = sortie.steps.len();
        for (idx, step) in sortie.steps.iter().enumerate() {
            state.current_step_index = step.index;

            match wait_for_proceed(&sortie, step, payload, mavlink, clock, link, state).await? {
                ProceedOutcome::GoAhead => { /* fall through to step handler */ }
                ProceedOutcome::RadioLossApplied(result) => {
                    return finalize_after_policy(result, state);
                }
                ProceedOutcome::AbortedByOracle { reason } => {
                    abort_sortie(&sortie, step, payload, mavlink, link, state, &reason).await?;
                    return Err(CoreError::AbortedByOracle { reason });
                }
                ProceedOutcome::RtlByOracle { reason } => {
                    rtl_sortie(&sortie, step, payload, mavlink, link, state, &reason).await?;
                    return Err(CoreError::RtlByOracle { reason });
                }
                ProceedOutcome::Cancelled => {
                    state.current_sortie = None;
                    state.drone_phase = DronePhase::Idle;
                    return Ok(());
                }
            }

            state.drone_phase = DronePhase::ExecutingStep;

            let outcome = steps::run_step(step, payload, mavlink, clock).await?;

            if idx + 1 == total_steps {
                state.drone_phase = DronePhase::Idle;
            } else {
                state.drone_phase = DronePhase::Holding;
            }

            state.last_completed_step = Some(step.index);

            let _ = store
                .checkpoint(&SortieProgress {
                    sortie_id: sortie.sortie_id.clone(),
                    last_completed_step: Some(step.index),
                    checkpoint_ms: clock.now_ms(),
                })
                .await;

            link.send(LegionToOracle::StepComplete {
                sortie_id: sortie.sortie_id.clone(),
                step_index: step.index,
                position: mavlink.position(),
                battery_pct: mavlink.battery_pct(),
                paint_remaining_ml: state.paint_remaining_ml,
                duration_s: outcome.duration.as_secs_f32(),
            })
            .await?;
        }

        link.send(LegionToOracle::SortieComplete {
            sortie_id: sortie.sortie_id.clone(),
        })
        .await?;
        let _ = store.mark_complete(&sortie.sortie_id).await;

        state.current_sortie = None;
        state.current_step_index = 0;
        state.drone_phase = DronePhase::Idle;
        Ok(())
    }
}

/// What the proceed-wait loop produced for the current step.
enum ProceedOutcome {
    /// Oracle sent a valid `Proceed` for this step. Run the handler.
    GoAhead,
    /// Silent timeout elapsed, and the radio-loss policy ran to a
    /// terminal outcome (Rtl or HoldingAtDestination).
    RadioLossApplied(radio_loss::PolicyResult),
    /// Oracle sent an `AbortSortie` for this sortie.
    AbortedByOracle { reason: String },
    /// Oracle sent `ReturnToBase` — hard RTL regardless of step.
    RtlByOracle { reason: String },
    /// Oracle sent `CancelSortie` — drop the sortie entirely.
    Cancelled,
}

async fn wait_for_proceed<P, M, C, L>(
    sortie: &Sortie,
    step: &SortieStep,
    payload: &mut P,
    mavlink: &M,
    clock: &C,
    link: &mut L,
    state: &mut LegionState,
) -> Result<ProceedOutcome, CoreError>
where
    P: Payload,
    M: MavlinkBackend,
    C: Clock,
    L: Link,
{
    let timeout = Duration::from_secs_f32(step.radio_loss.silent_timeout_s);

    loop {
        match link.recv_executor_event(timeout).await {
            Ok(Some(ExecutorEvent::Proceed {
                sortie_id,
                expected_step_index,
            })) => {
                if sortie_id != sortie.sortie_id {
                    link.send(LegionToOracle::Error {
                        code: "wrong_sortie".to_string(),
                        message: alloc::format!(
                            "expected sortie {}, got {}",
                            sortie.sortie_id,
                            sortie_id
                        ),
                    })
                    .await?;
                    continue;
                }
                if expected_step_index != step.index {
                    link.send(LegionToOracle::Error {
                        code: "proceed_out_of_order".to_string(),
                        message: alloc::format!(
                            "expected step {}, got {}",
                            step.index,
                            expected_step_index
                        ),
                    })
                    .await?;
                    continue;
                }
                return Ok(ProceedOutcome::GoAhead);
            }

            Ok(Some(ExecutorEvent::HoldStep { sortie_id, reason })) => {
                if sortie_id != sortie.sortie_id {
                    continue;
                }
                state.drone_phase = DronePhase::Holding;
                let _ = mavlink.hold().await;
                link.send(LegionToOracle::Held {
                    sortie_id: sortie.sortie_id.clone(),
                    step_index: step.index,
                    reason,
                })
                .await?;
                continue;
            }

            Ok(Some(ExecutorEvent::AbortSortie { sortie_id, reason })) => {
                if sortie_id != sortie.sortie_id {
                    continue;
                }
                return Ok(ProceedOutcome::AbortedByOracle { reason });
            }

            Ok(Some(ExecutorEvent::ReturnToBase { reason })) => {
                return Ok(ProceedOutcome::RtlByOracle { reason });
            }

            Ok(Some(ExecutorEvent::CancelSortie { sortie_id })) => {
                if sortie_id != sortie.sortie_id {
                    continue;
                }
                return Ok(ProceedOutcome::Cancelled);
            }

            Ok(None) | Err(LinkError::NotConnected) => {
                let result = radio_loss::apply(sortie, step, payload, mavlink, clock, link).await?;
                match result {
                    radio_loss::PolicyResult::RecoveredHolding => continue,
                    _ => return Ok(ProceedOutcome::RadioLossApplied(result)),
                }
            }

            Err(e) => return Err(e.into()),
        }
    }
}

fn finalize_after_policy(
    result: radio_loss::PolicyResult,
    state: &mut LegionState,
) -> Result<(), CoreError> {
    state.current_sortie = None;
    state.drone_phase = match result {
        radio_loss::PolicyResult::HoldingAtDestination
        | radio_loss::PolicyResult::RecoveredHolding => DronePhase::Holding,
        radio_loss::PolicyResult::Rtl => DronePhase::Landing,
    };
    Ok(())
}

async fn abort_sortie<P, M, L>(
    sortie: &Sortie,
    step: &SortieStep,
    payload: &mut P,
    mavlink: &M,
    link: &mut L,
    state: &mut LegionState,
    reason: &str,
) -> Result<(), CoreError>
where
    P: Payload,
    M: MavlinkBackend,
    L: Link,
{
    let _ = payload.pump().off().await;
    let _ = mavlink.return_to_launch().await;
    link.send(LegionToOracle::SortieFailed {
        sortie_id: sortie.sortie_id.clone(),
        step_index: step.index,
        reason: reason.to_string(),
    })
    .await?;
    state.current_sortie = None;
    state.drone_phase = DronePhase::Landing;
    Ok(())
}

async fn rtl_sortie<P, M, L>(
    sortie: &Sortie,
    step: &SortieStep,
    payload: &mut P,
    mavlink: &M,
    link: &mut L,
    state: &mut LegionState,
    reason: &str,
) -> Result<(), CoreError>
where
    P: Payload,
    M: MavlinkBackend,
    L: Link,
{
    let _ = payload.pump().off().await;
    mavlink.return_to_launch().await?;
    link.send(LegionToOracle::SortieFailed {
        sortie_id: sortie.sortie_id.clone(),
        step_index: step.index,
        reason: alloc::format!("rtl by oracle: {reason}"),
    })
    .await?;
    state.current_sortie = None;
    state.drone_phase = DronePhase::Landing;
    Ok(())
}
