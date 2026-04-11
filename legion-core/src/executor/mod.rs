//! Sortie executor. Walks a `Sortie` step by step, runs the explicit
//! `Proceed` handshake with oracle between every step, and enforces each
//! step's radio-loss policy if oracle goes silent.
//!
//! The executor is generic over the hardware traits — it doesn't know
//! whether it's running on a Pi or an MCU. The hosting binary builds an
//! [`Executor`] with concrete `Payload`, `MavlinkBackend`, `SortieStore`,
//! `Link`, and `Clock` impls and calls [`Executor::run_sortie`].
//!
//! Safety preemption is not handled *inside* this module — the safety
//! loop is a parallel task that may drop the pump or RTL the drone
//! independently. The hosting binary's wrapper around `run_sortie` is
//! what adds the `tokio::select!` that cancels the executor future on
//! a safety trip.

pub mod machine;
pub mod radio_loss;
pub mod steps;

pub use machine::Executor;
pub use steps::StepOutcome;

// Re-export the event type so `legion-core::ExecutorEvent` works at
// the crate root.
pub use crate::traits::link::ExecutorEvent;
