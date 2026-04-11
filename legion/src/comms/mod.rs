//! Oracle link client.
//!
//! Owns the concrete `hivemind_protocol::Transport` in a single tokio
//! task (`CommsClient`) and exposes two other things to the rest of
//! legion:
//!
//! - `ExecutorLink` — a `legion_core::Link` impl that the executor
//!   talks to via tokio mpsc channels.
//! - A broadcast-style inbound dispatch that feeds non-executor frames
//!   (heartbeats, RTK, UploadSortie, Hello) to the rest of the runtime.

pub mod client;
pub mod link;

pub use client::{spawn_comms_client, CommsCommand, CommsHandle, CommsInbound};
pub use link::ExecutorLink;
