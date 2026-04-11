//! `legion-core` — portable drone-side executor, safety check, and hardware
//! traits for the Hivemind swarm.
//!
//! This crate is `#![no_std]` with `extern crate alloc;`. Everything inside
//! compiles for both the v1 Pi 5 companion-computer target (std + tokio, via
//! the `legion` crate) and the v3 bare-metal MCU target (no_std + embassy,
//! via a future `legion-mcu` crate). The split is strict: this crate owns
//! *logic*; the hosting binary owns *impls*.
//!
//! See `legion/README.md` for the architectural context. The two halves:
//!
//! - **Traits** ([`traits`]) — `Payload`, `MavlinkBackend`, `SortieStore`,
//!   `Clock`, `Link`. Definitions only — the hosting binary supplies impls.
//! - **Logic** — the sortie [`executor`] (with the Proceed handshake + per-
//!   step radio-loss policy) and the [`safety`] check (single-tick, no
//!   timer types).
//!
//! # Runtime assumptions
//!
//! - `core::future::Future` and stable `async fn in trait` (Rust ≥1.75).
//! - No `std::sync` types (`Arc`, `Mutex`). State sharing happens in the
//!   hosting binary.
//! - No `tokio`, `embassy`, or any other runtime. The futures returned by
//!   trait methods are polled by whatever runtime the binary provides.
//! - `alloc::{vec::Vec, string::String, boxed::Box}` are OK. A global
//!   allocator is assumed on the MCU target.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod error;
pub mod executor;
pub mod safety;
pub mod state;
pub mod traits;

pub use error::{CoreError, LinkError, MavlinkError, PayloadError, StoreError};
pub use executor::{ExecutorEvent, StepOutcome};
pub use safety::{SafetyConfig, SafetyOutcome, SafetyState};
pub use state::LegionState;
pub use traits::{Clock, Link, MavlinkBackend, PaintLevel, Payload, SortieStore, Tof};

/// Re-exports of the wire types legion's logic operates on. The core never
/// defines its own Sortie — it uses the canonical protocol crate types.
pub use hivemind_protocol::{
    DroneId, LegionToOracle, OracleToLegion, Position, RadioLossBehaviour, RadioLossPolicy, Sortie,
    SortieId, SortieStep, StepType, Waypoint,
};
