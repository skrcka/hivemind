//! The oracle ↔ legion message catalogue.
//!
//! Two top-level enums:
//!
//! - [`OracleToLegion`] — commands oracle sends to legion (sortie upload,
//!   step gating, hold/abort/RTL, RTK corrections).
//! - [`LegionToOracle`] — status legion sends to oracle (telemetry, step
//!   completions, safety events).
//!
//! Both are wrapped in [`Envelope`] on the wire.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::sortie::{DroneId, InProgressSortie, Sortie, SortieId};
use crate::telemetry::{Position, SafetyEventKind, Telemetry};

/// Protocol version. Carried in the [`OracleToLegion::Hello`] /
/// [`LegionToOracle::Hello`] exchange; mismatched versions close the
/// connection with an [`LegionToOracle::Error`] frame.
pub const PROTOCOL_VERSION: u8 = 1;

/// Outer wire envelope. Every frame on the wire is `Envelope<OracleToLegion>`
/// or `Envelope<LegionToOracle>`.
///
/// `drone_id` is the routing key on a shared serial radio channel where
/// multiple drones coexist — every air-side radio hears every ground-side
/// frame, and legions ignore frames not addressed to themselves.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Envelope<T> {
    /// Protocol version. Always equals [`PROTOCOL_VERSION`] for new envelopes.
    pub v: u8,
    /// Sender's monotonic milliseconds, used for jitter analysis.
    pub ts_ms: u64,
    pub drone_id: DroneId,
    pub msg: T,
}

impl<T> Envelope<T> {
    /// Construct a new envelope at the current protocol version.
    pub fn new(drone_id: impl Into<DroneId>, ts_ms: u64, msg: T) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ts_ms,
            drone_id: drone_id.into(),
            msg,
        }
    }

    /// Whether the envelope's protocol version matches the build's
    /// [`PROTOCOL_VERSION`]. Used by both sides to bail on the `Hello` if the
    /// peer is on an incompatible version.
    pub fn version_matches(&self) -> bool {
        self.v == PROTOCOL_VERSION
    }
}

/// Commands from oracle to legion.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OracleToLegion {
    /// First frame after the transport opens. Establishes protocol version
    /// and clock offset.
    Hello {
        oracle_version: String,
        server_time_ms: u64,
    },
    /// 2 Hz keepalive. Resets legion's `oracle_silent` watchdog.
    Heartbeat,
    /// Full sortie upload. Legion validates, persists, then replies
    /// `LegionToOracle::SortieReceived`.
    UploadSortie { sortie: Sortie },
    /// Unblock legion's executor for the next step. The `expected_step_index`
    /// is checked against legion's current step — out-of-order frames are
    /// rejected with an `Error`.
    Proceed {
        sortie_id: SortieId,
        expected_step_index: u32,
    },
    /// Tell legion to hold at the current position before starting the next
    /// step.
    HoldStep {
        sortie_id: SortieId,
        reason: String,
    },
    /// Clean abort: legion stops the executor, RTLs the drone, replies
    /// `LegionToOracle::SortieFailed`.
    AbortSortie {
        sortie_id: SortieId,
        reason: String,
    },
    /// Hard RTL — overrides whatever step is in flight.
    ReturnToBase { reason: String },
    /// Drop a sortie that hasn't started executing yet. Errors if it's
    /// already running.
    CancelSortie { sortie_id: SortieId },
    /// Opaque RTCM3 bytes for RTK injection. Legion writes them to the
    /// Pixhawk via its local `mavlink::rtk` module.
    RtkCorrection { payload: Vec<u8> },
}

/// Status and telemetry from legion to oracle.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum LegionToOracle {
    /// First frame from legion. The `in_progress_sortie` field is non-null if
    /// legion booted with a partially-completed sortie on disk and is
    /// reporting it for the operator to decide what to do.
    Hello {
        drone_id: DroneId,
        legion_version: String,
        capabilities: Vec<String>,
        in_progress_sortie: Option<InProgressSortie>,
    },
    /// 2 Hz keepalive (typically piggybacked on `Telemetry`, but legion can
    /// send a bare heartbeat if it has nothing else to say).
    Heartbeat,
    /// Periodic state snapshot.
    Telemetry(Telemetry),
    /// Validation of an `UploadSortie` passed; sortie is persisted and ready
    /// to execute.
    SortieReceived { sortie_id: SortieId },
    /// Sent after a step handler returns successfully. Legion's executor then
    /// blocks waiting for `Proceed`.
    StepComplete {
        sortie_id: SortieId,
        step_index: u32,
        position: Position,
        battery_pct: f32,
        paint_remaining_ml: f32,
        duration_s: f32,
    },
    /// All steps in the sortie are done.
    SortieComplete { sortie_id: SortieId },
    /// Clean abort or unrecoverable executor error.
    SortieFailed {
        sortie_id: SortieId,
        step_index: u32,
        reason: String,
    },
    /// The local safety loop fired.
    SafetyEvent {
        kind: SafetyEventKind,
        action: String,
        detail: String,
    },
    /// Confirmation that legion is holding (whether from `HoldStep` or from a
    /// radio-loss policy trip).
    Held {
        sortie_id: SortieId,
        step_index: u32,
        reason: String,
    },
    /// Out-of-protocol issues: bad frame, version mismatch, expected_step_index
    /// out of order.
    Error { code: String, message: String },
}
