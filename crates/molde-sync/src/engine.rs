//! The per-engine contract for `sync`. Each database engine implements how to
//! read its live structure, how to emit additive DDL, and how to apply it. The
//! diff and the orchestration on top are engine-agnostic. PostgreSQL, MySQL,
//! SQLite and SQL Server are implemented; more can be added by implementing this
//! trait.

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

    /// Wrap the additive `body` in the engine's transaction syntax for the
    /// portable `.sql` file (the engine's own [`apply`](Self::apply) wraps its
    /// transaction independently). Most engines accept `BEGIN; … COMMIT;`.
    fn wrap_script(&self, body: &str) -> String {
        format!("BEGIN;\n\n{}\nCOMMIT;\n", body)
    }
}

/// Pick the engine for a connection string. SQL URLs are matched by scheme;
/// SQL Server is matched by its ADO string (no scheme). Returns `None` for
/// connection strings no engine recognizes.
pub fn engine_for(conn: &str) -> Option<Box<dyn SyncEngine>> {
    let c = conn.trim_start();
    if c.starts_with("postgres://") || c.starts_with("postgresql://") {
        return Some(Box::new(crate::postgres::PostgresEngine));
    }
    if c.starts_with("sqlite:") {
        return Some(Box::new(crate::sqlite::SqliteEngine));
    }
    if c.starts_with("mysql://") {
        return Some(Box::new(crate::mysql::MysqlEngine));
    }
    // SQL Server uses an ADO connection string (no URL scheme), e.g.
    // `Server=host,1433;Database=db;User Id=sa;Password=…;TrustServerCertificate=true`.
    let lower = c.to_ascii_lowercase();
    if !lower.contains("://") && (lower.contains("server=") || lower.contains("data source=")) {
        return Some(Box::new(crate::sqlserver::SqlServerEngine));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::engine_for;

    #[test]
    fn dispatch_by_scheme_and_ado() {
        assert_eq!(
            engine_for("postgres://u:p@h/db").unwrap().name(),
            "postgres"
        );
        assert_eq!(
            engine_for("postgresql://u:p@h/db").unwrap().name(),
            "postgres"
        );
        assert_eq!(engine_for("sqlite:/tmp/a.db").unwrap().name(), "sqlite");
        assert_eq!(engine_for("mysql://u:p@h/db").unwrap().name(), "mysql");
        assert_eq!(
            engine_for(
                "Server=h,1433;Database=db;User Id=sa;Password=x;TrustServerCertificate=true"
            )
            .unwrap()
            .name(),
            "sqlserver"
        );
        assert_eq!(
            engine_for("Data Source=h;Initial Catalog=db;User Id=sa;Password=x")
                .unwrap()
                .name(),
            "sqlserver"
        );
        assert!(engine_for("redis://h/0").is_none());
    }
}
