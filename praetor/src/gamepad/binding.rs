//! Controller axis/button bindings — the config-driven lookup the poller uses
//! to turn raw gilrs events into [`crate::gamepad::intent::ControlIntent`] fields.
//!
//! The binding set is expressed as serde-friendly enum values so it round-trips
//! through `praetor.toml`. The string values match gilrs's own names so an
//! operator can add a custom mapping without learning our internal vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bindings {
    pub roll: AxisBinding,
    pub pitch: AxisBinding,
    pub yaw: AxisBinding,
    pub throttle: AxisBinding,

    pub pump: Button,
    pub rtl: Button,
    pub takeoff: Button,
    pub land: Button,
    pub mode_cycle: Button,
    pub emergency: Button,

    /// Two buttons that must be held together for arming.
    pub arm_combo: (Button, Button),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisBinding {
    pub axis: Axis,
    #[serde(default)]
    pub invert: bool,
    /// Deadzone around zero (normalized). Below this absolute value, the
    /// axis reads as exactly 0.
    #[serde(default = "default_deadzone")]
    pub deadzone: f32,
}

const fn default_deadzone() -> f32 {
    0.08
}

impl AxisBinding {
    /// Apply deadzone + inversion to a raw gilrs axis reading (`-1..=1`).
    pub fn apply(&self, raw: f32) -> f32 {
        let mut v = raw;
        if v.abs() < self.deadzone {
            v = 0.0;
        }
        if self.invert {
            v = -v;
        }
        v.clamp(-1.0, 1.0)
    }
}

/// gilrs axis names. Only the axes we actually use are listed — not every
/// `gilrs::Axis` variant maps to anything useful on an Xbox controller.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Axis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftZ,  // LT trigger
    RightZ, // RT trigger
}

/// gilrs button names. Aliases ("A", "B", ...) are accepted on input so the
/// config file can read naturally.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Button {
    South,       // A on Xbox
    East,        // B
    North,       // Y
    West,        // X
    LeftTrigger, // LB (the bumper, not LT analog trigger)
    RightTrigger,
    LeftTrigger2,  // LT
    RightTrigger2, // RT
    Select,        // "Back"
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            roll: AxisBinding {
                axis: Axis::LeftStickX,
                invert: false,
                deadzone: default_deadzone(),
            },
            pitch: AxisBinding {
                axis: Axis::LeftStickY,
                invert: true, // gilrs reports up as +Y; pitch-forward = stick-forward = +Y
                deadzone: default_deadzone(),
            },
            throttle: AxisBinding {
                axis: Axis::RightStickY,
                invert: false,
                deadzone: default_deadzone(),
            },
            yaw: AxisBinding {
                axis: Axis::RightStickX,
                invert: false,
                deadzone: default_deadzone(),
            },
            pump: Button::South,
            rtl: Button::East,
            takeoff: Button::North,
            land: Button::West,
            mode_cycle: Button::Start,
            emergency: Button::Select,
            arm_combo: (Button::LeftTrigger, Button::RightTrigger),
        }
    }
}

impl Button {
    /// Map our named button to a gilrs `Button`. Kept as a `pub(crate)` helper
    /// so `gamepad::mod` owns the only reference to `gilrs::Button`.
    pub(crate) fn to_gilrs(self) -> gilrs::Button {
        use gilrs::Button as G;
        match self {
            Button::South => G::South,
            Button::East => G::East,
            Button::North => G::North,
            Button::West => G::West,
            Button::LeftTrigger => G::LeftTrigger,
            Button::RightTrigger => G::RightTrigger,
            Button::LeftTrigger2 => G::LeftTrigger2,
            Button::RightTrigger2 => G::RightTrigger2,
            Button::Select => G::Select,
            Button::Start => G::Start,
            Button::Mode => G::Mode,
            Button::LeftThumb => G::LeftThumb,
            Button::RightThumb => G::RightThumb,
            Button::DPadUp => G::DPadUp,
            Button::DPadDown => G::DPadDown,
            Button::DPadLeft => G::DPadLeft,
            Button::DPadRight => G::DPadRight,
        }
    }
}

impl Axis {
    /// Map our named axis to a gilrs `Axis`. Currently only used from tests
    /// and a planned config-driven axis override; keep compiled so the
    /// mapping doesn't silently rot. Removing `#[allow(dead_code)]` is a
    /// FIXME once the dynamic-binding code path lands in the poller.
    #[allow(dead_code)]
    pub(crate) fn to_gilrs(self) -> gilrs::Axis {
        use gilrs::Axis as G;
        match self {
            Axis::LeftStickX => G::LeftStickX,
            Axis::LeftStickY => G::LeftStickY,
            Axis::RightStickX => G::RightStickX,
            Axis::RightStickY => G::RightStickY,
            Axis::LeftZ => G::LeftZ,
            Axis::RightZ => G::RightZ,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_suppresses_small_inputs() {
        let b = AxisBinding {
            axis: Axis::LeftStickX,
            invert: false,
            deadzone: 0.1,
        };
        assert_eq!(b.apply(0.05), 0.0);
        assert!((b.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn invert_flips_sign() {
        let b = AxisBinding {
            axis: Axis::LeftStickY,
            invert: true,
            deadzone: 0.0,
        };
        assert!((b.apply(0.5) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn clamp_handles_oob_input() {
        let b = AxisBinding {
            axis: Axis::LeftStickX,
            invert: false,
            deadzone: 0.0,
        };
        assert_eq!(b.apply(2.0), 1.0);
        assert_eq!(b.apply(-2.0), -1.0);
    }
}
