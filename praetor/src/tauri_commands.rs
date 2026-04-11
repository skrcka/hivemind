//! `#[tauri::command]` handlers and the telemetry event emitter.
//!
//! Three things live here:
//!
//!   1. Command handlers that the React frontend calls via `invoke()`.
//!   2. A long-running task that watches the various `watch` channels in
//!      `AppState` and emits Tauri events (`telemetry_update`,
//!      `link_status`, `arming_state`, `controller_status`) whenever the
//!      values change. The frontend subscribes with `listen()`.
//!   3. A 20 Hz MANUAL_CONTROL streaming task that is spawned whenever the
//!      link is connected and dropped when it's not.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tokio::time::{self, MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::error::{PraetorError, Result};
use crate::gamepad::intent::{ControlIntent, ManualControlFrame};
use crate::mavlink_link::send::{
    build_emergency_disarm, build_land, build_manual_control, build_rtl, build_set_servo,
    build_takeoff, send,
};
use crate::mavlink_link::MavlinkLink;
use crate::safety::interlock;
use crate::state::{AppState, ArmingState, ControllerStatus, LinkStatus};

// ─── Command handlers ──────────────────────────────────────────────

#[tauri::command]
pub async fn connect(state: State<'_, AppState>, address: Option<String>) -> Result<()> {
    let address = address.unwrap_or_else(|| state.config.link.address.clone());
    info!(%address, "operator requested connect");

    let link = MavlinkLink::connect(state.inner().clone(), &address).await?;
    {
        let mut guard = state.link.write().await;
        *guard = Some(link);
    }

    // Spawn the 20 Hz MANUAL_CONTROL streamer tied to this connection.
    tokio::spawn(run_manual_control_streamer(state.inner().clone()));

    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<()> {
    info!("operator requested disconnect");
    let mut guard = state.link.write().await;
    *guard = None; // Drop impl signals the recv thread to exit.
    let _ = state.link_status_tx.send(LinkStatus::Disconnected);
    Ok(())
}

#[tauri::command]
pub async fn begin_arming(state: State<'_, AppState>) -> Result<()> {
    // UI trigger for operators who want the arming hold to start from a
    // button click rather than the controller combo. The actual state
    // transitions live in `safety::run_safety_loop` which watches
    // `intent.buttons.arm_combo`; setting it from a Tauri command is a
    // small bridge that lets the button behave identically to LB+RB.
    let mut intent = *state.intent.borrow();
    intent.buttons.arm_combo = true;
    state.intent_tx.send_replace(intent);
    Ok(())
}

#[tauri::command]
pub async fn cancel_arming(state: State<'_, AppState>) -> Result<()> {
    let mut intent = *state.intent.borrow();
    intent.buttons.arm_combo = false;
    state.intent_tx.send_replace(intent);
    Ok(())
}

#[tauri::command]
pub async fn emergency_stop(state: State<'_, AppState>) -> Result<()> {
    warn!("EMERGENCY STOP fired");
    let link_guard = state.link.read().await;
    let Some(link) = link_guard.as_ref() else {
        return Err(PraetorError::NotConnected);
    };
    let msg = build_emergency_disarm(link.target_system, link.target_component);
    send(link.conn(), msg).await?;
    let _ = state.arming_tx.send(ArmingState::disarmed());
    Ok(())
}

#[tauri::command]
pub async fn takeoff(state: State<'_, AppState>) -> Result<()> {
    let link_guard = state.link.read().await;
    let Some(link) = link_guard.as_ref() else {
        return Err(PraetorError::NotConnected);
    };
    let msg = build_takeoff(
        link.target_system,
        link.target_component,
        state.config.takeoff.default_altitude_m,
    );
    send(link.conn(), msg).await
}

#[tauri::command]
pub async fn land(state: State<'_, AppState>) -> Result<()> {
    let link_guard = state.link.read().await;
    let Some(link) = link_guard.as_ref() else {
        return Err(PraetorError::NotConnected);
    };
    let msg = build_land(link.target_system, link.target_component);
    send(link.conn(), msg).await
}

#[tauri::command]
pub async fn return_to_launch(state: State<'_, AppState>) -> Result<()> {
    let link_guard = state.link.read().await;
    let Some(link) = link_guard.as_ref() else {
        return Err(PraetorError::NotConnected);
    };
    let msg = build_rtl(link.target_system, link.target_component);
    send(link.conn(), msg).await
}

#[tauri::command]
pub async fn cycle_mode(_state: State<'_, AppState>) -> Result<()> {
    // Phase 2+ will implement a proper mode-cycle list. For v1 this is a
    // stub so the frontend can wire the button up now.
    warn!("cycle_mode not yet implemented");
    Ok(())
}

#[tauri::command]
pub async fn list_serial_ports() -> Result<Vec<String>> {
    tokio::task::spawn_blocking(|| {
        tokio_serial::available_ports()
            .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
            .map_err(PraetorError::from)
    })
    .await?
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<crate::config::Config> {
    Ok((*state.config).clone())
}

// ─── MANUAL_CONTROL streamer ───────────────────────────────────────

/// Streams `MANUAL_CONTROL` frames at 20 Hz whenever the link is up and
/// the drone is armed. Also handles the edge-triggered pump-on/off
/// commands (hold-to-spray semantics).
async fn run_manual_control_streamer(state: AppState) {
    let mut ticker = time::interval(Duration::from_millis(50)); // 20 Hz
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut prev_pump = false;

    loop {
        ticker.tick().await;

        // Stop when the link is gone.
        let conn_target = {
            let guard = state.link.read().await;
            guard
                .as_ref()
                .map(|link| (link.conn(), link.target_system, link.target_component))
        };
        let Some((conn, target_sys, target_cmp)) = conn_target else {
            debug!("manual_control streamer: link is gone — exiting");
            return;
        };

        let link_status = *state.link_status.borrow();
        if link_status != LinkStatus::Connected {
            continue;
        }

        let arming = *state.arming.borrow();
        let intent = *state.intent.borrow();

        // --- Pump edge detection (hold-to-spray) ---
        // We send pump-on on the rising edge and pump-off on the falling
        // edge. The 20 Hz loop doesn't need to spam DO_SET_SERVO frames —
        // one per transition is plenty.
        let pump_wanted = intent.buttons.pump;
        if pump_wanted != prev_pump {
            let snap = state.telemetry.borrow().clone();
            if pump_wanted {
                let guard_result = interlock::guard_pump_on(
                    link_status,
                    arming,
                    &snap,
                    state.config.safety.pump_minimum_altitude_m,
                );
                match guard_result {
                    Ok(()) => {
                        let msg = build_set_servo(
                            target_sys,
                            target_cmp,
                            state.config.pump.servo_index,
                            state.config.pump.pwm_on_us,
                        );
                        if let Err(e) = send(Arc::clone(&conn), msg).await {
                            warn!(error = ?e, "pump-on send failed");
                        } else {
                            info!("pump ON");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "pump-on interlock refused");
                    }
                }
            } else {
                // Pump-off always goes through, regardless of interlock.
                let msg = build_set_servo(
                    target_sys,
                    target_cmp,
                    state.config.pump.servo_index,
                    state.config.pump.pwm_off_us,
                );
                if let Err(e) = send(Arc::clone(&conn), msg).await {
                    warn!(error = ?e, "pump-off send failed");
                } else {
                    info!("pump OFF");
                }
            }
            prev_pump = pump_wanted;
        }

        // --- MANUAL_CONTROL stick stream ---
        if arming.kind != crate::state::ArmingKind::Armed {
            continue;
        }

        let frame = select_manual_frame(&state, &intent);
        if let Err(e) = interlock::guard_manual_control(link_status) {
            warn!(error = %e, "manual_control refused");
            continue;
        }

        let msg = build_manual_control(target_sys, frame);
        if let Err(e) = send(Arc::clone(&conn), msg).await {
            warn!(error = ?e, "manual_control send failed");
        }
    }
}

/// If the controller has been silent for longer than the soft threshold,
/// send neutral sticks instead of whatever stale intent is cached. After
/// the hard threshold the safety loop will also send a LOITER_UNLIM.
fn select_manual_frame(state: &AppState, intent: &ControlIntent) -> ManualControlFrame {
    let soft = Duration::from_secs_f32(state.config.gamepad.silent_threshold_s);
    let silent = matches!(intent.last_event_at, Some(t) if t.elapsed() > soft)
        || intent.last_event_at.is_none();

    if silent {
        ManualControlFrame::neutral()
    } else {
        ManualControlFrame::from_intent(intent)
    }
}

// ─── Event emitter ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct TelemetryEvent {
    #[serde(flatten)]
    snapshot: crate::mavlink_link::snapshot::TelemetrySnapshot,
}

/// Long-running task that watches the shared state and pushes events to
/// the frontend on every change. Runs at 20 Hz for telemetry (matching
/// MAVLink's natural cadence) and pushes all other status channels
/// change-triggered via `watch::Receiver::changed()`.
pub async fn run_event_emitter(handle: Arc<AppHandle>, state: AppState) {
    let handle_for_telemetry = Arc::clone(&handle);
    let state_for_telemetry = state.clone();
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_millis(50));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let snap = state_for_telemetry.telemetry.borrow().clone();
            let _ =
                handle_for_telemetry.emit("telemetry_update", TelemetryEvent { snapshot: snap });
        }
    });

    spawn_change_emitter(
        Arc::clone(&handle),
        state.link_status.clone(),
        "link_status",
    );
    spawn_change_emitter(Arc::clone(&handle), state.arming.clone(), "arming_state");
    spawn_change_emitter(
        Arc::clone(&handle),
        state.controller.clone(),
        "controller_status",
    );

    std::future::pending::<()>().await;
}

/// Helper: whenever `rx` sees a change, emit `event_name` with the new value.
fn spawn_change_emitter<T>(
    handle: Arc<AppHandle>,
    mut rx: tokio::sync::watch::Receiver<T>,
    event_name: &'static str,
) where
    T: Serialize + Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        // Emit the current value once at startup so new frontends get state
        // without waiting for the next change.
        {
            let initial = rx.borrow().clone();
            let _ = handle.emit(event_name, initial);
        }
        while rx.changed().await.is_ok() {
            let val = rx.borrow().clone();
            let _ = handle.emit(event_name, val);
        }
    });
}

/// Borrow + clone in one step for watch receivers whose `T` doesn't impl
/// `Copy`. Used by the intent stream above since `ControlIntent` has an
/// `Instant` inside and is `!Copy`.
#[allow(dead_code)]
fn clone_from_watch<T: Clone>(rx: &tokio::sync::watch::Receiver<T>) -> T {
    rx.borrow().clone()
}

// Compile-time proof that the RwLock-wrapped link is `Send + Sync` so the
// tokio tasks we spawn can hold an AppState clone.
fn _assert_send_sync() {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<AppState>();
    is_send_sync::<Arc<RwLock<Option<MavlinkLink>>>>();
    is_send_sync::<ControllerStatus>();
}
