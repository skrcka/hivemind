//! MAVLink link — the connection to the drone's Pixhawk.
//!
//! The public type `MavlinkLink` is what `AppState::link` holds. It bundles
//!
//!   - an `Arc<Box<dyn MavConnection>>` shared between the send path and
//!     the dedicated receiver thread,
//!   - the system ID the handshake saw (for outbound command targeting),
//!   - an `AtomicBool` the parent uses to stop the receiver when the
//!     operator hits "Disconnect".
//!
//! It is intentionally *not* a tokio actor. Commands go out via
//! [`send::send`]; telemetry comes in via `recv::spawn` which writes
//! directly to the `AppState::telemetry_tx` watch channel.

pub mod connect;
pub mod recv;
pub mod send;
pub mod snapshot;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tracing::{info, warn};

use crate::error::Result;
use crate::mavlink_link::connect::MavConn;
use crate::state::{AppState, LinkStatus};

pub struct MavlinkLink {
    pub conn: MavConn,
    pub target_system: u8,
    pub target_component: u8,
    running: Arc<AtomicBool>,
    recv_handle: Option<JoinHandle<()>>,
}

impl MavlinkLink {
    /// Open a connection to `address`, wait for the first HEARTBEAT, spawn
    /// the receiver thread, and return a live handle.
    pub async fn connect(state: AppState, address: &str) -> Result<Self> {
        let target_component = state.config.link.target_component_id;
        let _ = state.link_status_tx.send(LinkStatus::Connecting);

        // The open() call is blocking; run it on the blocking pool so
        // Tauri's async runtime doesn't stall.
        let addr_owned = address.to_owned();
        let (conn, target_system) =
            tokio::task::spawn_blocking(move || connect::open(&addr_owned, Duration::from_secs(5)))
                .await??;

        let running = Arc::new(AtomicBool::new(true));
        let recv_handle = Some(recv::spawn(
            Arc::clone(&conn),
            state.clone(),
            Arc::clone(&running),
        ));

        let _ = state.link_status_tx.send(LinkStatus::Connected);
        info!(target_system, "mavlink link up");

        Ok(Self {
            conn,
            target_system,
            target_component,
            running,
            recv_handle,
        })
    }

    pub fn conn(&self) -> MavConn {
        Arc::clone(&self.conn)
    }
}

impl Drop for MavlinkLink {
    fn drop(&mut self) {
        // Signal the receiver thread to exit its loop. The thread will
        // observe this on its next recv() return (which may take up to a
        // read-timeout's worth of time on serial, which is fine).
        self.running.store(false, Ordering::Relaxed);

        // We don't join the thread in Drop — it'd block the tokio runtime
        // thread that's running Drop. Detach it. The thread will exit on
        // its own once the underlying transport closes.
        if let Some(_h) = self.recv_handle.take() {
            // Intentional: detached.
        }
        warn!("mavlink link dropped");
    }
}
