//! Persistence backends for sorties and their progress checkpoints.

pub mod file_store;

pub use file_store::FileSortieStore;
