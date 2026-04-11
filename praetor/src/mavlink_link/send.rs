//! Outbound MAVLink — builders for `MANUAL_CONTROL`, `COMMAND_LONG`,
//! and `DO_SET_ACTUATOR`.
//!
//! Every outbound command flows through a single `send` function that
//! takes the already-built `MavMessage` and does two things:
//!
//!   1. Write it to the audit log (phase 3+ — currently a `tracing` call).
//!   2. Hand it to the `mavlink` crate's blocking `send_default()` via
//!      `tokio::task::spawn_blocking`.
//!
//! Callers construct the `MavMessage` themselves using the builder fns in
//! this module — this keeps the "which command did I send" layer visible
//! at the call site (the Tauri command handlers).

use mavlink::common::{MavCmd, MavMessage, COMMAND_LONG_DATA, MANUAL_CONTROL_DATA};
use tracing::debug;

use crate::error::{PraetorError, Result};
use crate::gamepad::intent::ManualControlFrame;
use crate::mavlink_link::connect::MavConn;

/// `MANUAL_CONTROL` at the configured cadence (20 Hz from the send task).
#[must_use]
pub fn build_manual_control(target: u8, frame: ManualControlFrame) -> MavMessage {
    MavMessage::MANUAL_CONTROL(MANUAL_CONTROL_DATA {
        target,
        x: frame.x,
        y: frame.y,
        z: frame.z,
        r: frame.r,
        buttons: 0,
    })
}

/// `MAV_CMD_COMPONENT_ARM_DISARM` — arm (`arm = true`) or disarm.
#[must_use]
pub fn build_arm(target_system: u8, target_component: u8, arm: bool) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
        confirmation: 0,
        param1: if arm { 1.0 } else { 0.0 },
        param2: 0.0,
        param3: 0.0,
        param4: 0.0,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    })
}

/// Emergency motor cut: disarm with PX4's documented `21196` magic value
/// in `param2`. PX4 treats this as "force disarm even if airborne".
#[must_use]
pub fn build_emergency_disarm(target_system: u8, target_component: u8) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
        confirmation: 0,
        param1: 0.0,
        param2: 21196.0,
        param3: 0.0,
        param4: 0.0,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    })
}

/// `MAV_CMD_NAV_TAKEOFF` to a relative altitude in metres.
#[must_use]
pub fn build_takeoff(target_system: u8, target_component: u8, altitude_m: f32) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_NAV_TAKEOFF,
        confirmation: 0,
        param1: 0.0,      // minimum pitch
        param2: 0.0,      // empty
        param3: 0.0,      // empty
        param4: f32::NAN, // yaw — NaN = hold current
        param5: f32::NAN, // lat — NaN = current position
        param6: f32::NAN, // lon
        param7: altitude_m,
    })
}

/// `MAV_CMD_NAV_LAND` at the current position.
#[must_use]
pub fn build_land(target_system: u8, target_component: u8) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_NAV_LAND,
        confirmation: 0,
        param1: 0.0, // abort altitude
        param2: 0.0, // precision land mode
        param3: 0.0,
        param4: f32::NAN, // yaw
        param5: f32::NAN, // lat
        param6: f32::NAN, // lon
        param7: 0.0,      // altitude
    })
}

/// `MAV_CMD_NAV_RETURN_TO_LAUNCH`.
#[must_use]
pub fn build_rtl(target_system: u8, target_component: u8) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_NAV_RETURN_TO_LAUNCH,
        confirmation: 0,
        param1: 0.0,
        param2: 0.0,
        param3: 0.0,
        param4: 0.0,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    })
}

/// `MAV_CMD_DO_SET_SERVO` for the pump on AUX5.
///
/// This matches the project decision in `project_hardware.md`: **the nozzle
/// servo is wired to Pixhawk AUX5 and driven by MAVLink `DO_SET_SERVO`
/// (servo index 5, PWM 2000 for ON / 1000 for OFF)**. There is deliberately
/// no alternative wiring path — praetor and legion both hit the Pixhawk
/// through the same MAVLink command so there is a single control path and
/// the pump inherits PX4's servo failsafes.
///
/// Parameters:
///
/// - `servo_index`: the Pixhawk output channel number (1..16). For the
///   v1 hardware this is `5` (AUX5).
/// - `pwm_us`: the PWM pulse width in microseconds, typically 1000–2000.
#[must_use]
pub fn build_set_servo(
    target_system: u8,
    target_component: u8,
    servo_index: u8,
    pwm_us: u16,
) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_DO_SET_SERVO,
        confirmation: 0,
        param1: f32::from(servo_index),
        param2: f32::from(pwm_us),
        param3: 0.0,
        param4: 0.0,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    })
}

/// `MAV_CMD_DO_SET_MODE` — used by the Start button to switch PX4 out of
/// Offboard (legion-driven) into a manual mode, and back again.
#[must_use]
pub fn build_set_mode(
    target_system: u8,
    target_component: u8,
    base_mode: u8,
    custom_mode: f32,
) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: MavCmd::MAV_CMD_DO_SET_MODE,
        confirmation: 0,
        param1: f32::from(base_mode),
        param2: custom_mode,
        param3: 0.0,
        param4: 0.0,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    })
}

/// Send a built message over the connection. Uses `spawn_blocking` so the
/// tokio runtime doesn't stall on a slow serial write.
pub async fn send(conn: MavConn, msg: MavMessage) -> Result<()> {
    debug!(?msg, "sending mavlink frame");
    tokio::task::spawn_blocking(move || {
        conn.send_default(&msg)
            .map(|_n| ())
            .map_err(|e| PraetorError::Mavlink(format!("send: {e}")))
    })
    .await?
}
