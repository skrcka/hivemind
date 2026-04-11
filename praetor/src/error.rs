//! Error types for praetor.
//!
//! All public APIs return `Result<T, PraetorError>`. The variant set is
//! intentionally small and coarse — this is a single-operator desktop app,
//! not a multi-tenant service, so we do not need a rich error taxonomy for
//! API clients.

use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, PraetorError>;

#[derive(Debug, Error)]
pub enum PraetorError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Serial port error: {0}")]
    Serial(#[from] tokio_serial::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("MAVLink error: {0}")]
    Mavlink(String),

    #[error("Link not connected")]
    NotConnected,

    #[error("Safety interlock: {0}")]
    Interlock(String),

    #[error("Mode handoff required: drone is not in a manual-capable flight mode")]
    ModeHandoffRequired,

    #[error("Arming refused: {0}")]
    ArmingRefused(String),

    #[error("Gamepad error: {0}")]
    Gamepad(String),

    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

// Tauri commands must return a type that implements serde::Serialize. We
// render any error as a plain string on the wire; the frontend displays it
// in a toast/banner. The canonical structured form stays in the tracing log.
impl serde::Serialize for PraetorError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
