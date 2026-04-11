//! SQLite persistence via sqlx. One module per table family.
//!
//! v1 uses runtime-checked queries (`sqlx::query`, `sqlx::query_as`) rather
//! than the compile-time `sqlx::query!` macros. The macros require either a
//! live `DATABASE_URL` at build time or an offline cache; both add CI
//! complexity. We can switch to macros later when CI is set up.

pub mod audit;
pub mod drones;
pub mod intents;
pub mod plans;
pub mod sorties;
pub mod steps;

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};

/// Migrations embedded into the binary at compile time. Applied at startup
/// via [`Store::migrate`].
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Handle to the SQLite connection pool. Cheaply cloneable (the pool is
/// internally reference-counted).
#[derive(Clone, Debug)]
pub struct Store {
    pool: Pool<Sqlite>,
}

impl Store {
    /// Open (or create) a SQLite database at `path` and run migrations.
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// In-memory store, used by tests. Each call to `open_memory` returns an
    /// independent fresh database.
    pub async fn open_memory() -> Result<Self, sqlx::Error> {
        let opts = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}
