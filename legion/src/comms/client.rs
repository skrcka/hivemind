//! The `CommsClient` task. Owns the concrete `hivemind_protocol::
//! Transport`, reads inbound frames, writes outbound frames, and
//! demultiplexes inbound messages so the executor sees only the
//! variants it cares about.
//!
//! On transport errors the client attempts an exponential-backoff
//! reconnect for tcp, or reopens the serial port. The `connected`
//! `AtomicBool` is updated at every transition.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hivemind_protocol::{
    DroneId, Envelope, LegionToOracle, OracleToLegion, TcpTransport, Transport,
};
use legion_core::traits::link::ExecutorEvent;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::config::TransportConfig;
use crate::LegionError;

/// Non-executor inbound messages the comms client fans out to the
/// runtime. Heartbeats aren't included — they just feed the watchdog.
#[derive(Debug, Clone)]
pub enum CommsInbound {
    Hello {
        oracle_version: String,
        server_time_ms: u64,
    },
    UploadSortie {
        sortie: hivemind_protocol::Sortie,
    },
    RtkCorrection {
        payload: Vec<u8>,
    },
}

/// High-level commands sent *to* the comms client task from elsewhere
/// in the runtime (e.g. "send this telemetry frame").
#[derive(Debug)]
pub enum CommsCommand {
    Send(LegionToOracle),
}

/// Handle to the running `CommsClient` task.
pub struct CommsHandle {
    pub command_tx: mpsc::UnboundedSender<CommsCommand>,
    pub executor_events_rx: Option<mpsc::Receiver<ExecutorEvent>>,
    pub inbound_rx: Option<mpsc::UnboundedReceiver<CommsInbound>>,
    pub connected: Arc<AtomicBool>,
    pub last_contact_ms: Arc<std::sync::atomic::AtomicU64>,
    pub conn_changes: Arc<Notify>,
    pub task: JoinHandle<Result<(), LegionError>>,
}

/// Spawn the comms client task. Returns a handle that owns the
/// outbound channel sender, the inbound executor-event receiver, and
/// the background task's `JoinHandle`.
pub fn spawn_comms_client(
    drone_id: DroneId,
    transport_cfg: TransportConfig,
    clock: Arc<crate::TokioClock>,
) -> CommsHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel::<CommsCommand>();
    let (executor_events_tx, executor_events_rx) = mpsc::channel::<ExecutorEvent>(32);
    let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<CommsInbound>();
    let connected = Arc::new(AtomicBool::new(false));
    let last_contact_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let conn_changes = Arc::new(Notify::new());

    let task = tokio::spawn(run_comms_loop(
        drone_id,
        transport_cfg,
        clock,
        command_rx,
        executor_events_tx,
        inbound_tx,
        connected.clone(),
        last_contact_ms.clone(),
        conn_changes.clone(),
    ));

    CommsHandle {
        command_tx,
        executor_events_rx: Some(executor_events_rx),
        inbound_rx: Some(inbound_rx),
        connected,
        last_contact_ms,
        conn_changes,
        task,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_comms_loop(
    drone_id: DroneId,
    transport_cfg: TransportConfig,
    clock: Arc<crate::TokioClock>,
    mut command_rx: mpsc::UnboundedReceiver<CommsCommand>,
    executor_events_tx: mpsc::Sender<ExecutorEvent>,
    inbound_tx: mpsc::UnboundedSender<CommsInbound>,
    connected: Arc<AtomicBool>,
    last_contact_ms: Arc<std::sync::atomic::AtomicU64>,
    conn_changes: Arc<Notify>,
) -> Result<(), LegionError> {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    loop {
        match open_transport(&transport_cfg).await {
            Ok(TransportHandle::Tcp(mut t)) => {
                connected.store(true, Ordering::Release);
                conn_changes.notify_waiters();
                tracing::info!(?transport_cfg, "comms: transport opened");
                backoff = Duration::from_secs(1);

                let result = drive_transport(
                    &drone_id,
                    &clock,
                    &mut t,
                    &mut command_rx,
                    &executor_events_tx,
                    &inbound_tx,
                    &last_contact_ms,
                )
                .await;

                connected.store(false, Ordering::Release);
                conn_changes.notify_waiters();

                match result {
                    Ok(()) => {
                        tracing::info!("comms: transport closed cleanly, reconnecting");
                    }
                    Err(e) => {
                        tracing::warn!(%e, "comms: transport error, reconnecting");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(%e, ?backoff, "comms: failed to open transport, backing off");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}

enum TransportHandle {
    Tcp(TcpTransport<LegionToOracle, OracleToLegion>),
}

async fn open_transport(cfg: &TransportConfig) -> Result<TransportHandle, LegionError> {
    match cfg {
        TransportConfig::Tcp { addr } => {
            let stream = TcpStream::connect(addr).await.map_err(|e| {
                LegionError::Transport(format!("tcp connect {addr} failed: {e}"))
            })?;
            Ok(TransportHandle::Tcp(TcpTransport::new(stream)))
        }
        TransportConfig::Serial { path, baud } => {
            // Serial transport uses a different generic type, so we'd
            // return a different variant. For v1 we default to TCP in
            // dev; serial is opened in a separate code path not
            // implemented in this stub. Return an error with a clear
            // message so the operator knows.
            Err(LegionError::Transport(format!(
                "serial transport {path}@{baud} not wired in the v1 stub; use transport.kind = \"tcp\" for dev"
            )))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive_transport(
    drone_id: &str,
    clock: &crate::TokioClock,
    transport: &mut TcpTransport<LegionToOracle, OracleToLegion>,
    command_rx: &mut mpsc::UnboundedReceiver<CommsCommand>,
    executor_events_tx: &mpsc::Sender<ExecutorEvent>,
    inbound_tx: &mpsc::UnboundedSender<CommsInbound>,
    last_contact_ms: &Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), LegionError> {
    use legion_core::Clock;
    loop {
        tokio::select! {
            // Outbound: runtime asked us to send a frame.
            cmd = command_rx.recv() => {
                match cmd {
                    Some(CommsCommand::Send(msg)) => {
                        let env = Envelope::new(drone_id.to_string(), clock.now_ms(), msg);
                        transport
                            .send(&env)
                            .await
                            .map_err(|e| LegionError::Transport(format!("{e:?}")))?;
                    }
                    None => return Ok(()),
                }
            }

            // Inbound: transport produced a frame.
            frame = transport.recv() => {
                let env = frame.map_err(|e| LegionError::Transport(format!("{e:?}")))?;
                last_contact_ms.store(clock.now_ms(), std::sync::atomic::Ordering::Release);
                if let Err(e) = dispatch_inbound(env.msg, executor_events_tx, inbound_tx).await {
                    tracing::warn!(%e, "comms: dispatch_inbound error");
                }
            }
        }
    }
}

async fn dispatch_inbound(
    msg: OracleToLegion,
    executor_events_tx: &mpsc::Sender<ExecutorEvent>,
    inbound_tx: &mpsc::UnboundedSender<CommsInbound>,
) -> Result<(), LegionError> {
    match msg {
        // Executor-relevant control frames. Demux into the executor
        // mpsc.
        OracleToLegion::Proceed {
            sortie_id,
            expected_step_index,
        } => {
            let _ = executor_events_tx
                .send(ExecutorEvent::Proceed {
                    sortie_id,
                    expected_step_index,
                })
                .await;
        }
        OracleToLegion::HoldStep { sortie_id, reason } => {
            let _ = executor_events_tx
                .send(ExecutorEvent::HoldStep { sortie_id, reason })
                .await;
        }
        OracleToLegion::AbortSortie { sortie_id, reason } => {
            let _ = executor_events_tx
                .send(ExecutorEvent::AbortSortie { sortie_id, reason })
                .await;
        }
        OracleToLegion::ReturnToBase { reason } => {
            let _ = executor_events_tx
                .send(ExecutorEvent::ReturnToBase { reason })
                .await;
        }
        OracleToLegion::CancelSortie { sortie_id } => {
            let _ = executor_events_tx
                .send(ExecutorEvent::CancelSortie { sortie_id })
                .await;
        }

        // Non-executor frames. Fan out to the runtime or swallow.
        OracleToLegion::Hello {
            oracle_version,
            server_time_ms,
        } => {
            let _ = inbound_tx.send(CommsInbound::Hello {
                oracle_version,
                server_time_ms,
            });
        }
        OracleToLegion::Heartbeat => {
            // The heartbeat just feeds the watchdog — the timestamp was
            // already stored before `dispatch_inbound` was called.
        }
        OracleToLegion::UploadSortie { sortie } => {
            let _ = inbound_tx.send(CommsInbound::UploadSortie { sortie });
        }
        OracleToLegion::RtkCorrection { payload } => {
            let _ = inbound_tx.send(CommsInbound::RtkCorrection { payload });
        }
    }
    Ok(())
}
