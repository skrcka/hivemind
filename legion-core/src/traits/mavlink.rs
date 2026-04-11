//! MAVLink driver abstraction.
//!
//! The executor and safety loop never touch the `mavlink` crate directly
//! — they go through this trait. The Pi binary provides a
//! `RustMavlinkDriver` impl that wraps `mavlink` + `tokio-serial` on
//! TELEM2; a future MCU binary provides `EmbassyMavlinkDriver` using
//! `embassy-stm32-usart` or similar.
//!
//! All methods are `async` because the real driver needs to wait for
//! ACKs, position deltas, altitude reach, etc. Unit tests use a mock
//! that resolves every future immediately.

use core::future::Future;

use crate::error::MavlinkError;
use hivemind_protocol::{Position, Waypoint};

/// Everything the executor, radio-loss policy, and safety loop need from
/// the autopilot.
///
/// Implementations are expected to be `&self`-only for the commands —
/// concurrent access is expected (the safety loop and the executor both
/// need to command the Pixhawk). The binary's concrete driver uses
/// interior mutability (typically a `tokio::sync::Mutex` around the
/// `MavConnection`) to serialize the actual byte writes.
pub trait MavlinkBackend: Send + Sync {
    fn arm(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;
    fn disarm(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Command a takeoff to `alt_m` metres above launch point. Resolves
    /// once the drone has reached the target altitude.
    fn takeoff(&self, alt_m: f32) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Fly to a single GPS waypoint at `speed_m_s`. Resolves once the
    /// drone has reached within the driver's configured tolerance.
    fn goto(
        &self,
        wp: Waypoint,
        speed_m_s: f32,
    ) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Follow a path of waypoints in offboard mode. Resolves once the
    /// drone has reached the last waypoint.
    fn follow_path(
        &self,
        path: &[Waypoint],
        speed_m_s: f32,
    ) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Command RTL. Resolves once the drone has landed at home.
    fn return_to_launch(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Command a local LAND at the current position. Resolves once
    /// touchdown is detected.
    fn land(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Hold position (PX4 loiter mode). Resolves as soon as the mode
    /// change is acknowledged, *not* after any particular hold duration.
    fn hold(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Safety-loop-only: back away from whatever's in front of the ToF
    /// sensor and switch to HOLD. Used by `SafetyOutcome::TofAvoidance`.
    fn emergency_pullback(&self) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Inject RTCM3 bytes for RTK corrections. Fragments as needed into
    /// `GPS_RTCM_DATA` MAVLink messages.
    fn inject_rtk(
        &self,
        rtcm: &[u8],
    ) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Command the nozzle servo. `true` = pressed (spray on),
    /// `false` = released (spray off).
    ///
    /// v1 wiring: the SG90 sits on Pixhawk AUX5 (see
    /// `hw/nozzle/README.md`). The impl maps to a
    /// `MAV_CMD_DO_SET_SERVO` for servo index 5 with PWM 2000/1000,
    /// or to an AUX5 actuator-output override, per PX4's parameter
    /// setup.
    ///
    /// This method is `&self` so the safety loop and the executor
    /// can both call it concurrently — the driver serializes at
    /// its own interior-mutability layer.
    fn set_nozzle(
        &self,
        open: bool,
    ) -> impl Future<Output = Result<(), MavlinkError>> + Send;

    /// Latest decoded global position. Cached by the driver's telemetry
    /// decoder; never blocks.
    fn position(&self) -> Position;

    /// Latest decoded battery percentage. Cached by the driver.
    fn battery_pct(&self) -> f32;
}
