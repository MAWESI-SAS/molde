//! The per-engine contract for `sync`. Each database engine implements how to
//! read its live structure, how to emit additive DDL, and how to apply it. The
//! diff and the orchestration on top are engine-agnostic. Postgres is implemented
//! today; MySQL / SQLite / SQL Server can be added by implementing this trait.

use anyhow::Result;

use crate::schema::{DbSchema, DiffResult};

#[async_trait::async_trait]
pub trait SyncEngine: Send + Sync {
    /// Engine name, for messages (e.g. `postgres`).
    fn name(&self) -> &'static str;

    /// Read the live structure of the database at `conn` into a `DbSchema`.
    async fn read_schema(&self, conn: &str) -> Result<DbSchema>;

    /// Build the additive SQL body for `diff` (no transaction wrapper).
    fn write_ddl(&self, diff: &DiffResult) -> String;

    /// Apply `body` to `conn` atomically (wrapped in a transaction).
    async fn apply(&self, conn: &str, body: &str) -> Result<()>;

    /// Connection string with the password removed, safe to print/embed.
    fn redact(&self, conn: &str) -> String;
}

/// Pick the engine for a connection string by its scheme. Returns `None` for
/// engines not yet supported by `sync`.
pub fn engine_for(conn: &str) -> Option<Box<dyn SyncEngine>> {
    let c = conn.trim_start();
    if c.starts_with("postgres://") || c.starts_with("postgresql://") {
        return Some(Box::new(crate::postgres::PostgresEngine));
    }
    if c.starts_with("sqlite:") {
        return Some(Box::new(crate::sqlite::SqliteEngine));
    }
    None
}
