//! Error types used across `legion-core`. These are intentionally plain
//! enums with `Debug + Display` — no `thiserror`, because the crate is
//! `no_std` by default and `thiserror` pulls in std on older versions.
//!
//! The hosting binary maps its own error types into these at the trait
//! boundary.

use alloc::string::String;
use core::fmt;

/// Errors the payload traits (`Pump`, `Nozzle`, `Tof`, `PaintLevel`) can
/// return. The driver-specific error (I²C bus fault, PWM init failure,
/// etc.) is squashed into `Other { detail }` at the trait boundary.
#[derive(Debug, Clone)]
pub enum PayloadError {
    /// The underlying device reported a transient error — the caller may
    /// retry.
    Transient { detail: String },
    /// The underlying device reported a permanent failure — the caller
    /// should give up on this device.
    Permanent { detail: String },
    /// The device is not installed on this drone at all.
    NotInstalled,
    /// Catch-all for driver-specific failures.
    Other { detail: String },
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transient { detail } => write!(f, "payload transient: {detail}"),
            Self::Permanent { detail } => write!(f, "payload permanent: {detail}"),
            Self::NotInstalled => write!(f, "payload device not installed"),
            Self::Other { detail } => write!(f, "payload: {detail}"),
        }
    }
}

/// Errors the `MavlinkBackend` trait can return. Mapped from the binary's
/// concrete driver errors (e.g. `rust-mavlink` + `tokio-serial`).
#[derive(Debug, Clone)]
pub enum MavlinkError {
    /// Pixhawk is not currently responding (no HEARTBEAT in the expected
    /// window, or a command didn't ACK within its timeout).
    Unreachable,
    /// The autopilot rejected the command (refused mode change, unknown
    /// command ID, etc.).
    Rejected { detail: String },
    /// Internal driver failure (serial port closed, bad MAVLink frame,
    /// ...).
    Io { detail: String },
    /// The backend ran its own state machine and decided the command is
    /// illegal in the current phase (e.g. takeoff while already in air).
    IllegalState { detail: String },
}

impl fmt::Display for MavlinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable => write!(f, "mavlink: pixhawk unreachable"),
            Self::Rejected { detail } => write!(f, "mavlink: rejected: {detail}"),
            Self::Io { detail } => write!(f, "mavlink: io: {detail}"),
            Self::IllegalState { detail } => write!(f, "mavlink: illegal state: {detail}"),
        }
    }
}

/// Errors the `SortieStore` trait can return.
#[derive(Debug, Clone)]
pub enum StoreError {
    NotFound { sortie_id: String },
    Io { detail: String },
    Corrupt { detail: String },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { sortie_id } => write!(f, "store: sortie {sortie_id} not found"),
            Self::Io { detail } => write!(f, "store: io: {detail}"),
            Self::Corrupt { detail } => write!(f, "store: corrupt: {detail}"),
        }
    }
}

/// Errors the `Link` trait can return. The link is the binary's wrapper
/// around `hivemind_protocol::Transport`; its concrete transport error
/// (serial / TCP) is squashed into `Transport { detail }`.
#[derive(Debug, Clone)]
pub enum LinkError {
    /// The underlying transport is not currently connected. The executor
    /// can still run under the radio-loss policy.
    NotConnected,
    /// Transport-level failure (serial EOF, TCP reset, codec error).
    Transport { detail: String },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "link: not connected"),
            Self::Transport { detail } => write!(f, "link: transport: {detail}"),
        }
    }
}

/// The umbrella error type for `legion-core`. The executor returns these;
/// every step handler, radio-loss policy, and safety check maps its own
/// error kind into one variant.
#[derive(Debug, Clone)]
pub enum CoreError {
    Payload(PayloadError),
    Mavlink(MavlinkError),
    Store(StoreError),
    Link(LinkError),
    /// Oracle sent a `Proceed` for a step index other than the one legion
    /// is currently blocked on. The binary should forward an `Error` frame
    /// back to oracle.
    ProceedOutOfOrder {
        sortie_id: String,
        expected: u32,
        got: u32,
    },
    /// Oracle sent a sortie-level command for a sortie that isn't the
    /// currently-executing one. Ignored by legion and reported as an error
    /// to the operator.
    WrongSortie {
        current: String,
        got: String,
    },
    /// Executor received an `AbortSortie` from oracle mid-execution. The
    /// executor honours the abort and unwinds cleanly.
    AbortedByOracle { reason: String },
    /// Executor was told `ReturnToBase` mid-execution. Higher priority
    /// than `AbortedByOracle` — the active step is dropped immediately.
    RtlByOracle { reason: String },
    /// A step's handler exceeded the per-step expected duration watchdog.
    StepTimeout { step_index: u32 },
    /// The safety loop raised a preemption and the binary cancelled the
    /// executor.
    SafetyPreemption { detail: String },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Payload(e) => write!(f, "{e}"),
            Self::Mavlink(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
            Self::Link(e) => write!(f, "{e}"),
            Self::ProceedOutOfOrder {
                sortie_id,
                expected,
                got,
            } => write!(
                f,
                "proceed out of order for {sortie_id}: expected step {expected}, got {got}"
            ),
            Self::WrongSortie { current, got } => {
                write!(f, "command for wrong sortie (current={current}, got={got})")
            }
            Self::AbortedByOracle { reason } => write!(f, "aborted by oracle: {reason}"),
            Self::RtlByOracle { reason } => write!(f, "rtl by oracle: {reason}"),
            Self::StepTimeout { step_index } => {
                write!(f, "step {step_index} exceeded expected duration")
            }
            Self::SafetyPreemption { detail } => write!(f, "safety preemption: {detail}"),
        }
    }
}

impl From<PayloadError> for CoreError {
    fn from(e: PayloadError) -> Self {
        Self::Payload(e)
    }
}

impl From<MavlinkError> for CoreError {
    fn from(e: MavlinkError) -> Self {
        Self::Mavlink(e)
    }
}

impl From<StoreError> for CoreError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<LinkError> for CoreError {
    fn from(e: LinkError) -> Self {
        Self::Link(e)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CoreError {}

#[cfg(feature = "std")]
impl std::error::Error for PayloadError {}

#[cfg(feature = "std")]
impl std::error::Error for MavlinkError {}

#[cfg(feature = "std")]
impl std::error::Error for StoreError {}

#[cfg(feature = "std")]
impl std::error::Error for LinkError {}
