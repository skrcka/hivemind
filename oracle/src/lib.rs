//! Oracle library — re-exported so integration tests in `tests/` can use the
//! domain types, slicer, and apply layer without going through the binary.

pub mod api;
pub mod apply;
pub mod cli;
pub mod config;
pub mod domain;
pub mod error;
pub mod fleet;
pub mod legion_link;
pub mod slicer;
pub mod store;
