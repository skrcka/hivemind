//! `legion_core::Link` impl backed by tokio mpsc channels that the
//! `CommsClient` task fans data in and out of.
//!
//! The executor never touches the wire directly — it talks to this
//! `ExecutorLink`, which in turn forwards to the comms task.

use std::sync::Arc;

use legion_core::error::LinkError;
use legion_core::traits::link::ExecutorEvent;
use legion_core::{LegionToOracle, Link};
use tokio::sync::mpsc;
use tokio::sync::Notify;

/// Handle the executor uses to talk to the oracle link.
///
/// Outbound messages are pushed onto an unbounded channel; the
/// `CommsClient` task reads from the channel and writes them to the
/// transport. Inbound executor events (`Proceed`, `HoldStep`, …) are
/// received on a bounded mpsc the comms task pushes into after
/// demultiplexing.
pub struct ExecutorLink {
    outbound: mpsc::UnboundedSender<LegionToOracle>,
    inbound: mpsc::Receiver<ExecutorEvent>,
    connected: Arc<std::sync::atomic::AtomicBool>,
    /// Fired by the comms client when the transport either connects or
    /// disconnects — surfaces as `is_connected()` transitions.
    #[allow(dead_code)]
    conn_changes: Arc<Notify>,
}

impl ExecutorLink {
    pub fn new(
        outbound: mpsc::UnboundedSender<LegionToOracle>,
        inbound: mpsc::Receiver<ExecutorEvent>,
        connected: Arc<std::sync::atomic::AtomicBool>,
        conn_changes: Arc<Notify>,
    ) -> Self {
        Self {
            outbound,
            inbound,
            connected,
            conn_changes,
        }
    }
}

impl Link for ExecutorLink {
    async fn send(&mut self, msg: LegionToOracle) -> Result<(), LinkError> {
        self.outbound.send(msg).map_err(|e| LinkError::Transport {
            detail: format!("outbound channel closed: {e}"),
        })
    }

    async fn recv_executor_event(
        &mut self,
        timeout: core::time::Duration,
    ) -> Result<Option<ExecutorEvent>, LinkError> {
        match tokio::time::timeout(timeout, self.inbound.recv()).await {
            Ok(Some(ev)) => Ok(Some(ev)),
            Ok(None) => Err(LinkError::Transport {
                detail: "inbound channel closed".into(),
            }),
            Err(_) => {
                // Timeout. Report `NotConnected` if the transport is
                // down, so the executor treats it the same way as a
                // hard disconnect; otherwise it's a plain silent
                // timeout (`Ok(None)`) and the radio-loss policy
                // kicks in.
                if !self
                    .connected
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    Err(LinkError::NotConnected)
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected
            .load(std::sync::atomic::Ordering::Acquire)
    }
}
