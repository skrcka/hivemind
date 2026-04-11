//! Composed safety loop — ties the arming state machine, the watchdogs,
//! and the interlocks into one task that ticks at the gamepad cadence.
//!
//! What it does on every tick:
//!
//!   1. Reads the latest `ControlIntent` from the gamepad watch channel.
//!   2. Runs the controller-silent and link-silent watchdogs.
//!   3. Advances the arming hold timer if LB+RB is being held.
//!   4. Publishes the latest `ArmingState` to the shared watch channel.
//!   5. When the arming hold fires, sends the arm command via `send::send`.
//!
//! This task does NOT own the MAVLink send loop for `MANUAL_CONTROL` at
//! 20 Hz — that lives in a separate task spawned inside
//! `tauri_commands::connect` once the link comes up. The safety loop's job
//! is to supervise the arming state and event-triggered commands.

pub mod arming;
pub mod interlock;
pub mod watchdog;

use std::time::Duration;

use tokio::time::{self, MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::mavlink_link::send::{self as send_m, build_arm};
use crate::safety::arming::{HoldState, HoldTimer};
use crate::state::{AppState, ArmingKind, ArmingState, LinkStatus};

pub async fn run_safety_loop(state: AppState) {
    let poll_interval = Duration::from_millis(25); // 40 Hz
    let arm_hold_duration = Duration::from_secs_f32(state.config.safety.arm_hold_duration_s);
    let mut arm_timer = HoldTimer::new(arm_hold_duration);

    let mut ticker = time::interval(poll_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        let intent = *state.intent.borrow();
        let link_status = *state.link_status.borrow();

        // Mirror PX4's armed bit into our cached ArmingState on every tick.
        // This is how we reconcile if the operator armed via another GCS or
        // if legion commanded arming.
        let remote_armed = state.telemetry.borrow().heartbeat.armed;

        let current = *state.arming.borrow();
        let mut next = current;

        // Advance the arming hold timer from controller input.
        let arm_held = intent.buttons.arm_combo;
        let arm_state = arm_timer.tick(arm_held);

        match (remote_armed, arm_state, current.kind) {
            // Remote says armed → mirror it regardless of the local timer.
            (true, _, _) => {
                if current.kind != ArmingKind::Armed {
                    info!("heartbeat reports armed — mirroring");
                }
                next = ArmingState::armed();
            }
            // Hold completed → send the arm command.
            (false, HoldState::Fired, _) => {
                if link_status == LinkStatus::Connected {
                    info!("arm hold fired — sending COMPONENT_ARM_DISARM");
                    let target_sys = state.config.link.drone_system_id;
                    let target_cmp = state.config.link.target_component_id;
                    if let Some(link) = state.link.read().await.as_ref() {
                        let conn = link.conn();
                        let msg = build_arm(target_sys, target_cmp, true);
                        if let Err(e) = send_m::send(conn, msg).await {
                            warn!(error = ?e, "arm command send failed");
                        }
                    } else {
                        warn!("arm hold fired but link is not active");
                    }
                    next = ArmingState::arming(1.0);
                } else {
                    warn!(?link_status, "arm hold fired but link is not Connected");
                    next = ArmingState::disarmed();
                }
            }
            // Hold in progress → show progress.
            (false, HoldState::Holding { progress }, _) => {
                next = ArmingState::arming(progress);
            }
            // Idle → if we previously reported Arming, revert.
            (false, HoldState::Idle, ArmingKind::Arming) => {
                next = ArmingState::disarmed();
            }
            (false, HoldState::Idle, _) => {
                // Leave as is (Disarmed / Armed — but Armed with remote_armed=false
                // is stale; clear it).
                if current.kind == ArmingKind::Armed {
                    next = ArmingState::disarmed();
                }
            }
        }

        if next != current {
            debug!(?next, "arming state change");
            let _ = state.arming_tx.send(next);
        }

        // Watchdog: if gamepad and link are both silent, do nothing special
        // — the gamepad task's per-tick intent update will push neutral
        // sticks once last_event_at grows old, and the MAVLink recv loop
        // will bump link status to Stale.
    }
}
