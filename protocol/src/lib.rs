//! `hivemind-protocol` — wire-format crate for the Hivemind oracle ↔ legion link.
//!
//! This crate is the contract between the [`oracle`] and [`legion`] binaries:
//! it defines the message types, the postcard + COBS wire codec, and the
//! [`Transport`] trait abstraction. Both binaries link this crate so wire-format
//! drift between them is a `cargo check` error.
//!
//! See `protocol/README.md` at the workspace root for design context.
//!
//! # Crate properties
//!
//! - `#![no_std]` with `extern crate alloc;`. Standard collection types
//!   (`String`, `Vec`) are required and supplied by `alloc`.
//! - The core type definitions and codec compile cleanly under `no_std + alloc`.
//! - The transport implementations (`TcpTransport`, `SerialTransport`) live
//!   behind feature flags that opt into `std` and `tokio`.
//!
//! # Quick example
//!
//! ```
//! use hivemind_protocol::{decode_frame, encode_frame, Envelope, OracleToLegion};
//!
//! let env = Envelope::new("drone-01", 0, OracleToLegion::Heartbeat);
//! let frame = encode_frame(&env).unwrap();
//! assert_eq!(frame.last(), Some(&0)); // trailing COBS delimiter
//!
//! let body = &frame[..frame.len() - 1];
//! let decoded: Envelope<OracleToLegion> = decode_frame(body).unwrap();
//! assert_eq!(env, decoded);
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod codec;
pub mod error;
pub mod messages;
pub mod sortie;
pub mod telemetry;
pub mod transport;

#[cfg(feature = "tcp")]
pub mod tcp;

#[cfg(feature = "serial")]
pub mod serial;

pub use codec::{decode_frame, encode_frame, FrameDecoder};
pub use error::CodecError;
pub use messages::{Envelope, LegionToOracle, OracleToLegion, PROTOCOL_VERSION};
pub use sortie::{
    DroneId, InProgressSortie, PlanId, RadioLossBehaviour, RadioLossPolicy, Sortie, SortieId,
    SortieStep, StepType, Waypoint,
};
pub use telemetry::{
    Attitude, DronePhase, GpsFixType, Position, SafetyEventKind, SortieEventKind, Telemetry,
};
pub use transport::Transport;

#[cfg(feature = "tcp")]
pub use tcp::{TcpTransport, TcpTransportError};

#[cfg(feature = "serial")]
pub use serial::{SerialTransport, SerialTransportError};
