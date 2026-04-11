//! `TelemetrySnapshot` — the shape of the data the HUD renders.
//!
//! This is a deliberately narrow subset of what MAVLink messages carry:
//! every field corresponds to something the operator actually needs to see
//! on the screen while flying manually. New fields get added here only when
//! a new HUD widget demands one.
//!
//! Has NO dependency on the `mavlink` crate — the conversion from
//! `MavMessage` variants into this struct lives in `recv.rs`, so this type
//! stays simple to serialize through Tauri events.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TelemetrySnapshot {
    pub attitude: Attitude,
    pub position: Position,
    pub battery: Battery,
    pub gps: Gps,
    pub tof_distance_m: Option<f32>,
    pub radio: Radio,
    pub heartbeat: Heartbeat,

    /// Milliseconds since UNIX epoch at which the snapshot was last updated
    /// by the receiver task. The frontend uses this to detect staleness.
    pub updated_at_ms: u64,
}

impl TelemetrySnapshot {
    pub fn touch(&mut self) {
        self.updated_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Attitude {
    pub roll_rad: f32,
    pub pitch_rad: f32,
    pub yaw_rad: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Position {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: f32,
    pub relative_alt_m: f32,
    pub vx_m_s: f32,
    pub vy_m_s: f32,
    pub vz_m_s: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Battery {
    pub voltage_v: f32,
    pub current_a: f32,
    /// 0..100 — directly from `SYS_STATUS.battery_remaining`.
    pub remaining_pct: i8,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Gps {
    pub fix: GpsFix,
    pub satellites_visible: u8,
    pub eph_cm: u16,
    pub epv_cm: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpsFix {
    #[default]
    None,
    Fix2d,
    Fix3d,
    Dgps,
    RtkFloat,
    RtkFixed,
}

impl GpsFix {
    /// Convert from the `MAV_GPS_FIX_TYPE` integer carried in `GPS_RAW_INT`.
    pub fn from_mav(n: u32) -> Self {
        match n {
            2 => Self::Fix2d,
            3 => Self::Fix3d,
            4 => Self::Dgps,
            5 => Self::RtkFloat,
            6 => Self::RtkFixed,
            // 0, 1, and any unknown value all mean "no usable fix"
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Radio {
    pub rssi: u8,
    pub remrssi: u8,
    pub noise: u8,
    pub remnoise: u8,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Heartbeat {
    pub armed: bool,
    /// PX4 custom mode enum as a raw u32 — decoded to a human-readable
    /// string in the frontend, or left as-is.
    pub custom_mode: u32,
    pub base_mode: u8,
    pub system_status: u8,
    /// Milliseconds since we last received a HEARTBEAT. The HUD flashes
    /// when this grows past the watchdog threshold.
    pub age_ms: u64,
}
