//! Figment-loaded configuration. `/etc/legion/config.toml` at runtime,
//! overridable by `LEGION_*` environment variables.

use std::path::PathBuf;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LegionError {
    #[error("config: {0}")]
    Config(#[from] figment::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol codec: {0}")]
    Codec(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("executor: {0}")]
    Executor(String),
    #[error("{0}")]
    Other(String),
}

/// Top-level config. Matches `legion/README.md#configuration`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub drone: DroneConfig,
    pub mavlink: MavlinkConfig,
    pub transport: TransportConfig,
    pub oracle: OracleConfig,
    pub safety: SafetyConfigToml,
    pub storage: StorageConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            drone: DroneConfig::default(),
            mavlink: MavlinkConfig::default(),
            transport: TransportConfig::default(),
            oracle: OracleConfig::default(),
            safety: SafetyConfigToml::default(),
            storage: StorageConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneConfig {
    pub id: String,
    #[serde(default = "default_capabilities")]
    pub capabilities: Vec<String>,
}

fn default_capabilities() -> Vec<String> {
    vec!["spray".into(), "tof".into()]
}

impl Default for DroneConfig {
    fn default() -> Self {
        Self {
            id: "drone-dev".into(),
            capabilities: default_capabilities(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavlinkConfig {
    /// Pixhawk address in the form understood by `rust-mavlink`, e.g.
    /// `"serial:/dev/ttyAMA0:921600"`. For v1 stub-backend builds this
    /// is informational only.
    #[serde(default = "default_mavlink_address")]
    pub address: String,
    #[serde(default = "default_mavlink_connect_timeout")]
    pub connect_timeout_s: u64,
}

impl Default for MavlinkConfig {
    fn default() -> Self {
        Self {
            address: default_mavlink_address(),
            connect_timeout_s: default_mavlink_connect_timeout(),
        }
    }
}

fn default_mavlink_address() -> String {
    "serial:/dev/ttyAMA0:921600".into()
}

fn default_mavlink_connect_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    Serial {
        path: String,
        #[serde(default = "default_baud")]
        baud: u32,
    },
    Tcp {
        addr: String,
    },
}

fn default_baud() -> u32 {
    57_600
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self::Tcp {
            addr: "127.0.0.1:7346".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    #[serde(default = "default_heartbeat_hz")]
    pub heartbeat_hz: u32,
    #[serde(default = "default_reconnect_initial_s")]
    pub reconnect_initial_s: f32,
    #[serde(default = "default_reconnect_max_s")]
    pub reconnect_max_s: f32,
}

fn default_heartbeat_hz() -> u32 {
    2
}

fn default_reconnect_initial_s() -> f32 {
    1.0
}

fn default_reconnect_max_s() -> f32 {
    30.0
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            heartbeat_hz: default_heartbeat_hz(),
            reconnect_initial_s: default_reconnect_initial_s(),
            reconnect_max_s: default_reconnect_max_s(),
        }
    }
}

/// TOML-facing shape for the safety section. Kept separate from
/// `legion_core::SafetyConfig` because the TOML schema uses seconds (a
/// f32) while the core uses integer milliseconds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SafetyConfigToml {
    #[serde(default = "default_tof_min_cm")]
    pub tof_min_cm: f32,
    #[serde(default = "default_battery_critical_pct")]
    pub battery_critical_pct: f32,
    #[serde(default = "default_paint_empty_ml")]
    pub paint_empty_ml: f32,
    #[serde(default = "default_oracle_silent_s")]
    pub oracle_silent_s: f32,
}

fn default_tof_min_cm() -> f32 {
    30.0
}

fn default_battery_critical_pct() -> f32 {
    15.0
}

fn default_paint_empty_ml() -> f32 {
    20.0
}

fn default_oracle_silent_s() -> f32 {
    5.0
}

impl Default for SafetyConfigToml {
    fn default() -> Self {
        Self {
            tof_min_cm: default_tof_min_cm(),
            battery_critical_pct: default_battery_critical_pct(),
            paint_empty_ml: default_paint_empty_ml(),
            oracle_silent_s: default_oracle_silent_s(),
        }
    }
}

impl SafetyConfigToml {
    /// Convert into the core's integer-ms shape.
    pub fn to_core(self) -> legion_core::SafetyConfig {
        legion_core::SafetyConfig {
            tof_min_cm: self.tof_min_cm,
            battery_critical_pct: self.battery_critical_pct,
            paint_empty_ml: self.paint_empty_ml,
            oracle_silent_ms: (self.oracle_silent_s * 1000.0) as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_sortie_dir")]
    pub sortie_dir: PathBuf,
}

fn default_sortie_dir() -> PathBuf {
    PathBuf::from("/var/lib/legion/sorties")
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            sortie_dir: default_sortie_dir(),
        }
    }
}

impl Config {
    /// Load from the given TOML file, with defaults and env overrides
    /// layered underneath. Missing files fall back to defaults so
    /// `legion debug status` works without a `/etc/legion/config.toml`.
    pub fn load(path: Option<&std::path::Path>) -> Result<Self, LegionError> {
        let mut fig = Figment::from(Serialized::defaults(Config::default()));
        if let Some(p) = path {
            if p.exists() {
                fig = fig.merge(Toml::file(p));
            }
        }
        fig = fig.merge(Env::prefixed("LEGION_").split("__"));
        fig.extract().map_err(Into::into)
    }
}
