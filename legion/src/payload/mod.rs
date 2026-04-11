//! Payload hardware drivers. The default build uses `MockPayload` so
//! the binary compiles and runs on any dev machine. The `pi-hardware`
//! feature swaps in `RppalPayload` on Linux/Pi 5.

pub mod mock;

#[cfg(feature = "pi-hardware")]
pub mod rppal_impl;

pub use mock::MockPayload;

#[cfg(feature = "pi-hardware")]
pub use rppal_impl::RppalPayload;
