//! MAVLink driver backends.
//!
//! v1 ships a `StubMavlinkDriver` that simulates the autopilot in
//! software — it just logs every command, updates an internal
//! position/battery model, and resolves every future immediately. This
//! is what the binary builds against by default so the whole stack is
//! runnable on any dev machine, in SITL, and against a mock legion pair
//! in oracle's integration tests.
//!
//! The "real" driver that talks to a Pixhawk over TELEM2 via
//! `rust-mavlink` + `tokio-serial` lives in a separate module to be
//! added once the SITL spike resolves README open question #1
//! (mission-mode vs offboard for spray paths). Both backends implement
//! the same `legion_core::MavlinkBackend` trait so the swap is a
//! type change in `runtime.rs`.

pub mod stub;

pub use stub::StubMavlinkDriver;
