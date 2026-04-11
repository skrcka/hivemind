//! Executor-facing link abstraction.
//!
//! The Pi binary owns the concrete `hivemind_protocol::Transport` in its
//! comms client task; that task dispatches incoming frames. The executor
//! never touches the transport directly — it talks to the comms task
//! through this trait, which:
//!
//! - accepts outbound `LegionToOracle` messages (typed), and
//! - returns executor-relevant inbound events (`Proceed`, `HoldStep`,
//!   `AbortSortie`, `ReturnToBase`, `CancelSortie`) with a timeout.
//!
//! All other inbound message kinds (`Heartbeat`, `Hello`, `UploadSortie`,
//! `RtkCorrection`) are handled *inside* the comms task (they feed the
//! watchdog, arm the MAVLink driver, load a new sortie, etc.) and never
//! reach the executor.

use alloc::string::String;
use core::future::Future;
use core::time::Duration;

use crate::error::LinkError;
use hivemind_protocol::{LegionToOracle, SortieId};

/// A subset of `OracleToLegion` — only the variants that the executor
/// acts on. The Pi binary's comms client demuxes incoming frames into
/// this enum before pushing them to the executor.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutorEvent {
    Proceed {
        sortie_id: SortieId,
        expected_step_index: u32,
    },
    HoldStep {
        sortie_id: SortieId,
        reason: String,
    },
    AbortSortie {
        sortie_id: SortieId,
        reason: String,
    },
    /// Hard RTL — overrides whatever step is in flight.
    ReturnToBase {
        reason: String,
    },
    /// Drop a sortie that hasn't started executing yet.
    CancelSortie {
        sortie_id: SortieId,
    },
}

/// The executor-facing side of the oracle link. Implementors adapt the
/// binary's concrete `hivemind_protocol::Transport` into this.
pub trait Link: Send {
    /// Send an envelope to oracle. Blocks until the frame is queued into
    /// the transport; delivery is best-effort under the transport's own
    /// semantics (reliable on TCP, unreliable on serial).
    fn send(
        &mut self,
        msg: LegionToOracle,
    ) -> impl Future<Output = Result<(), LinkError>> + Send;

    /// Receive the next executor-relevant event, or `Ok(None)` if
    /// `timeout` elapsed without one arriving. Non-executor frames
    /// (heartbeats, RTK corrections, etc.) do *not* count — they're
    /// handled in the comms task and the executor doesn't see them.
    ///
    /// A clean transport close returns `Err(LinkError::NotConnected)`;
    /// the executor interprets that the same way as a timeout for
    /// radio-loss-policy purposes.
    fn recv_executor_event(
        &mut self,
        timeout: Duration,
    ) -> impl Future<Output = Result<Option<ExecutorEvent>, LinkError>> + Send;

    /// Whether the underlying transport is connected. `false` during a
    /// reconnect-with-backoff window.
    fn is_connected(&self) -> bool;
}
