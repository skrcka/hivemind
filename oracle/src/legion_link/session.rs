//! Per-drone session task. Owns one `Transport` and shuttles frames between
//! the per-drone command mailbox and the shared event broadcast.

use std::time::{SystemTime, UNIX_EPOCH};

use hivemind_protocol::{Envelope, LegionToOracle, OracleToLegion, TcpTransport, Transport};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use super::LegionEvent;

/// Run a per-drone session against an already-handshaken `TcpTransport`.
/// Loops until the transport closes or the command mailbox is dropped.
///
/// The session is bidirectional: outbound commands flow from `commands` to
/// the transport, inbound frames flow from the transport to `events`.
pub async fn run_tcp(
    drone_id: String,
    mut transport: TcpTransport<OracleToLegion, LegionToOracle>,
    mut commands: mpsc::Receiver<OracleToLegion>,
    events: broadcast::Sender<LegionEvent>,
) {
    debug!(drone_id = %drone_id, "session started");
    loop {
        tokio::select! {
            outbound = commands.recv() => {
                if let Some(msg) = outbound {
                    let env = Envelope::new(&drone_id, now_ms(), msg);
                    if let Err(e) = transport.send(&env).await {
                        warn!(drone_id = %drone_id, error = ?e, "session write failed; closing");
                        return;
                    }
                } else {
                    debug!(drone_id = %drone_id, "command mailbox closed; ending session");
                    return;
                }
            }
            inbound = transport.recv() => {
                match inbound {
                    Ok(env) => {
                        let _ = events.send(LegionEvent {
                            drone_id: env.drone_id.clone(),
                            msg: env.msg,
                        });
                    }
                    Err(e) => {
                        warn!(drone_id = %drone_id, error = ?e, "session read failed; closing");
                        return;
                    }
                }
            }
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
