//! # efrust-migrate
//!
//! Runtime de aplicación de migraciones (`efrust database update`). Abstrae la
//! ejecución en un [`Backend`] con dos implementaciones:
//! - **sqlx `Any`**: Postgres, SQLite, MySQL.
//! - **tiberius (TDS)**: SQL Server (fuera del driver `Any` de sqlx).
//!
//! Garantiza la tabla `__EFMigrationsHistory`, calcula el plan hacia el objetivo
//! y aplica cada migración en su propia transacción, renderizando el SQL con el
//! `SqlGenerator` del provider.

use efrust_core::migration::Migration;
use efrust_providers::{Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::{AnyPool, Row};
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

const HISTORY_TABLE: &str = "__EFMigrationsHistory";

type MssqlClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("error de base de datos: {0}")]
    Db(#[from] sqlx::Error),
    #[error("error de SQL Server: {0}")]
    Mssql(#[from] tiberius::error::Error),
    #[error("error de red conectando a SQL Server: {0}")]
    Io(#[from] std::io::Error),
    #[error("error generando SQL: {0}")]
    Provider(#[from] efrust_providers::ProviderError),
    #[error("la migración objetivo '{0}' no existe")]
    TargetNotFound(String),
}

/// Backend de ejecución: sqlx (Any) o tiberius (SQL Server).
enum Backend {
    Sqlx(AnyPool),
    Mssql(Mutex<MssqlClient>),
}

impl Backend {
    /// Ejecuta sentencias dentro de una transacción (todo o nada).
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

    /// Ejecuta una sola sentencia (sin transacción explícita).
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

    /// Devuelve la primera columna (texto) de cada fila.
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
                let rows = c.simple_query(sql.to_string()).await?.into_first_result().await?;
                Ok(rows
                    .iter()
                    .map(|r| r.get::<&str, _>(0).unwrap_or("").to_string())
                    .collect())
            }
        }
    }
}

/// Ejecuta una sentencia en SQL Server y consume su resultado.
async fn mssql_run(client: &mut MssqlClient, sql: &str) -> Result<(), MigrateError> {
    client.simple_query(sql.to_string()).await?.into_results().await?;
    Ok(())
}

/// Resultado de un `update`: qué se aplicó y qué se revirtió, en orden.
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

/// Aplica migraciones contra una base de datos.
pub struct Migrator {
    backend: Backend,
    generator: Box<dyn SqlGenerator>,
    product_version: String,
}

impl Migrator {
    /// Conecta a la base de datos. Para SQL Server espera una cadena ADO
    /// (`Server=...;Database=...;User Id=...;Password=...;TrustServerCertificate=true`);
    /// para el resto, una URL sqlx.
    pub async fn connect(url: &str, provider: Provider) -> Result<Self, MigrateError> {
        let generator = provider.generator();
        let backend = if matches!(provider, Provider::SqlServer) {
            let config = tiberius::Config::from_ado_string(url)?;
            let tcp = tokio::net::TcpStream::connect(config.get_addr()).await?;
            tcp.set_nodelay(true)?;
            let client = tiberius::Client::connect(config, tcp.compat_write()).await?;
            Backend::Mssql(Mutex::new(client))
        } else {
            install_default_drivers();
            let pool = AnyPoolOptions::new().max_connections(5).connect(url).await?;
            Backend::Sqlx(pool)
        };
        Ok(Self {
            backend,
            generator,
            product_version: format!("efrust-{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Crea la tabla de historial si no existe (DDL específico del provider).
    pub async fn ensure_history(&self) -> Result<(), MigrateError> {
        self.backend
            .execute_one(&self.generator.create_history_table_sql())
            .await
    }

    /// Devuelve los `MigrationId` ya aplicados, ordenados ascendentemente.
    pub async fn applied(&self) -> Result<Vec<String>, MigrateError> {
        let t = self.generator.quote_ident(HISTORY_TABLE);
        let id = self.generator.quote_ident("MigrationId");
        self.backend
            .fetch_first_col(&format!("SELECT {id} FROM {t} ORDER BY {id};"))
            .await
    }

    /// Lleva la base de datos al estado `target` (ver doc del CLI).
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
                tracing::info!("revirtiendo {}", m.id);
                self.revert_one(m).await?;
                report.reverted.push(m.id.clone());
            }
        }

        for m in ordered.iter() {
            if m.id.as_str() <= target_id.as_str() && !applied_set.contains(m.id.as_str()) {
                tracing::info!("aplicando {}", m.id);
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

/// Escapa comillas simples para inyectar valores de control de forma segura.
fn escape_literal(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use efrust_core::diff::Operation;
    use efrust_core::migration::Migration;
    use efrust_core::model::{Column, PrimaryKey, Table};

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
            columns: vec![col("Id", "System.Int32", true), col("Name", "System.String", false)],
            primary_key: Some(PrimaryKey { name: "PK_Customer".into(), columns: vec!["Id".into()] }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
        };
        Migration::new(
            "20260607120000_InitialCreate",
            "InitialCreate",
            vec![Operation::CreateTable { table }],
            vec![Operation::DropTable { schema: None, name: "Customer".into() }],
        )
    }

    #[tokio::test]
    async fn aplicar_revertir_e_idempotencia_sqlite() {
        let path = std::env::temp_dir().join(format!("efrust_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let migrator = Migrator::connect(&url, Provider::Sqlite).await.expect("conexión sqlite");
        let migs = vec![initial_create()];

        let r = migrator.update(&migs, None).await.unwrap();
        assert_eq!(r.applied, vec!["20260607120000_InitialCreate".to_string()]);
        assert!(r.reverted.is_empty());
        assert_eq!(migrator.applied().await.unwrap().len(), 1);

        let r2 = migrator.update(&migs, None).await.unwrap();
        assert!(r2.is_empty(), "segunda corrida debe ser no-op");

        let r3 = migrator.update(&migs, Some("0")).await.unwrap();
        assert_eq!(r3.reverted.len(), 1);
        assert!(migrator.applied().await.unwrap().is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
