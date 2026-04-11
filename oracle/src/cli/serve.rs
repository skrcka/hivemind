//! `hivemind serve` — boot the oracle daemon.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use tokio::net::TcpListener;
use tracing::info;

use crate::api::AppState;
use crate::apply::supervisor::OperatorSignals;
use crate::config::OracleConfig;
use crate::fleet::{monitor, FleetState};
use crate::legion_link::server::{start_with_listener, ListenerConfig};
use crate::store::Store;

#[derive(Debug, Args)]
pub struct ServeArgs {}

pub async fn run(cfg: OracleConfig, _args: ServeArgs) -> Result<()> {
    info!("starting oracle");

    // Persistence.
    let store = Store::open(&cfg.storage.db_path())
        .await
        .with_context(|| format!("opening store at {}", cfg.storage.db_path().display()))?;
    let store = Arc::new(store);
    info!(db_path = %cfg.storage.db_path().display(), "store ready");

    // Legion link.
    let listener_cfg = ListenerConfig {
        allowed_drones: cfg.legion_link.allowed_drones.clone(),
        shared_token: cfg.legion_link.shared_token.clone(),
        oracle_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let link = match cfg.legion_link.kind {
        crate::config::TransportKind::Tcp => {
            let listener = TcpListener::bind(&cfg.legion_link.tcp_listen)
                .await
                .with_context(|| format!("binding legion link {}", cfg.legion_link.tcp_listen))?;
            info!(addr = %cfg.legion_link.tcp_listen, "legion link TCP listener bound");
            start_with_listener(listener, listener_cfg)
                .await
                .context("starting legion link")?
        }
        crate::config::TransportKind::Serial => {
            anyhow::bail!(
                "serial legion link transport is wired into hivemind-protocol but the listener \
                 entry point is v2; for v1 use kind = \"tcp\""
            );
        }
    };
    let link = Arc::new(link);

    // Fleet state + monitor.
    let fleet = FleetState::new();
    let _monitor = monitor::spawn(fleet.clone(), &cfg.safety);

    // Operator signal bus.
    let operator_signals = OperatorSignals::new();

    // App state.
    let cfg_arc = Arc::new(cfg.clone());
    let state = AppState {
        store: store.clone(),
        link: link.clone(),
        fleet: fleet.clone(),
        config: cfg_arc,
        operator_signals,
    };

    // HTTP+WS API.
    let app = crate::api::router(state);
    let listener = TcpListener::bind(&cfg.server.http_addr)
        .await
        .with_context(|| format!("binding API {}", cfg.server.http_addr))?;
    info!(addr = %cfg.server.http_addr, "API server bound");

    // Spawn a small task that ingests legion telemetry into the FleetState +
    // store. Lives for the duration of the process.
    let telemetry_task = tokio::spawn(telemetry_pump(link.clone(), fleet.clone(), store.clone()));

    axum::serve(listener, app)
        .await
        .context("axum serve loop")?;

    telemetry_task.abort();
    Ok(())
}

/// Background task that subscribes to the legion link's event broadcast and
/// updates fleet state + the drones table whenever telemetry arrives.
async fn telemetry_pump(
    link: Arc<crate::legion_link::Link>,
    fleet: FleetState,
    store: Arc<Store>,
) {
    let mut rx = link.subscribe();
    loop {
        match rx.recv().await {
            Ok(evt) => {
                if let hivemind_protocol::LegionToOracle::Telemetry(t) = &evt.msg {
                    fleet.record_telemetry(&evt.drone_id, t.clone()).await;
                    if let Err(e) = store.record_telemetry(&evt.drone_id, t).await {
                        tracing::warn!(error = ?e, "telemetry persist failed");
                    }
                }
                if let hivemind_protocol::LegionToOracle::Hello {
                    drone_id,
                    legion_version,
                    capabilities,
                    ..
                } = &evt.msg
                {
                    let _ = store
                        .upsert_drone(drone_id, Some(legion_version), capabilities)
                        .await;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(lagged = n, "telemetry pump lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}
