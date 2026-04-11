//! Per-step radio-loss policy enforcement.
//!
//! Called by the executor when `Link::recv_executor_event` times out
//! while waiting for the next `Proceed`. The policy is per-step (stamped
//! by oracle's slicer; operator can override) and tells legion what to
//! do if oracle stops answering mid-sortie:
//!
//! - `Continue`: finish the current step autonomously, then hover.
//! - `HoldThenRtl`: hold immediately; after `hold_then_rtl_after_s`
//!   without contact, RTL.
//! - `RtlImmediately`: don't wait — RTL right now.
//!
//! This is distinct from the safety loop. The safety loop is always-on
//! and drone-physical ("the ToF sees a wall"); the radio-loss policy is
//! per-step and context-aware ("the operator is offline during takeoff
//! vs during spray"). They overlap only in that both can cut the pump.

use alloc::string::ToString;
use core::time::Duration;

use crate::error::CoreError;
use crate::traits::{Clock, Link, MavlinkBackend, Payload, Pump};
use hivemind_protocol::{LegionToOracle, RadioLossBehaviour, Sortie, SortieStep};

use super::steps;

/// Apply the step's radio-loss policy. Returns the reason to put in the
/// outbound `Held` or `SortieFailed` frame the executor emits after
/// unwinding.
pub async fn apply<P, M, C, L>(
    sortie: &Sortie,
    step: &SortieStep,
    payload: &mut P,
    mavlink: &M,
    clock: &C,
    link: &mut L,
) -> Result<PolicyResult, CoreError>
where
    P: Payload,
    M: MavlinkBackend,
    C: Clock,
    L: Link,
{
    // In every case, the first thing we do is drop the pump. Paint
    // without supervision is never correct.
    let _ = payload.pump().off().await;

    match step.radio_loss.behaviour {
        RadioLossBehaviour::Continue => {
            // Finish the step autonomously. If the step handler errors
            // out, we propagate — the drone is now in an unknown state
            // and the safety loop is the last line of defence.
            let _ = steps::run_step(step, payload, mavlink, clock).await?;
            mavlink.hold().await?;
            // Try to report the held state, best effort. If the link is
            // still down, the comms task will queue it for the next
            // reconnect.
            let _ = link
                .send(LegionToOracle::Held {
                    sortie_id: sortie.sortie_id.clone(),
                    step_index: step.index,
                    reason: "radio_loss_continue".to_string(),
                })
                .await;
            Ok(PolicyResult::HoldingAtDestination)
        }
        RadioLossBehaviour::HoldThenRtl => {
            mavlink.hold().await?;
            let _ = link
                .send(LegionToOracle::Held {
                    sortie_id: sortie.sortie_id.clone(),
                    step_index: step.index,
                    reason: "radio_loss_hold".to_string(),
                })
                .await;

            let wait = step.radio_loss.hold_then_rtl_after_s.unwrap_or(30.0);
            clock.sleep(Duration::from_secs_f32(wait)).await;

            // If the link recovered in the meantime, don't RTL — the
            // executor's next `recv_executor_event` will pick up the
            // backlog and continue.
            if link.is_connected() {
                return Ok(PolicyResult::RecoveredHolding);
            }
            mavlink.return_to_launch().await?;
            Ok(PolicyResult::Rtl)
        }
        RadioLossBehaviour::RtlImmediately => {
            mavlink.return_to_launch().await?;
            Ok(PolicyResult::Rtl)
        }
    }
}

/// What the policy ended up doing. The executor maps this into its own
/// unwinding logic (report `SortieFailed` vs give up quietly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyResult {
    /// `Continue`: the step finished autonomously and the drone is
    /// holding at its destination, waiting for oracle.
    HoldingAtDestination,
    /// `HoldThenRtl` with a link recovery during the hold window.
    /// Executor may retry the `recv_executor_event` loop.
    RecoveredHolding,
    /// The drone was commanded to RTL (either `HoldThenRtl` timed out
    /// or `RtlImmediately` fired). Sortie is effectively terminated.
    Rtl,
}
