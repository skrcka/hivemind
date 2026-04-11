//! Typed control intent — what the human-on-the-stick wants, normalized.
//!
//! This is the layer *between* the gamepad and MAVLink. The gamepad poller
//! publishes `ControlIntent`; the MAVLink sender subscribes and converts it
//! into `MANUAL_CONTROL` / `COMMAND_LONG` frames at its own cadence. The
//! intermediate type exists so both sides stay decoupled: swap a controller
//! binding, or swap MAVLink for something else, and only one side changes.

use std::time::Instant;

use serde::Serialize;

/// The latest snapshot of what the operator is asking for.
///
/// All axes are normalized to `-1.0 ..= 1.0`; throttle is `0.0 ..= 1.0`.
/// Buttons are edge-triggered from the gamepad side but latched here so a
/// consumer at an arbitrary cadence sees a stable state.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ControlIntent {
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub throttle: f32,

    /// Fine trim overrides, applied on top of the main axes.
    /// (`-1.0` = full nudge left/down, `+1.0` = full nudge right/up, `0` = no trim.)
    pub trim_roll: f32,
    pub trim_pitch: f32,

    pub buttons: ButtonStates,

    /// Monotonic time of the last gamepad *event* of any kind. The watchdog
    /// uses `now.duration_since(last_event_at)` to decide the controller is
    /// silent. Not serialized — it's a pure internal field.
    #[serde(skip)]
    pub last_event_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ButtonStates {
    /// "A" on Xbox — hold to spray.
    pub pump: bool,
    /// "B" — return to launch.
    pub rtl: bool,
    /// "Y" — takeoff.
    pub takeoff: bool,
    /// "X" — land.
    pub land: bool,
    /// LB + RB combo held (arming request).
    pub arm_combo: bool,
    /// Start — cycle mode.
    pub mode_cycle: bool,
    /// Back — emergency motor cut (must be held > emergency_stop_hold_s).
    pub emergency: bool,
}

impl ControlIntent {
    pub const fn neutral() -> Self {
        Self {
            roll: 0.0,
            pitch: 0.0,
            yaw: 0.0,
            throttle: 0.0,
            trim_roll: 0.0,
            trim_pitch: 0.0,
            buttons: ButtonStates {
                pump: false,
                rtl: false,
                takeoff: false,
                land: false,
                arm_combo: false,
                mode_cycle: false,
                emergency: false,
            },
            last_event_at: None,
        }
    }

    /// Effective roll after applying the d-pad trim.
    pub fn effective_roll(&self) -> f32 {
        (self.roll + self.trim_roll).clamp(-1.0, 1.0)
    }

    /// Effective pitch after applying the d-pad trim.
    pub fn effective_pitch(&self) -> f32 {
        (self.pitch + self.trim_pitch).clamp(-1.0, 1.0)
    }
}

impl Default for ControlIntent {
    fn default() -> Self {
        Self::neutral()
    }
}

/// MAVLink `MANUAL_CONTROL` expects signed i16 in `-1000..1000` and throttle
/// `0..1000` (with 500 as "neutral"). We compute those from the normalized
/// [`ControlIntent`] in one place so the MAVLink sender has no float-conversion
/// logic of its own.
#[derive(Debug, Clone, Copy)]
pub struct ManualControlFrame {
    pub x: i16, // pitch
    pub y: i16, // roll
    pub z: i16, // throttle (0..1000)
    pub r: i16, // yaw
}

impl ManualControlFrame {
    /// Convert a normalized intent into MAVLink-ready integers.
    pub fn from_intent(intent: &ControlIntent) -> Self {
        let pitch_i = f_to_signed_i16(intent.effective_pitch());
        let roll_i = f_to_signed_i16(intent.effective_roll());
        let yaw_i = f_to_signed_i16(intent.yaw);

        // Throttle: 0..1.0 maps to 0..1000. 0 throttle becomes 0, full
        // becomes 1000. PX4 in MANUAL / ALT mode treats 500 as "hover" for
        // the `z` channel when the ManualControl hint is used, so we bias
        // the operator's 0..1.0 over 0..1000 and let PX4 interpret.
        let throttle_i = f_to_unsigned_i16(intent.throttle);

        Self {
            x: pitch_i,
            y: roll_i,
            z: throttle_i,
            r: yaw_i,
        }
    }

    /// Centred sticks — what the controller-silent watchdog sends.
    pub const fn neutral() -> Self {
        Self {
            x: 0,
            y: 0,
            z: 500,
            r: 0,
        }
    }
}

fn f_to_signed_i16(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * 1000.0).round() as i16
}

fn f_to_unsigned_i16(v: f32) -> i16 {
    (v.clamp(0.0, 1.0) * 1000.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_all_zeros_except_throttle_midpoint() {
        let f = ManualControlFrame::from_intent(&ControlIntent::neutral());
        assert_eq!(f.x, 0);
        assert_eq!(f.y, 0);
        assert_eq!(f.r, 0);
        assert_eq!(f.z, 0); // intent throttle is 0, so frame throttle is 0
        let n = ManualControlFrame::neutral();
        assert_eq!(n.z, 500);
    }

    #[test]
    fn full_stick_maps_to_1000() {
        let mut i = ControlIntent::neutral();
        i.roll = 1.0;
        i.pitch = -1.0;
        i.yaw = 0.5;
        i.throttle = 1.0;
        let f = ManualControlFrame::from_intent(&i);
        assert_eq!(f.y, 1000);
        assert_eq!(f.x, -1000);
        assert_eq!(f.r, 500);
        assert_eq!(f.z, 1000);
    }

    #[test]
    fn trim_applied_on_top_and_clamped() {
        let mut i = ControlIntent::neutral();
        i.roll = 0.8;
        i.trim_roll = 0.5;
        assert!((i.effective_roll() - 1.0).abs() < 1e-6);
    }
}
