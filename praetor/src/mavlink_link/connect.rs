//! Opening a MAVLink connection — serial (SiK radio) or TCP (PX4 SITL).
//!
//! The `mavlink` crate's `connect()` call handles the URI parsing for us
//! (`serial:/dev/ttyUSB1:57600` or `tcp:127.0.0.1:5760`), so this module is
//! little more than a typed wrapper + the "wait for the first HEARTBEAT"
//! handshake that confirms the drone is actually on the other end.

use std::sync::Arc;
use std::time::Duration;

use mavlink::common::MavMessage;
use mavlink::{MavConnection, MavlinkVersion};
use tracing::{info, warn};

use crate::error::{PraetorError, Result};

/// Type alias for the connection trait object — it's long enough that
/// writing it out everywhere gets ugly.
pub type MavConn = Arc<Box<dyn MavConnection<MavMessage> + Send + Sync>>;

/// Open a connection to the MAVLink endpoint and wait for the first
/// HEARTBEAT to confirm the other side is alive.
///
/// `address` is anything `mavlink::connect` accepts:
///
/// - `serial:/dev/ttyUSB1:57600`
/// - `tcp:127.0.0.1:5760` (PX4 SITL)
/// - `udpin:0.0.0.0:14540` / `udpout:127.0.0.1:14540` (for MAVSDK/QGC setups)
///
/// Blocks for up to `timeout` waiting for the first HEARTBEAT. Returns the
/// wrapped connection and the system ID the HEARTBEAT came from.
pub fn open(address: &str, timeout: Duration) -> Result<(MavConn, u8)> {
    info!(%address, "opening MAVLink connection");

    let mut conn = mavlink::connect::<MavMessage>(address)
        .map_err(|e| PraetorError::Mavlink(format!("connect {address}: {e}")))?;

    // Use MAVLink 2 — PX4 speaks both but we want the v2-only features
    // (signed frames, larger fields). Matches legion's setup.
    conn.set_protocol_version(MavlinkVersion::V2);

    // Wait for the first HEARTBEAT. The `recv()` call is blocking; we run
    // it in a small loop so a transient decode error on another message
    // type doesn't kill the handshake.
    let start = std::time::Instant::now();
    let system_id = loop {
        if start.elapsed() > timeout {
            return Err(PraetorError::Mavlink(format!(
                "no HEARTBEAT from {address} within {timeout:?}"
            )));
        }

        match conn.recv() {
            Ok((header, MavMessage::HEARTBEAT(_))) => {
                info!(system_id = header.system_id, "received first HEARTBEAT");
                break header.system_id;
            }
            Ok(_) => {
                // Any other message means the link is alive but not yet
                // in sync on our HEARTBEAT wait — keep waiting.
            }
            Err(e) => {
                warn!(error = ?e, "transient recv error while waiting for HEARTBEAT");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    Ok((Arc::new(conn), system_id))
}
