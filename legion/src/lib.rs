//! Library crate for `legion` — exposes the Pi-side runtime modules so
//! integration tests and the `legion` binary entry point (`main.rs`) can
//! both consume them.

pub mod clock;
pub mod cli;
pub mod comms;
pub mod config;
pub mod mavlink_driver;
pub mod payload;
pub mod runtime;
pub mod safety_loop;
pub mod shared_state;
pub mod store;

pub use clock::TokioClock;
pub use config::{Config, LegionError};
pub use shared_state::SharedState;
