//! Trait definitions — the hardware + transport abstraction boundary.
//!
//! Every trait in here is a pure definition; there are no impls and no
//! defaults that reach outside the core. The Pi binary (`legion`) provides
//! std-backed impls using `rppal`, `rust-mavlink`, `tokio-serial`, and
//! `std::fs`; a future MCU binary (`legion-mcu`) will provide the same
//! traits backed by `embedded-hal`, `embassy-stm32-*`, and flash-backed
//! storage.
//!
//! The trait methods use stable `async fn in trait` (Rust ≥1.75). This
//! makes the traits *non-object-safe* — concrete generics are the usage
//! model, not `dyn Trait`. That's fine for legion at v1: exactly one
//! backend is wired in at compile time.

pub mod clock;
pub mod link;
pub mod mavlink;
pub mod payload;
pub mod store;

pub use clock::Clock;
pub use link::Link;
pub use mavlink::MavlinkBackend;
pub use payload::{Nozzle, PaintLevel, Payload, Pump, Tof};
pub use store::SortieStore;
