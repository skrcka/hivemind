//! Legion Link — the only path between oracle and the drone fleet.
//!
//! The link is structured as one server task that owns a `Transport`
//! (TCP listener or serial port) and a `HashMap<DroneId, SessionHandle>`.
//! Per-drone session tasks read incoming frames into a broadcast channel
//! and serve outgoing commands from a per-drone mpsc mailbox.

pub mod authority;
pub mod server;
pub mod session;

pub use authority::{AuthorityKind, CommandAuthority, HoldReason};

use std::collections::HashMap;
use std::sync::Arc;

use hivemind_protocol::{LegionToOracle, OracleToLegion, SortieId};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::domain::plan::PlanId;

/// Errors returned by the public `Link` surface.
#[derive(Debug, Error)]
pub enum LinkError {
    #[error("drone not connected: {0}")]
    DroneNotConnected(String),
    #[error("send failed: {0}")]
    Send(String),
    #[error("receive failed: {0}")]
    Recv(String),
    #[error("transport closed")]
    Closed,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// One observed event from a legion. The drone id is duplicated outside the
/// envelope so subscribers can filter without parsing every variant.
#[derive(Debug, Clone)]
pub struct LegionEvent {
    pub drone_id: String,
    pub msg: LegionToOracle,
}

/// Per-drone session handle held by the Link's drone map.
#[derive(Debug, Clone)]
struct SessionHandle {
    /// Outbound command mailbox. Closing this drops the session.
    sender: mpsc::Sender<OracleToLegion>,
}

/// Public Legion Link handle. Cheaply cloneable. Created by
/// [`server::start_tcp`] or [`server::start_serial`].
#[derive(Clone)]
pub struct Link {
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    events_tx: broadcast::Sender<LegionEvent>,
}

impl Link {
    /// Subscribe to inbound events from every connected legion.
    pub fn subscribe(&self) -> broadcast::Receiver<LegionEvent> {
        self.events_tx.subscribe()
    }

    /// `true` if at least one connected legion is registered with `drone_id`.
    pub async fn is_connected(&self, drone_id: &str) -> bool {
        self.sessions.lock().await.contains_key(drone_id)
    }

    /// Number of currently-connected legions.
    pub async fn connected_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Upload a sortie to a specific drone.
    pub async fn upload_sortie(
        &self,
        drone_id: &str,
        sortie: &hivemind_protocol::Sortie,
        _auth: &CommandAuthority,
    ) -> Result<(), LinkError> {
        self.send(drone_id, OracleToLegion::UploadSortie {
            sortie: sortie.clone(),
        })
        .await
    }

    /// Tell legion it can start the next step.
    pub async fn send_proceed(
        &self,
        drone_id: &str,
        sortie_id: &SortieId,
        expected_step_index: u32,
        _auth: &CommandAuthority,
    ) -> Result<(), LinkError> {
        self.send(
            drone_id,
            OracleToLegion::Proceed {
                sortie_id: sortie_id.clone(),
                expected_step_index,
            },
        )
        .await
    }

    /// Tell legion to hold at the current position before starting the next
    /// step.
    pub async fn send_hold(
        &self,
        drone_id: &str,
        sortie_id: &SortieId,
        reason: String,
        _auth: &CommandAuthority,
    ) -> Result<(), LinkError> {
        self.send(
            drone_id,
            OracleToLegion::HoldStep {
                sortie_id: sortie_id.clone(),
                reason,
            },
        )
        .await
    }

    /// Clean abort: legion stops the executor, RTLs the drone.
    pub async fn send_abort(
        &self,
        drone_id: &str,
        sortie_id: &SortieId,
        reason: String,
        _auth: AuthorityKind,
    ) -> Result<(), LinkError> {
        self.send(
            drone_id,
            OracleToLegion::AbortSortie {
                sortie_id: sortie_id.clone(),
                reason,
            },
        )
        .await
    }

    /// Hard RTL — overrides whatever step is in flight.
    pub async fn return_to_base(
        &self,
        drone_id: &str,
        reason: String,
        _auth: AuthorityKind,
    ) -> Result<(), LinkError> {
        self.send(drone_id, OracleToLegion::ReturnToBase { reason })
            .await
    }

    /// Hold position while still in a step. Used by the Fleet Monitor for
    /// Layer 2 deconfliction holds.
    pub async fn hold_position(
        &self,
        drone_id: &str,
        reason: HoldReason,
        _auth: AuthorityKind,
    ) -> Result<(), LinkError> {
        self.send(
            drone_id,
            OracleToLegion::HoldStep {
                sortie_id: String::new(),
                reason: reason.as_str().into(),
            },
        )
        .await
    }

    /// Send opaque RTCM3 bytes to a single drone.
    pub async fn send_rtk(
        &self,
        drone_id: &str,
        payload: Vec<u8>,
        _auth: AuthorityKind,
    ) -> Result<(), LinkError> {
        self.send(drone_id, OracleToLegion::RtkCorrection { payload })
            .await
    }

    /// Broadcast RTCM3 bytes to every connected legion.
    pub async fn broadcast_rtk(&self, payload: Vec<u8>) -> Result<(), LinkError> {
        let sessions = self.sessions.lock().await;
        for handle in sessions.values() {
            let _ = handle
                .sender
                .send(OracleToLegion::RtkCorrection {
                    payload: payload.clone(),
                })
                .await;
        }
        Ok(())
    }

    /// Internal: dispatch one outbound message to a specific drone.
    async fn send(&self, drone_id: &str, msg: OracleToLegion) -> Result<(), LinkError> {
        let sessions = self.sessions.lock().await;
        let Some(handle) = sessions.get(drone_id) else {
            return Err(LinkError::DroneNotConnected(drone_id.to_string()));
        };
        handle
            .sender
            .send(msg)
            .await
            .map_err(|_| LinkError::Closed)
    }

    /// Mint an authority token for an Approved plan. Only the Apply
    /// Supervisor's `start()` should call this.
    pub fn authority_for_plan(&self, plan_id: PlanId) -> CommandAuthority {
        CommandAuthority::for_approved_plan(plan_id)
    }

    /// Mint a safety-override authority token. Only the safety/abort path
    /// should call this.
    pub fn safety_override_authority(&self) -> AuthorityKind {
        AuthorityKind::SafetyOverride
    }
}
