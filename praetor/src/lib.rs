//! `praetor_lib` — library crate for the praetor Tauri app.
//!
//! Everything except the Tauri `main()` entry point lives here so the same
//! code is reusable from integration tests.
//!
//! See `praetor/README.md` for the design overview.

pub mod config;
pub mod error;
pub mod gamepad;
pub mod mavlink_link;
pub mod safety;
pub mod state;
pub mod tauri_commands;

pub use config::Config;
pub use error::{PraetorError, Result};
pub use state::{AppState, ArmingKind, ArmingState, ControllerStatus, LinkStatus};
