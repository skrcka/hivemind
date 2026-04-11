//! Listener task. Accepts incoming TCP connections from legions, performs
//! the Hello handshake, registers the resulting session in the link's drone
//! map, and spawns a per-drone task.

use std::collections::HashMap;
use std::sync::Arc;

use hivemind_protocol::{
    Envelope, LegionToOracle, OracleToLegion, TcpTransport, Transport, PROTOCOL_VERSION,
};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, info, warn};

use super::session::{now_ms, run_tcp};
use super::{LegionEvent, Link, SessionHandle};

const COMMAND_MAILBOX_DEPTH: usize = 64;
const EVENT_BROADCAST_CAPACITY: usize = 256;

/// Configuration that the listener honours during the Hello handshake.
#[derive(Debug, Clone)]
pub struct ListenerConfig {
    /// Drone IDs that may connect; everyone else is rejected.
    pub allowed_drones: Vec<String>,
    /// Shared bearer token (placeholder for v1; not yet enforced).
    pub shared_token: String,
    /// Oracle's version, sent in the server-side Hello.
    pub oracle_version: String,
}

/// Start a TCP listener bound to `addr` and return a `Link` handle plus the
/// listener task. The task accepts connections in the background and
/// terminates only on a fatal error.
pub async fn start_tcp(addr: &str, config: ListenerConfig) -> Result<Link, std::io::Error> {
    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "legion link TCP listener bound");
    start_with_listener(listener, config).await
}

/// Start a TCP listener using a pre-bound `TcpListener` (used by tests so
/// they can pick a random port and read it back).
pub async fn start_with_listener(
    listener: TcpListener,
    config: ListenerConfig,
) -> Result<Link, std::io::Error> {
    let sessions: Arc<Mutex<HashMap<String, SessionHandle>>> = Arc::new(Mutex::new(HashMap::new()));
    let (events_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);

    let link = Link {
        sessions: Arc::clone(&sessions),
        events_tx: events_tx.clone(),
    };

    let listener_link = link.clone();
    let listener_config = config;
    tokio::spawn(async move {
        accept_loop(listener, listener_link, listener_config).await;
    });

    Ok(link)
}

async fn accept_loop(listener: TcpListener, link: Link, config: ListenerConfig) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                warn!(error = ?e, "accept() failed; listener exiting");
                return;
            }
        };
        debug!(peer = %peer, "accepted legion connection");

        let link = link.clone();
        let config = config.clone();
        tokio::spawn(async move {
            handle_connection(stream, link, config).await;
        });
    }
}

async fn handle_connection(stream: tokio::net::TcpStream, link: Link, config: ListenerConfig) {
    let mut transport: TcpTransport<OracleToLegion, LegionToOracle> = TcpTransport::new(stream);

    // Wait for Hello from legion.
    let hello_env = match transport.recv().await {
        Ok(env) => env,
        Err(e) => {
            warn!(error = ?e, "transport closed before Hello");
            return;
        }
    };

    if !hello_env.version_matches() {
        warn!(version = hello_env.v, "version mismatch on legion Hello; closing");
        let _ = transport
            .send(&Envelope::new(
                &hello_env.drone_id,
                now_ms(),
                OracleToLegion::ReturnToBase {
                    reason: format!(
                        "version mismatch: oracle expects {PROTOCOL_VERSION}, got {}",
                        hello_env.v
                    ),
                },
            ))
            .await;
        return;
    }

    let drone_id = hello_env.drone_id.clone();
    let (legion_version, _capabilities, _in_progress) = match &hello_env.msg {
        LegionToOracle::Hello {
            legion_version,
            capabilities,
            in_progress_sortie,
            ..
        } => (
            Some(legion_version.clone()),
            capabilities.clone(),
            in_progress_sortie.clone(),
        ),
        other => {
            warn!(drone_id = %drone_id, msg = ?other, "first frame was not Hello; closing");
            return;
        }
    };

    if !config.allowed_drones.contains(&drone_id) {
        warn!(drone_id = %drone_id, "drone not in allowlist; rejecting");
        return;
    }

    info!(drone_id = %drone_id, version = ?legion_version, "legion connected");

    // Send our Hello back.
    let server_hello = Envelope::new(
        &drone_id,
        now_ms(),
        OracleToLegion::Hello {
            oracle_version: config.oracle_version.clone(),
            server_time_ms: now_ms(),
        },
    );
    if let Err(e) = transport.send(&server_hello).await {
        warn!(drone_id = %drone_id, error = ?e, "failed to send oracle Hello");
        return;
    }

    // Forward the legion Hello onto the broadcast as the first event so
    // subscribers know the drone has joined.
    let _ = link.events_tx.send(LegionEvent {
        drone_id: drone_id.clone(),
        msg: hello_env.msg,
    });

    // Register the session and spawn the per-drone loop.
    let (cmd_tx, cmd_rx) = mpsc::channel::<OracleToLegion>(COMMAND_MAILBOX_DEPTH);

    {
        let mut sessions = link.sessions.lock().await;
        sessions.insert(drone_id.clone(), SessionHandle { sender: cmd_tx });
    }

    let drone_id_for_session = drone_id.clone();
    let events_tx = link.events_tx.clone();
    let sessions_arc = Arc::clone(&link.sessions);
    tokio::spawn(async move {
        run_tcp(drone_id_for_session.clone(), transport, cmd_rx, events_tx).await;
        // Session terminated — remove from the map.
        let mut sessions = sessions_arc.lock().await;
        sessions.remove(&drone_id_for_session);
        info!(drone_id = %drone_id_for_session, "legion disconnected");
    });
}
