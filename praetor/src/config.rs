//! Configuration — `praetor.toml` + env var overrides.
//!
//! Loaded once at startup via `figment` with this precedence (lower → higher):
//!
//!   1. Built-in defaults (`Config::default()`)
//!   2. `$HOME/.config/praetor/praetor.toml` (user global)
//!   3. `./praetor.toml` (project-local)
//!   4. env vars prefixed `PRAETOR_` (e.g. `PRAETOR_LINK_ADDRESS=tcp:…`)

use std::path::PathBuf;

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

use crate::error::{PraetorError, Result};
use crate::gamepad::binding::Bindings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub link: LinkConfig,
    pub gamepad: GamepadConfig,
    pub safety: SafetyConfig,
    pub pump: PumpConfig,
    pub takeoff: TakeoffConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfig {
    /// Either `serial:/dev/ttyUSB1:57600` or `tcp:127.0.0.1:5760`.
    pub address: String,
    pub drone_system_id: u8,
    pub target_component_id: u8,
    pub watchdog: LinkWatchdogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkWatchdogConfig {
    pub link_silent_threshold_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadConfig {
    pub poll_hz: u32,
    pub silent_threshold_s: f32,
    pub hard_silent_threshold_s: f32,
    pub bindings: Bindings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub arm_hold_duration_s: f32,
    pub emergency_stop_hold_s: f32,
    pub pump_minimum_altitude_m: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpConfig {
    /// Pixhawk servo output channel the nozzle servo is wired to. Sent as
    /// `MAV_CMD_DO_SET_SERVO.param1`. For the v1 hardware this is `5` (AUX5
    /// on a Pixhawk 6C). This is the only supported wiring per
    /// `project_hardware.md` — do not plumb a Pi-GPIO alternative.
    pub servo_index: u8,
    /// PWM pulse width (µs) for pump-on. Default 2000 (full servo travel).
    pub pwm_on_us: u16,
    /// PWM pulse width (µs) for pump-off. Default 1000 (zero servo travel).
    pub pwm_off_us: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeoffConfig {
    pub default_altitude_m: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            link: LinkConfig {
                address: "tcp:127.0.0.1:5760".to_owned(),
                drone_system_id: 1,
                target_component_id: 1,
                watchdog: LinkWatchdogConfig {
                    link_silent_threshold_s: 3.0,
                },
            },
            gamepad: GamepadConfig {
                poll_hz: 100,
                silent_threshold_s: 1.0,
                hard_silent_threshold_s: 3.0,
                bindings: Bindings::default(),
            },
            safety: SafetyConfig {
                arm_hold_duration_s: 3.0,
                emergency_stop_hold_s: 1.0,
                pump_minimum_altitude_m: 0.5,
            },
            pump: PumpConfig {
                servo_index: 5,
                pwm_on_us: 2000,
                pwm_off_us: 1000,
            },
            takeoff: TakeoffConfig {
                default_altitude_m: 2.0,
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut fig = Figment::from(Serialized::defaults(Config::default()));

        if let Some(user_path) = user_config_path() {
            if user_path.exists() {
                fig = fig.merge(Toml::file(user_path));
            }
        }

        let local = PathBuf::from("praetor.toml");
        if local.exists() {
            fig = fig.merge(Toml::file(local));
        }

        fig = fig.merge(Env::prefixed("PRAETOR_").split("__"));

        fig.extract()
            .map_err(|e| PraetorError::Config(format!("figment: {e}")))
    }
}

fn user_config_path() -> Option<PathBuf> {
    // We don't pull in the `dirs` crate just for this — $HOME is reliable on
    // macOS/Linux and irrelevant on Windows (no Windows support in v1).
    std::env::var_os("HOME").map(|home| {
        let mut p = PathBuf::from(home);
        p.push(".config");
        p.push("praetor");
        p.push("praetor.toml");
        p
    })
}
