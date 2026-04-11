//! Inbound MAVLink — drains the receiver, converts to `TelemetrySnapshot`,
//! publishes to the shared `watch::Sender`, updates link status.
//!
//! Runs on a dedicated OS thread (not a tokio task) because the `mavlink`
//! crate's `recv()` is blocking and we don't want to hold a tokio runtime
//! thread spinning on it. State updates reach the tokio side via
//! `watch::Sender::send_replace`, which is `!Send`-safe to call from any
//! thread once the Sender handle is in hand.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mavlink::common::{MavMessage, MavModeFlag};
use tracing::{debug, warn};

use crate::mavlink_link::connect::MavConn;
use crate::mavlink_link::snapshot::{
    Attitude, Battery, GpsFix, Heartbeat, Position, Radio, TelemetrySnapshot,
};
use crate::state::{AppState, LinkStatus};

/// Spawn the receiver thread. Returns immediately; the thread runs for the
/// life of the connection and exits (gracefully) when the connection is
/// dropped by the parent task's `set_link(None)` call via a `running`
/// AtomicBool check.
pub fn spawn(
    conn: MavConn,
    state: AppState,
    running: Arc<std::sync::atomic::AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("praetor-mav-recv".into())
        .spawn(move || recv_loop(conn, state, running))
        .expect("failed to spawn mav recv thread")
}

#[allow(clippy::needless_pass_by_value)] // all three args are moved into the spawned OS thread
fn recv_loop(conn: MavConn, state: AppState, running: Arc<std::sync::atomic::AtomicBool>) {
    let mut last_heartbeat = Instant::now();
    let link_silent_threshold =
        Duration::from_secs_f32(state.config.link.watchdog.link_silent_threshold_s);

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        match conn.recv() {
            Ok((header, msg)) => {
                // Filter to our configured drone. We do not silently drop
                // other system IDs — we log them so the operator notices if
                // they pointed praetor at the wrong endpoint — but we do
                // not apply them to our snapshot.
                if header.system_id != state.config.link.drone_system_id {
                    debug!(
                        got = header.system_id,
                        expected = state.config.link.drone_system_id,
                        ?msg,
                        "ignoring frame from unexpected system id"
                    );
                    continue;
                }

                apply_message(&state, &msg, &mut last_heartbeat);
            }
            Err(e) => {
                // mavlink's recv returns an error on any parse issue or
                // serial read error. We log and keep looping — transient
                // errors are normal on a real radio link.
                warn!(error = ?e, "mavlink recv error");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        // Every loop, check whether we've gone silent for too long.
        if last_heartbeat.elapsed() > link_silent_threshold
            && *state.link_status.borrow() == LinkStatus::Connected
        {
            warn!(
                elapsed_s = last_heartbeat.elapsed().as_secs_f32(),
                "HEARTBEAT silent — marking link stale"
            );
            let _ = state.link_status_tx.send(LinkStatus::Stale);
        }
    }

    debug!("recv loop exiting");
}

/// Apply a single parsed MAVLink message to the shared snapshot. The
/// snapshot is read-modify-write via the watch channel to keep every
/// field up to date even if the operator only looks at one of them.
fn apply_message(state: &AppState, msg: &MavMessage, last_heartbeat: &mut Instant) {
    let mut snap: TelemetrySnapshot = state.telemetry.borrow().clone();

    match msg {
        MavMessage::HEARTBEAT(hb) => {
            *last_heartbeat = Instant::now();
            snap.heartbeat = Heartbeat {
                armed: hb
                    .base_mode
                    .contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED),
                custom_mode: hb.custom_mode,
                base_mode: hb.base_mode.bits(),
                system_status: hb.system_status as u8,
                age_ms: 0,
            };
            if *state.link_status.borrow() != LinkStatus::Connected {
                let _ = state.link_status_tx.send(LinkStatus::Connected);
            }
        }
        MavMessage::ATTITUDE(a) => {
            snap.attitude = Attitude {
                roll_rad: a.roll,
                pitch_rad: a.pitch,
                yaw_rad: a.yaw,
            };
        }
        MavMessage::GLOBAL_POSITION_INT(p) => {
            snap.position = Position {
                lat_deg: f64::from(p.lat) / 1e7,
                lon_deg: f64::from(p.lon) / 1e7,
                alt_msl_m: (p.alt as f32) / 1000.0,
                relative_alt_m: (p.relative_alt as f32) / 1000.0,
                vx_m_s: f32::from(p.vx) / 100.0,
                vy_m_s: f32::from(p.vy) / 100.0,
                vz_m_s: f32::from(p.vz) / 100.0,
            };
        }
        MavMessage::GPS_RAW_INT(g) => {
            snap.gps = crate::mavlink_link::snapshot::Gps {
                fix: GpsFix::from_mav(g.fix_type as u32),
                satellites_visible: g.satellites_visible,
                eph_cm: g.eph,
                epv_cm: g.epv,
            };
        }
        MavMessage::SYS_STATUS(s) => {
            // `SYS_STATUS.battery_remaining` is a percentage 0..100, or -1
            // if unknown. Current is in centiamps; voltage in mV.
            snap.battery = Battery {
                voltage_v: f32::from(s.voltage_battery) / 1000.0,
                current_a: f32::from(s.current_battery) / 100.0,
                remaining_pct: s.battery_remaining,
            };
        }
        MavMessage::BATTERY_STATUS(b) => {
            // Newer "rich" battery message — takes precedence over SYS_STATUS
            // when present. Voltages are mV in the `voltages` array; the
            // first non-UINT16_MAX entry is cell 1.
            if let Some(v) = b.voltages.iter().find(|v| **v != u16::MAX) {
                snap.battery.voltage_v = f32::from(*v) / 1000.0;
            }
            snap.battery.current_a = f32::from(b.current_battery) / 100.0;
            snap.battery.remaining_pct = b.battery_remaining;
        }
        MavMessage::DISTANCE_SENSOR(d) => {
            // Only trust the downward-facing sensor (id = 0) for the HUD
            // readout — if the hardware has multiple ToFs we'll extend
            // this later.
            snap.tof_distance_m = Some(f32::from(d.current_distance) / 100.0);
        }
        MavMessage::RADIO_STATUS(r) => {
            snap.radio = Radio {
                rssi: r.rssi,
                remrssi: r.remrssi,
                noise: r.noise,
                remnoise: r.remnoise,
            };
        }
        MavMessage::STATUSTEXT(_t) => {
            // Log verbatim — the UI surfaces these in a toast via the
            // event emitter, not the snapshot.
            debug!(?msg, "STATUSTEXT");
        }
        _ => {
            // Ignored: everything else.
        }
    }

    snap.touch();
    state.telemetry_tx.send_replace(snap);
}
