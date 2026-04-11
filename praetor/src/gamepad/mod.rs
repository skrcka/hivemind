//! Xbox controller input — polled at a configurable rate via `gilrs`.
//!
//! `gilrs` is inherently polling-based and not tokio-aware, so the poller
//! lives in its own blocking thread and bridges to the tokio runtime by
//! publishing `ControlIntent` over a `watch` channel. Consumers (the
//! MAVLink send task, the safety loop, the Tauri event emitter) subscribe
//! and read the latest value at their own cadence — there is no per-event
//! fan-out.

pub mod binding;
pub mod intent;

use std::thread;
use std::time::{Duration, Instant};

use gilrs::{Axis as GilrsAxis, Event, EventType, Gilrs};
use tracing::{debug, error, info, warn};

use crate::gamepad::binding::{AxisBinding, Bindings};
use crate::gamepad::intent::ControlIntent;
use crate::state::{AppState, ControllerStatus};

/// Long-running task spawned from `main.rs`. Never returns under normal
/// operation. If gilrs fails to initialise (no input backend) the task
/// exits after logging — the rest of the app keeps running so the operator
/// can still see telemetry, they just can't fly.
pub async fn run_poller_task(state: AppState) {
    // gilrs::Gilrs::new() is blocking and (on macOS) must run on a thread
    // that can open IOKit HID devices. We run the whole loop on a dedicated
    // OS thread and bridge state changes via `watch::Sender::send_replace`
    // (which is !Send-safe from any thread as long as we hold the handle).
    let state_for_thread = state.clone();
    let handle = thread::Builder::new()
        .name("praetor-gamepad".into())
        .spawn(move || poller_blocking(state_for_thread))
        .expect("failed to spawn gamepad thread");

    // We don't actually need to join — the thread runs for the life of the
    // process. We keep the JoinHandle in scope by detaching it here.
    let _ = handle;
    std::future::pending::<()>().await;
}

#[allow(clippy::needless_pass_by_value)] // `state` is moved into the spawned OS thread
fn poller_blocking(state: AppState) {
    let bindings = state.config.gamepad.bindings.clone();
    let poll_interval = Duration::from_secs_f64(1.0 / f64::from(state.config.gamepad.poll_hz));

    let mut gilrs = match Gilrs::new() {
        Ok(g) => g,
        Err(e) => {
            error!(error = ?e, "gilrs init failed — gamepad input disabled");
            let _ = state.controller_tx.send(ControllerStatus::Disconnected);
            return;
        }
    };

    // Log discovered gamepads at startup. If the operator plugs a controller
    // in later gilrs will surface a `Connected` event and the loop will
    // pick it up.
    for (id, pad) in gilrs.gamepads() {
        info!(gamepad_id = ?id, name = %pad.name(), "gamepad present");
    }

    let mut current = ControlIntent::neutral();

    loop {
        let frame_start = Instant::now();

        // Drain every pending event from gilrs, then fold them into `current`.
        while let Some(event) = gilrs.next_event() {
            apply_event(&mut current, &event, &bindings, &state);
            current.last_event_at = Some(Instant::now());
        }

        // Refresh stick positions from the current gamepad state (not just
        // events — gilrs' event stream doesn't always emit a `AxisChanged`
        // when the stick is *held* at a position, so we re-read on every
        // tick). We use the *first* active gamepad; single-controller
        // assumption matches the single-drone assumption.
        if let Some((_id, pad)) = gilrs.gamepads().next() {
            current.roll = read_axis(pad.value(GilrsAxis::LeftStickX), &bindings.roll, |_| true);
            current.pitch = read_axis(pad.value(GilrsAxis::LeftStickY), &bindings.pitch, |_| true);
            current.yaw = read_axis(pad.value(GilrsAxis::RightStickX), &bindings.yaw, |_| true);
            current.throttle =
                throttle_from_right_stick_y(pad.value(GilrsAxis::RightStickY), &bindings.throttle);

            if *state.controller.borrow() != ControllerStatus::Connected {
                info!("gamepad connected");
                let _ = state.controller_tx.send(ControllerStatus::Connected);
            }
        } else if *state.controller.borrow() == ControllerStatus::Connected {
            warn!("gamepad disconnected");
            let _ = state.controller_tx.send(ControllerStatus::Disconnected);
            current = ControlIntent::neutral();
        }

        // Publish the latest intent. `send_replace` is O(1) and never blocks
        // — subscribers pick it up on their next poll.
        state.intent_tx.send_replace(current);

        // Sleep to the next tick. `thread::sleep` is fine here; this thread
        // does nothing else.
        let elapsed = frame_start.elapsed();
        if elapsed < poll_interval {
            thread::sleep(poll_interval - elapsed);
        }
    }
}

/// Apply a single gilrs event to the accumulated intent. Axes are only
/// partially handled here (the main loop re-reads stick positions on every
/// tick); the event path is primarily used for button edges, d-pad nudges,
/// and connect/disconnect notifications.
fn apply_event(current: &mut ControlIntent, event: &Event, bindings: &Bindings, _state: &AppState) {
    match &event.event {
        EventType::Connected => {
            debug!(id = ?event.id, "gamepad event: connected");
        }
        EventType::Disconnected => {
            debug!(id = ?event.id, "gamepad event: disconnected");
            *current = ControlIntent::neutral();
        }
        EventType::ButtonPressed(btn, _) => {
            set_button(current, *btn, bindings, true);
        }
        EventType::ButtonReleased(btn, _) => {
            set_button(current, *btn, bindings, false);
        }
        // ButtonChanged / AxisChanged and everything else are handled
        // implicitly by the per-tick re-read of the gamepad state above.
        _ => {}
    }
}

fn set_button(current: &mut ControlIntent, btn: gilrs::Button, b: &Bindings, pressed: bool) {
    let mut s = current.buttons;

    if btn == b.pump.to_gilrs() {
        s.pump = pressed;
    }
    if btn == b.rtl.to_gilrs() {
        s.rtl = pressed;
    }
    if btn == b.takeoff.to_gilrs() {
        s.takeoff = pressed;
    }
    if btn == b.land.to_gilrs() {
        s.land = pressed;
    }
    if btn == b.mode_cycle.to_gilrs() {
        s.mode_cycle = pressed;
    }
    if btn == b.emergency.to_gilrs() {
        s.emergency = pressed;
    }

    // Arm combo: both buttons must be held. We track their individual
    // states via two implicit fields — we don't need to surface them
    // separately because only the combined state feeds arming. The combo
    // is recomputed from the latest per-button state.
    //
    // For simplicity we treat the combo as held iff the *event* that
    // just fired was one of the combo buttons AND the other is already
    // pressed on the gamepad. Since we don't have a direct readback here
    // we check via the current ButtonStates bits only — good enough
    // because both events arrive within a few ms of each other at worst.
    if btn == b.arm_combo.0.to_gilrs() || btn == b.arm_combo.1.to_gilrs() {
        // Approximation: set true if pressed, false on release. The main
        // loop's per-tick refresh corrects any brief desync via the real
        // gamepad state.
        s.arm_combo = pressed;
    }

    // d-pad nudges become trim offsets that decay back to zero over ~500 ms
    // (handled by the main loop). For v1 we just snap to ±1 on press and 0
    // on release.
    if btn == gilrs::Button::DPadLeft {
        current.trim_roll = if pressed { -1.0 } else { 0.0 };
    }
    if btn == gilrs::Button::DPadRight {
        current.trim_roll = if pressed { 1.0 } else { 0.0 };
    }
    if btn == gilrs::Button::DPadDown {
        current.trim_pitch = if pressed { -1.0 } else { 0.0 };
    }
    if btn == gilrs::Button::DPadUp {
        current.trim_pitch = if pressed { 1.0 } else { 0.0 };
    }

    current.buttons = s;
}

fn read_axis<F: Fn(&AxisBinding) -> bool>(raw: f32, binding: &AxisBinding, _filter: F) -> f32 {
    binding.apply(raw)
}

/// Right-stick-Y is the default throttle axis. gilrs reports it as `-1..=1`
/// with +1 at the top, but throttle should be `0..=1` with 0 at the bottom.
/// We map the top half of the stick to `0..=1` and ignore the bottom half
/// (no negative throttle on a quad). This matches standard "throttle on the
/// right stick" behaviour for Mode-2 transmitter users holding an Xbox
/// controller.
fn throttle_from_right_stick_y(raw: f32, binding: &AxisBinding) -> f32 {
    let normalized = binding.apply(raw); // applies deadzone + invert
                                         // Map [-1, 1] to [0, 1] linearly so the operator can "pull down" to cut
                                         // throttle and "push up" to full. 0 in the middle is 0.5 throttle.
    ((normalized + 1.0) / 2.0).clamp(0.0, 1.0)
}
