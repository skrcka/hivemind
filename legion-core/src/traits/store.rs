//! Sortie persistence abstraction.
//!
//! The Pi binary backs this with JSON files in `/var/lib/legion/sorties/`.
//! An MCU binary backs it with flash storage, or with a `NullStore` that
//! simply forgets across reboots.
//!
//! The contract: on reboot, legion scans the store for any persisted
//! sorties, and if there's one with a progress checkpoint but no
//! completion marker, it reports it in the initial `Hello` as an
//! in-progress sortie — **without** automatically resuming. The
//! operator decides.

use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

use crate::error::StoreError;
use hivemind_protocol::{Sortie, SortieId};

/// Progress checkpoint written after each `StepComplete`. Survives
/// crashes; read on boot to reconstruct an `InProgressSortie`.
#[derive(Debug, Clone, PartialEq)]
pub struct SortieProgress {
    pub sortie_id: SortieId,
    pub last_completed_step: Option<u32>,
    /// Monotonic ms at the checkpoint, from [`crate::Clock::now_ms`].
    pub checkpoint_ms: u64,
}

/// Persistence for sorties and their progress checkpoints.
pub trait SortieStore: Send + Sync {
    /// Persist a freshly received sortie. Called from the comms client
    /// after validation but before the `SortieReceived` reply.
    fn put(
        &self,
        sortie: &Sortie,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Load a sortie by id.
    fn get(
        &self,
        sortie_id: &str,
    ) -> impl Future<Output = Result<Sortie, StoreError>> + Send;

    /// Write a progress checkpoint for a sortie. Atomic — partial writes
    /// must not corrupt the previous checkpoint.
    fn checkpoint(
        &self,
        progress: &SortieProgress,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Load the last checkpoint for a sortie, if any.
    fn load_progress(
        &self,
        sortie_id: &str,
    ) -> impl Future<Output = Result<Option<SortieProgress>, StoreError>> + Send;

    /// List all sortie ids currently in the store.
    fn list(&self) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send;

    /// Mark a sortie complete (by writing a completion marker or
    /// deleting it — impl's choice). After this, `load_progress` may
    /// return `None`.
    fn mark_complete(
        &self,
        sortie_id: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
