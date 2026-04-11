//! Shared application state — the single source of truth that every
//! background task writes to and every Tauri command reads from.
//!
//! Concurrency model:
//!
//!   - `TelemetrySnapshot` is published via `tokio::sync::watch` so any task
//!     (the Tauri event emitter, the safety watchdog, the interlock checker)
//!     can `.borrow()` the latest value without contention.
//!   - `ArmingState`, `LinkStatus`, and `ControllerStatus` are published via
//!     separate `watch` channels — they change rarely but every subscriber
//!     needs the latest value without polling.
//!   - `MavlinkLink` is wrapped in `RwLock<Option<…>>` so the connect /
//!     disconnect Tauri commands can swap the connection without cloning it
//!     through every task.

use std::sync::Arc;

use tokio::sync::{watch, RwLock};

use crate::config::Config;
use crate::gamepad::intent::ControlIntent;
use crate::mavlink_link::snapshot::TelemetrySnapshot;
use crate::mavlink_link::MavlinkLink;

/// The root handle every task holds a clone of.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,

    /// The live MAVLink connection. `None` while disconnected.
    pub link: Arc<RwLock<Option<MavlinkLink>>>,

    pub telemetry: watch::Receiver<TelemetrySnapshot>,
    pub telemetry_tx: watch::Sender<TelemetrySnapshot>,

    pub link_status: watch::Receiver<LinkStatus>,
    pub link_status_tx: watch::Sender<LinkStatus>,

    pub arming: watch::Receiver<ArmingState>,
    pub arming_tx: watch::Sender<ArmingState>,

    pub controller: watch::Receiver<ControllerStatus>,
    pub controller_tx: watch::Sender<ControllerStatus>,

    pub intent: watch::Receiver<ControlIntent>,
    pub intent_tx: watch::Sender<ControlIntent>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (telemetry_tx, telemetry) = watch::channel(TelemetrySnapshot::default());
        let (link_status_tx, link_status) = watch::channel(LinkStatus::Disconnected);
        let (arming_tx, arming) = watch::channel(ArmingState::disarmed());
        let (controller_tx, controller) = watch::channel(ControllerStatus::Disconnected);
        let (intent_tx, intent) = watch::channel(ControlIntent::neutral());

        Self {
            config: Arc::new(config),
            link: Arc::new(RwLock::new(None)),
            telemetry,
            telemetry_tx,
            link_status,
            link_status_tx,
            arming,
            arming_tx,
            controller,
            controller_tx,
            intent,
            intent_tx,
        }
    }
}

/// Published over `link_status` — drives the UI connection indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Disconnected,
    Connecting,
    Connected,
    /// HEARTBEAT has been absent for longer than the configured threshold.
    Stale,
    /// Fatal connection error — operator must reconnect manually.
    Failed,
}

/// Whether praetor *believes* the drone is armed, based on the `HEARTBEAT`
/// base mode flags plus the local hold-to-arm state machine. The ground
/// truth lives in PX4; this is the cached view for the UI and the pump
/// interlock.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ArmingState {
    pub kind: ArmingKind,
    /// 0..1 — only meaningful while `kind` is `Arming` or `Disarming`.
    pub progress: f32,
}

impl ArmingState {
    pub const fn disarmed() -> Self {
        Self {
            kind: ArmingKind::Disarmed,
            progress: 0.0,
        }
    }
    pub const fn armed() -> Self {
        Self {
            kind: ArmingKind::Armed,
            progress: 1.0,
        }
    }
    pub const fn arming(progress: f32) -> Self {
        Self {
            kind: ArmingKind::Arming,
            progress,
        }
    }
    pub const fn disarming(progress: f32) -> Self {
        Self {
            kind: ArmingKind::Disarming,
            progress,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmingKind {
    Disarmed,
    Arming,
    Armed,
    Disarming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerStatus {
    Disconnected,
    Connected,
}
