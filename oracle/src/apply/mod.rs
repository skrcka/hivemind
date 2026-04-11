//! Apply phase — drives sorties through the step-confirmation handshake
//! against legion.
//!
//! See [`oracle/README.md → Apply phase`] for the design.
//!
//! [`oracle/README.md → Apply phase`]: ../../../README.md#apply-phase--the-step-confirmation-handshake

pub mod gate;
pub mod handshake;
pub mod supervisor;

pub use gate::{Gate, GateContext, GateEvaluator, OperatorDecision};
pub use handshake::{handshake_one_sortie, HandshakeError};
pub use supervisor::{spawn_apply, ApplyError, OperatorSignals};
