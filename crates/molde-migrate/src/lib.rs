//! # molde-migrate
//!
//! Migration application runtime (`molde database update`). Abstracts
//! execution into a [`Backend`] with two implementations:
//! - **sqlx `Any`**: Postgres, SQLite, MySQL.
//! - **tiberius (TDS)**: SQL Server (outside sqlx's `Any` driver).
//!
//! Ensures the `__EFMigrationsHistory` table, computes the plan toward the target
//! and applies each migration in its own transaction, rendering the SQL with the
//! provider's `SqlGenerator`.

use molde_core::migration::Migration;
use molde_providers::{Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::{AnyPool, Row};
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub mod admin;

const HISTORY_TABLE: &str = "__EFMigrationsHistory";

type MssqlClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("SQL Server error: {0}")]
    Mssql(#[from] tiberius::error::Error),
    #[error("network error connecting to SQL Server: {0}")]
    Io(#[from] std::io::Error),
    #[error("error generating SQL: {0}")]
    Provider(#[from] molde_providers::ProviderError),
    #[error("target migration '{0}' does not exist")]
    TargetNotFound(String),
}

/// Execution backend: sqlx (Any) or tiberius (SQL Server).
enum Backend {
    Sqlx(AnyPool),
    // Boxed: the tiberius client is large; avoids bloating the Sqlx variant.
    Mssql(Box<Mutex<MssqlClient>>),
}

impl Backend {
    /// Executes statements inside a transaction (all or nothing).
    async fn execute_all(&self, stmts: &[String]) -> Result<(), MigrateError> {
        match self {
            Backend::Sqlx(pool) => {
                let mut tx = pool.begin().await?;
                for stmt in stmts {
                    sqlx::query(stmt).execute(&mut *tx).await?;
                }
                tx.commit().await?;
                Ok(())
            }
            Backend::Mssql(client) => {
                let mut c = client.lock().await;
                mssql_run(&mut c, "BEGIN TRANSACTION").await?;
                for stmt in stmts {
                    if let Err(e) = mssql_run(&mut c, stmt).await {
                        let _ = mssql_run(&mut c, "ROLLBACK").await;
                        return Err(e);
                    }
                }
                mssql_run(&mut c, "COMMIT").await
            }
        }
    }

    /// Executes a single statement (without an explicit transaction).
    async fn execute_one(&self, sql: &str) -> Result<(), MigrateError> {
        match self {
            Backend::Sqlx(pool) => {
                sqlx::query(sql).execute(pool).await?;
                Ok(())
            }
            Backend::Mssql(client) => {
                let mut c = client.lock().await;
                mssql_run(&mut c, sql).await
            }
        }
    }

    /// Returns the first column (text) of each row.
    async fn fetch_first_col(&self, sql: &str) -> Result<Vec<String>, MigrateError> {
        match self {
            Backend::Sqlx(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    out.push(r.try_get::<String, _>(0)?);
                }
                Ok(out)
            }
            Backend::Mssql(client) => {
                let mut c = client.lock().await;
                let rows = c
                    .simple_query(sql.to_string())
                    .await?
                    .into_first_result()
                    .await?;
                Ok(rows
                    .iter()
                    .map(|r| r.get::<&str, _>(0).unwrap_or("").to_string())
                    .collect())
            }
        }
    }
}

/// Executes a statement on SQL Server and consumes its result.
async fn mssql_run(client: &mut MssqlClient, sql: &str) -> Result<(), MigrateError> {
    client
        .simple_query(sql.to_string())
        .await?
        .into_results()
        .await?;
    Ok(())
}

/// Result of an `update`: what was applied and what was reverted, in order.
#[derive(Debug, Default)]
pub struct UpdateReport {
    pub applied: Vec<String>,
    pub reverted: Vec<String>,
}

impl UpdateReport {
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty() && self.reverted.is_empty()
    }
}

/// Applies migrations against a database.
pub struct Migrator {
    backend: Backend,
    generator: Box<dyn SqlGenerator>,
    product_version: String,
}

impl Migrator {
    /// Connects to the database. For SQL Server it expects an ADO string
    /// (`Server=...;Database=...;User Id=...;Password=...;TrustServerCertificate=true`);
    /// for the rest, a sqlx URL.
    pub async fn connect(url: &str, provider: Provider) -> Result<Self, MigrateError> {
        let generator = provider.generator();
        let backend = if matches!(provider, Provider::SqlServer) {
            let config = tiberius::Config::from_ado_string(url)?;
            let tcp = tokio::net::TcpStream::connect(config.get_addr()).await?;
            tcp.set_nodelay(true)?;
            let client = tiberius::Client::connect(config, tcp.compat_write()).await?;
            Backend::Mssql(Box::new(Mutex::new(client)))
        } else {
            install_default_drivers();
            let pool = AnyPoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?;
            Backend::Sqlx(pool)
        };
        Ok(Self {
            backend,
            generator,
            product_version: format!("molde-{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Creates the history table if it does not exist (provider-specific DDL).
    pub async fn ensure_history(&self) -> Result<(), MigrateError> {
        self.backend
            .execute_one(&self.generator.create_history_table_sql())
            .await
    }

    /// Returns the already-applied `MigrationId`s, sorted ascending.
    pub async fn applied(&self) -> Result<Vec<String>, MigrateError> {
        let t = self.generator.quote_ident(HISTORY_TABLE);
        let id = self.generator.quote_ident("MigrationId");
        self.backend
            .fetch_first_col(&format!("SELECT {id} FROM {t} ORDER BY {id};"))
            .await
    }

    /// Brings the database to the `target` state (see CLI docs).
    pub async fn update(
        &self,
        migrations: &[Migration],
        target: Option<&str>,
    ) -> Result<UpdateReport, MigrateError> {
        self.ensure_history().await?;

        let mut ordered: Vec<&Migration> = migrations.iter().collect();
        ordered.sort_by(|a, b| a.id.cmp(&b.id));

        let applied = self.applied().await?;
        let applied_set: std::collections::HashSet<&str> =
            applied.iter().map(String::as_str).collect();

        let target_id: Option<String> = match target {
            None => ordered.last().map(|m| m.id.clone()),
            Some("0") => Some(String::new()),
            Some(t) => match ordered.iter().find(|m| m.id == t || m.name == t) {
                Some(m) => Some(m.id.clone()),
                None => return Err(MigrateError::TargetNotFound(t.to_string())),
            },
        };
        let Some(target_id) = target_id else {
            return Ok(UpdateReport::default());
        };

        let mut report = UpdateReport::default();

        for m in ordered.iter().rev() {
            if m.id.as_str() > target_id.as_str() && applied_set.contains(m.id.as_str()) {
                tracing::info!("reverting {}", m.id);
                self.revert_one(m).await?;
                report.reverted.push(m.id.clone());
            }
        }

        for m in ordered.iter() {
            if m.id.as_str() <= target_id.as_str() && !applied_set.contains(m.id.as_str()) {
                tracing::info!("applying {}", m.id);
                self.apply_one(m).await?;
                report.applied.push(m.id.clone());
            }
        }

        Ok(report)
    }

    async fn apply_one(&self, m: &Migration) -> Result<(), MigrateError> {
        let t = self.generator.quote_ident(HISTORY_TABLE);
        let id = self.generator.quote_ident("MigrationId");
        let ver = self.generator.quote_ident("ProductVersion");
        let mut stmts = self.generator.emit_all(&m.up)?;
        stmts.push(format!(
            "INSERT INTO {t} ({id}, {ver}) VALUES ('{}', '{}');",
            escape_literal(&m.id),
            escape_literal(&self.product_version),
        ));
        self.backend.execute_all(&stmts).await
    }

    async fn revert_one(&self, m: &Migration) -> Result<(), MigrateError> {
        let t = self.generator.quote_ident(HISTORY_TABLE);
        let id = self.generator.quote_ident("MigrationId");
        let mut stmts = self.generator.emit_all(&m.down)?;
        stmts.push(format!(
            "DELETE FROM {t} WHERE {id} = '{}';",
            escape_literal(&m.id),
        ));
        self.backend.execute_all(&stmts).await
    }
}

/// Escapes single quotes to inject control values safely.
fn escape_literal(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use molde_core::diff::Operation;
    use molde_core::migration::Migration;
    use molde_core::model::{Column, PrimaryKey, Table};

    fn col(name: &str, clr: &str, identity: bool) -> Column {
        Column {
            name: name.into(),
            store_type: None,
            clr_type: Some(clr.into()),
            is_nullable: false,
            is_identity: identity,
            max_length: None,
            precision: None,
            scale: None,
            default_value_sql: None,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        }
    }

    fn initial_create() -> Migration {
        let table = Table {
            name: "Customer".into(),
            schema: None,
            clr_type: Some("App.Models.Customer".into()),
            comment: None,
            columns: vec![
                col("Id", "System.Int32", true),
                col("Name", "System.String", false),
            ],
            primary_key: Some(PrimaryKey {
                name: "PK_Customer".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
            seed_key: vec![],
        };
        Migration::new(
            "20260607120000_InitialCreate",
            "InitialCreate",
            vec![Operation::CreateTable { table }],
            vec![Operation::DropTable {
                schema: None,
                name: "Customer".into(),
            }],
        )
    }

    #[tokio::test]
    async fn apply_revert_and_idempotency_sqlite() {
        let path = std::env::temp_dir().join(format!("molde_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let migrator = Migrator::connect(&url, Provider::Sqlite)
            .await
            .expect("sqlite connection");
        let migs = vec![initial_create()];

        let r = migrator.update(&migs, None).await.unwrap();
        assert_eq!(r.applied, vec!["20260607120000_InitialCreate".to_string()]);
        assert!(r.reverted.is_empty());
        assert_eq!(migrator.applied().await.unwrap().len(), 1);

        let r2 = migrator.update(&migs, None).await.unwrap();
        assert!(r2.is_empty(), "second run must be a no-op");

        let r3 = migrator.update(&migs, Some("0")).await.unwrap();
        assert_eq!(r3.reverted.len(), 1);
        assert!(migrator.applied().await.unwrap().is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
