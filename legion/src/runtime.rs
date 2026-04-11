//! Tokio runtime wiring. Spawns the comms client, the safety loop,
//! the telemetry pumper, and the top-level task that receives
//! `UploadSortie` messages and hands them to the executor.

use std::sync::Arc;
use std::time::Duration;

use hivemind_protocol::DronePhase;
use legion_core::safety::SafetyState;
use legion_core::{executor::Executor, Clock, LegionToOracle, MavlinkBackend};
use tokio::sync::{Mutex, Notify};

use crate::comms::{spawn_comms_client, CommsCommand, CommsInbound, ExecutorLink};
use crate::config::Config;
use crate::mavlink_driver::StubMavlinkDriver;
use crate::payload::MockPayload;
use crate::safety_loop::{self, SafetyLoopHandles};
use crate::shared_state::{self, SharedState};
use crate::store::FileSortieStore;
use crate::{LegionError, TokioClock};

/// Build and run the full legion runtime with the current (v1)
/// backend selection: `MockPayload`, `StubMavlinkDriver`,
/// `FileSortieStore`, `TcpTransport` (or serial).
pub async fn run(config: Config) -> Result<(), LegionError> {
    tracing::info!(drone_id = %config.drone.id, "legion: starting runtime");

    let clock = Arc::new(TokioClock::new());
    let state: SharedState = shared_state::new(config.drone.id.clone());

    // Seed state with a fresh oracle contact timestamp so the safety
    // watchdog doesn't trip before the first frame arrives.
    state.write().await.last_oracle_contact_ms = clock.now_ms();

    let payload = Arc::new(Mutex::new(MockPayload::new()));
    let mavlink = Arc::new(StubMavlinkDriver::new());

    let store = Arc::new(FileSortieStore::new(&config.storage.sortie_dir).map_err(|e| {
        LegionError::Other(format!(
            "sortie store init at {}: {}",
            config.storage.sortie_dir.display(),
            e
        ))
    })?);

    // Check the store for an in-progress sortie to report in Hello.
    let in_progress = match store.find_in_progress() {
        Ok(Some(p)) => Some(hivemind_protocol::InProgressSortie {
            sortie_id: p.sortie_id,
            last_completed_step: p.last_completed_step,
        }),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(%e, "legion: could not scan store for in-progress sortie");
            None
        }
    };

    let mut comms = spawn_comms_client(
        config.drone.id.clone(),
        config.transport.clone(),
        clock.clone(),
    );

    // Initial Hello. This is queued for send as soon as the transport
    // comes up.
    comms
        .command_tx
        .send(CommsCommand::Send(LegionToOracle::Hello {
            drone_id: config.drone.id.clone(),
            legion_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: config.drone.capabilities.clone(),
            in_progress_sortie: in_progress,
        }))
        .map_err(|e| LegionError::Other(format!("comms command channel closed: {e}")))?;

    let (safety_tx, safety_rx) = safety_loop::new_watch();

    // Safety loop task.
    let safety_handles = SafetyLoopHandles {
        state: state.clone(),
        payload: payload.clone(),
        mavlink: mavlink.clone(),
        clock: clock.clone(),
        cfg: config.safety.to_core(),
        safety_tx,
        command_tx: comms.command_tx.clone(),
        last_oracle_contact_ms: comms.last_contact_ms.clone(),
    };
    tokio::spawn(safety_loop::run(safety_handles));

    // Telemetry pumper task: sends a Telemetry frame at 2 Hz.
    let telem_command_tx = comms.command_tx.clone();
    let telem_state = state.clone();
    let telem_clock = clock.clone();
    let telem_mavlink = mavlink.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        loop {
            tick.tick().await;
            let snap = telem_state.read().await;
            let telem = hivemind_protocol::Telemetry {
                ts_ms: telem_clock.now_ms(),
                position: telem_mavlink.position(),
                attitude: snap.attitude,
                battery_pct: telem_mavlink.battery_pct(),
                voltage: snap.voltage,
                paint_remaining_ml: snap.paint_remaining_ml,
                tof_distance_cm: snap.tof_distance_cm,
                gps_fix: snap.gps_fix,
                sortie_id: snap.current_sortie.as_ref().map(|s| s.sortie_id.clone()),
                step_index: snap
                    .current_sortie
                    .as_ref()
                    .map(|_| snap.current_step_index),
                drone_phase: snap.drone_phase,
            };
            let _ = telem_command_tx.send(CommsCommand::Send(LegionToOracle::Telemetry(telem)));
        }
    });

    // Executor task: waits for UploadSortie from the comms inbound
    // channel, runs the sortie under the executor, then loops.
    let executor_events_rx = comms
        .executor_events_rx
        .take()
        .expect("executor events channel");
    let inbound_rx = comms.inbound_rx.take().expect("inbound channel");

    run_executor_loop(
        executor_events_rx,
        inbound_rx,
        state.clone(),
        payload.clone(),
        mavlink.clone(),
        store.clone(),
        clock.clone(),
        comms.command_tx.clone(),
        comms.connected.clone(),
        comms.conn_changes.clone(),
        safety_rx,
    )
    .await?;

    // The comms task is the only long-lived one left — wait for it.
    match comms.task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(LegionError::Other(format!("comms task join: {e}"))),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_executor_loop(
    executor_events_rx: tokio::sync::mpsc::Receiver<legion_core::traits::link::ExecutorEvent>,
    mut inbound_rx: tokio::sync::mpsc::UnboundedReceiver<CommsInbound>,
    state: SharedState,
    payload: Arc<Mutex<MockPayload>>,
    mavlink: Arc<StubMavlinkDriver>,
    store: Arc<FileSortieStore>,
    clock: Arc<TokioClock>,
    command_tx: tokio::sync::mpsc::UnboundedSender<CommsCommand>,
    connected: Arc<std::sync::atomic::AtomicBool>,
    conn_changes: Arc<Notify>,
    mut safety_rx: tokio::sync::watch::Receiver<SafetyState>,
) -> Result<(), LegionError> {
    let outbound_tx = command_tx.clone();
    // Wrap the executor-events receiver behind a new channel so we can
    // feed a custom `ExecutorLink` that sends via `command_tx`.
    let (link_outbound_tx, mut link_outbound_rx) =
        tokio::sync::mpsc::unbounded_channel::<LegionToOracle>();

    // Bridge: drain link_outbound_rx into the existing comms command
    // channel. This lets `ExecutorLink::send` stay a plain unbounded
    // sender without holding a strong ref to the multi-producer
    // `command_tx`.
    tokio::spawn(async move {
        while let Some(msg) = link_outbound_rx.recv().await {
            if outbound_tx.send(CommsCommand::Send(msg)).is_err() {
                break;
            }
        }
    });

    let mut link = ExecutorLink::new(
        link_outbound_tx,
        executor_events_rx,
        connected,
        conn_changes,
    );

    loop {
        tokio::select! {
            biased;

            // Priority 1: safety trip. Abort whatever we're doing and
            // wait for it to clear.
            changed = safety_rx.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let current = safety_rx.borrow().clone();
                if matches!(current, SafetyState::Ok) {
                    continue;
                }
                tracing::warn!(?current, "runtime: safety trip");
                // The safety loop has already commanded the Pixhawk
                // directly. Nothing to do from the executor side other
                // than making sure we don't start a new sortie while
                // the trip is latched.
                continue;
            }

            // Priority 2: new work from oracle.
            msg = inbound_rx.recv() => {
                let Some(msg) = msg else { return Ok(()); };
                match msg {
                    CommsInbound::UploadSortie { sortie } => {
                        if let Err(e) = handle_upload(
                            &sortie,
                            &store,
                            &command_tx,
                        )
                        .await
                        {
                            tracing::warn!(%e, "runtime: upload validation failed");
                            continue;
                        }

                        // Guard against starting a sortie while safety
                        // is tripped.
                        if safety_rx.borrow().is_tripped() {
                            tracing::warn!("runtime: refusing to start sortie while safety tripped");
                            continue;
                        }

                        // Run the sortie. The executor future blocks
                        // here — if safety trips mid-step, the
                        // priority-1 arm above gets the cancel via the
                        // `tokio::select!` and cancels this future.
                        let sortie_id = sortie.sortie_id.clone();
                        let result = run_one_sortie(
                            sortie,
                            &state,
                            &payload,
                            &mavlink,
                            &store,
                            &clock,
                            &mut link,
                            &mut safety_rx,
                        )
                        .await;
                        if let Err(e) = result {
                            tracing::warn!(%sortie_id, %e, "runtime: executor exited with error");
                        }
                    }
                    CommsInbound::Hello { oracle_version, server_time_ms } => {
                        tracing::info!(%oracle_version, server_time_ms, "runtime: oracle Hello");
                    }
                    CommsInbound::RtkCorrection { payload: rtcm } => {
                        let _ = mavlink.inject_rtk(&rtcm).await;
                    }
                }
            }
        }
    }
}

async fn handle_upload(
    sortie: &hivemind_protocol::Sortie,
    store: &FileSortieStore,
    command_tx: &tokio::sync::mpsc::UnboundedSender<CommsCommand>,
) -> Result<(), LegionError> {
    use legion_core::SortieStore;
    store
        .put(sortie)
        .await
        .map_err(|e| LegionError::Other(format!("store put: {e}")))?;
    command_tx
        .send(CommsCommand::Send(LegionToOracle::SortieReceived {
            sortie_id: sortie.sortie_id.clone(),
        }))
        .map_err(|e| LegionError::Other(format!("sortie-received send: {e}")))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_one_sortie(
    sortie: hivemind_protocol::Sortie,
    state: &SharedState,
    payload: &Arc<Mutex<MockPayload>>,
    mavlink: &Arc<StubMavlinkDriver>,
    store: &Arc<FileSortieStore>,
    clock: &Arc<TokioClock>,
    link: &mut ExecutorLink,
    safety_rx: &mut tokio::sync::watch::Receiver<SafetyState>,
) -> Result<(), LegionError> {
    // We need a `&mut LegionState` for the executor. Rather than hold
    // the write lock across the entire sortie (which would starve the
    // safety loop), we clone it in, run, then copy the executor-owned
    // fields back out at the end.
    let mut local_state = state.read().await.clone();
    let mut payload_guard = payload.lock().await;

    let sortie_future = Executor::run_sortie(
        sortie,
        &mut *payload_guard,
        mavlink.as_ref(),
        store.as_ref(),
        clock.as_ref(),
        link,
        &mut local_state,
    );

    let result = tokio::select! {
        biased;
        changed = safety_rx.changed() => {
            let _ = changed;
            let s = safety_rx.borrow().clone();
            if s.is_tripped() {
                // Safety tripped — cancel the executor future by
                // dropping it here.
                Err(LegionError::Executor(format!("safety preemption: {s:?}")))
            } else {
                Ok(())
            }
        }
        r = sortie_future => {
            r.map_err(|e| LegionError::Executor(format!("{e}")))
        }
    };

    // Copy executor-owned fields back into the shared state.
    let mut st = state.write().await;
    st.current_sortie = local_state.current_sortie.clone();
    st.current_step_index = local_state.current_step_index;
    st.last_completed_step = local_state.last_completed_step;
    st.drone_phase = local_state.drone_phase;
    drop(st);
    drop(payload_guard);

    // If we exited with a safety preemption, force the phase to
    // Landing so downstream telemetry reflects it.
    if matches!(result, Err(LegionError::Executor(_))) {
        state.write().await.drone_phase = DronePhase::Landing;
    }

    result
}
