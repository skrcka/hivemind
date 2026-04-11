//! 10 Hz safety wrapper. Ticks `tokio::time::interval`, calls the
//! single-tick `legion_core::safety::safety_check` function against
//! the shared state, and publishes the resulting `SafetyState` on a
//! `tokio::sync::watch` channel so the executor can `tokio::select!`
//! on it for mid-step preemption.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use legion_core::safety::{check::safety_check, SafetyConfig, SafetyOutcome, SafetyState};
use legion_core::{LegionToOracle, MavlinkBackend, Payload};
use tokio::sync::{watch, Mutex};

use crate::comms::CommsCommand;
use crate::shared_state::SharedState;

/// Shared handles the safety loop needs.
pub struct SafetyLoopHandles<P, M>
where
    P: Payload + Send + 'static,
    M: MavlinkBackend + 'static,
{
    pub state: SharedState,
    pub payload: Arc<Mutex<P>>,
    pub mavlink: Arc<M>,
    pub clock: Arc<crate::TokioClock>,
    pub cfg: SafetyConfig,
    pub safety_tx: watch::Sender<SafetyState>,
    pub command_tx: tokio::sync::mpsc::UnboundedSender<CommsCommand>,
    pub last_oracle_contact_ms: Arc<AtomicU64>,
}

/// Run the safety loop forever. Intended to be spawned into its own
/// tokio task.
pub async fn run<P, M>(handles: SafetyLoopHandles<P, M>)
where
    P: Payload + Send + 'static,
    M: MavlinkBackend + 'static,
{
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;

        // Mirror the atomic into the state snapshot the check reads.
        {
            let mut st = handles.state.write().await;
            st.last_oracle_contact_ms = handles.last_oracle_contact_ms.load(Ordering::Acquire);
        }

        let outcome = {
            let mut payload = handles.payload.lock().await;
            let mut state = handles.state.write().await;
            safety_check(
                &mut *payload,
                handles.mavlink.as_ref(),
                handles.clock.as_ref(),
                &mut state,
                &handles.cfg,
            )
            .await
        };

        match outcome {
            SafetyOutcome::Ok => {
                if *handles.safety_tx.borrow() != SafetyState::Ok {
                    let _ = handles.safety_tx.send(SafetyState::Ok);
                }
            }
            SafetyOutcome::Tripped { state, action } => {
                tracing::warn!(?state, action, "safety: tripped");
                if let Some((kind, detail)) =
                    legion_core::safety::check::outbound_event_fields(&state)
                {
                    let _ = handles.command_tx.send(CommsCommand::Send(
                        LegionToOracle::SafetyEvent {
                            kind,
                            action: action.to_string(),
                            detail,
                        },
                    ));
                }
                let _ = handles.safety_tx.send(state);
            }
        }
    }
}

/// Build a fresh `watch` channel for the safety state, defaulting to
/// `Ok`.
pub fn new_watch() -> (watch::Sender<SafetyState>, watch::Receiver<SafetyState>) {
    watch::channel(SafetyState::Ok)
}
