//! Oracle configuration. Loaded from a TOML file with environment overlays
//! via figment.
//!
//! Default config path is `oracle.toml` in the working directory; override
//! with `--config <path>` on the CLI or `ORACLE__CONFIG` env var.

use std::path::{Path, PathBuf};

use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OracleConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub legion_link: LegionLinkConfig,
    pub rtk: RtkConfig,
    pub slicer: SlicerConfig,
    pub safety: SafetyConfig,
}

impl OracleConfig {
    /// Load from `path`, layering on env-var overrides with prefix `ORACLE__`.
    #[allow(clippy::result_large_err)] // figment::Error is the figment-side type
    pub fn load(path: Option<&Path>) -> Result<Self, figment::Error> {
        let mut fig = Figment::from(figment::providers::Serialized::defaults(Self::default()));
        if let Some(p) = path {
            fig = fig.merge(Toml::file(p));
        } else {
            fig = fig.merge(Toml::file("oracle.toml"));
        }
        fig.merge(Env::prefixed("ORACLE__").split("__")).extract()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// HTTP+WS listen address for the pantheon-facing API.
    pub http_addr: String,
    /// Unix socket path for CLI ↔ daemon communication.
    pub unix_socket: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_addr: "127.0.0.1:7345".to_string(),
            unix_socket: PathBuf::from("/var/run/oracle/oracle.sock"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Directory holding the SQLite database and any persistent artefacts.
    pub state_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            state_dir: PathBuf::from("/var/lib/oracle"),
        }
    }
}

impl StorageConfig {
    pub fn db_path(&self) -> PathBuf {
        self.state_dir.join("oracle.db")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LegionLinkConfig {
    /// Which transport to use: `"serial"` (production) or `"tcp"` (SITL/dev).
    pub kind: TransportKind,
    /// Serial port path; only meaningful when `kind = "serial"`.
    pub serial_path: PathBuf,
    pub serial_baud: u32,
    /// TCP listen address; only meaningful when `kind = "tcp"`.
    pub tcp_listen: String,

    /// Drone IDs allowed to connect via the Hello exchange.
    pub allowed_drones: Vec<String>,
    /// Shared bearer token for v1 auth (legions present this in Hello).
    pub shared_token: String,
    pub heartbeat_to_legion_hz: u32,
    pub legion_heartbeat_timeout_ms: u32,
    /// How long an operator-required gate waits before timing out the plan.
    pub operator_gate_timeout_s: u32,
}

impl Default for LegionLinkConfig {
    fn default() -> Self {
        Self {
            kind: TransportKind::Tcp,
            serial_path: PathBuf::from("/dev/ttyUSB0"),
            serial_baud: 57600,
            tcp_listen: "0.0.0.0:7346".to_string(),
            allowed_drones: vec!["drone-01".to_string()],
            shared_token: "dev-token".to_string(),
            heartbeat_to_legion_hz: 2,
            legion_heartbeat_timeout_ms: 3000,
            operator_gate_timeout_s: 600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Serial,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RtkConfig {
    pub source: String,
    pub broadcast_hz: u32,
}

impl Default for RtkConfig {
    fn default() -> Self {
        Self {
            source: "serial:/dev/ttyUSB-rtk:115200".to_string(),
            broadcast_hz: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SlicerConfig {
    /// Spray nozzle width in metres.
    pub spray_width_m: f32,
    /// Overlap between adjacent passes (0.0–1.0).
    pub overlap_pct: f32,
    /// Minimum 3D separation required between any two simultaneous waypoints
    /// at lane assignment time.
    pub min_horizontal_separation_m: f32,
    /// Battery capacity safety margin — drones never plan to discharge below
    /// this percentage.
    pub battery_safety_margin_pct: f32,
    /// Paint capacity safety margin.
    pub paint_safety_margin_pct: f32,
    /// Standoff distance from the surface during a spray pass.
    pub standoff_m: f32,
    /// Truck origin in WGS84. ENU vertex coordinates in the intent are
    /// converted to lat/lon around this origin via a flat-Earth approximation
    /// (good enough for distances under ~10 km).
    pub origin_lat_deg: f64,
    pub origin_lon_deg: f64,
    pub origin_alt_m: f32,
    /// Per-region planarity tolerance: maximum angle (degrees) between the
    /// region's average normal and any individual face normal.
    pub planarity_tol_deg: f32,
    /// Default ferry / takeoff / landing speed in m/s.
    pub ferry_speed_m_s: f32,
    /// Default spray-pass speed in m/s.
    pub spray_speed_m_s: f32,
    /// Default takeoff altitude AGL.
    pub takeoff_alt_m: f32,
}

impl Default for SlicerConfig {
    fn default() -> Self {
        Self {
            spray_width_m: 0.30,
            overlap_pct: 0.20,
            min_horizontal_separation_m: 3.0,
            battery_safety_margin_pct: 25.0,
            paint_safety_margin_pct: 15.0,
            standoff_m: 0.6,
            origin_lat_deg: 50.0,
            origin_lon_deg: 14.0,
            origin_alt_m: 200.0,
            planarity_tol_deg: 15.0,
            ferry_speed_m_s: 3.0,
            spray_speed_m_s: 0.5,
            takeoff_alt_m: 5.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    pub fleet_monitor_hz: u32,
    pub min_safe_distance_m: f32,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            fleet_monitor_hz: 5,
            min_safe_distance_m: 3.0,
        }
    }
}
