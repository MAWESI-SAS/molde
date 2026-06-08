//! Database lifecycle — one level above the schema. `migrate`/`apply` operate
//! *inside* an existing database; these create, drop and check the **database
//! itself** (like `createdb`/`dropdb`). For Postgres/MySQL/SQLite this uses
//! sqlx's `MigrateDatabase` (which connects to the maintenance database itself);
//! SQL Server is handled with tiberius against `master`.

use molde_providers::Provider;
use sqlx::migrate::MigrateDatabase;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::MigrateError;

type MssqlClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

/// Does the database the connection points to exist?
pub async fn database_exists(conn: &str, provider: Provider) -> Result<bool, MigrateError> {
    Ok(match provider {
        Provider::Postgres => sqlx::Postgres::database_exists(conn).await?,
        Provider::MySql => sqlx::MySql::database_exists(conn).await?,
        Provider::Sqlite => sqlx::Sqlite::database_exists(conn).await?,
        Provider::SqlServer => mssql_exists(conn).await?,
    })
}

/// Create the database if absent. Returns `true` if it was created.
pub async fn create_database(conn: &str, provider: Provider) -> Result<bool, MigrateError> {
    if database_exists(conn, provider).await? {
        return Ok(false);
    }
    match provider {
        Provider::Postgres => sqlx::Postgres::create_database(conn).await?,
        Provider::MySql => sqlx::MySql::create_database(conn).await?,
        Provider::Sqlite => sqlx::Sqlite::create_database(conn).await?,
        Provider::SqlServer => mssql_create(conn).await?,
    }
    Ok(true)
}

/// Drop the database if present. Returns `true` if it was dropped.
pub async fn drop_database(conn: &str, provider: Provider) -> Result<bool, MigrateError> {
    if !database_exists(conn, provider).await? {
        return Ok(false);
    }
    match provider {
        Provider::Postgres => sqlx::Postgres::drop_database(conn).await?,
        Provider::MySql => sqlx::MySql::drop_database(conn).await?,
        Provider::Sqlite => sqlx::Sqlite::drop_database(conn).await?,
        Provider::SqlServer => mssql_drop(conn).await?,
    }
    Ok(true)
}

// ---- SQL Server (tiberius against `master`) ----

/// The `Database=` / `Initial Catalog=` value of an ADO string.
fn mssql_db_name(conn: &str) -> Option<String> {
    conn.split(';').find_map(|seg| {
        let (k, v) = seg.split_once('=')?;
        let k = k.trim().to_ascii_lowercase();
        (k == "database" || k == "initial catalog").then(|| v.trim().to_string())
    })
}

/// The same ADO string but pointing at `master`.
fn mssql_master_conn(conn: &str) -> String {
    let mut found = false;
    let mut parts: Vec<String> = conn
        .split(';')
        .map(|seg| match seg.split_once('=') {
            Some((k, _))
                if matches!(
                    k.trim().to_ascii_lowercase().as_str(),
                    "database" | "initial catalog"
                ) =>
            {
                found = true;
                format!("{}=master", k.trim())
            }
            _ => seg.to_string(),
        })
        .collect();
    if !found {
        parts.push("Database=master".to_string());
    }
    parts.join(";")
}

async fn mssql_connect_master(conn: &str) -> Result<MssqlClient, MigrateError> {
    let config = tiberius::Config::from_ado_string(&mssql_master_conn(conn))?;
    let tcp = tokio::net::TcpStream::connect(config.get_addr()).await?;
    tcp.set_nodelay(true).ok();
    Ok(tiberius::Client::connect(config, tcp.compat_write()).await?)
}

async fn mssql_exists(conn: &str) -> Result<bool, MigrateError> {
    let Some(name) = mssql_db_name(conn) else {
        return Ok(false);
    };
    let mut c = mssql_connect_master(conn).await?;
    let rows = c
        .simple_query(format!(
            "SELECT 1 FROM sys.databases WHERE name = N'{}'",
            name.replace('\'', "''")
        ))
        .await?
        .into_first_result()
        .await?;
    Ok(!rows.is_empty())
}

async fn mssql_create(conn: &str) -> Result<(), MigrateError> {
    if let Some(name) = mssql_db_name(conn) {
        let mut c = mssql_connect_master(conn).await?;
        c.simple_query(format!("CREATE DATABASE [{}]", name.replace(']', "]]")))
            .await?
            .into_results()
            .await?;
    }
    Ok(())
}

async fn mssql_drop(conn: &str) -> Result<(), MigrateError> {
    if let Some(name) = mssql_db_name(conn) {
        let b = name.replace(']', "]]");
        let mut c = mssql_connect_master(conn).await?;
        // Kick existing connections, then drop.
        c.simple_query(format!(
            "ALTER DATABASE [{b}] SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE [{b}];"
        ))
        .await?
        .into_results()
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{mssql_db_name, mssql_master_conn};

    #[test]
    fn ado_name_and_master_rewrite() {
        let c = "Server=h,1433;Database=app;User Id=sa;Password=x";
        assert_eq!(mssql_db_name(c).as_deref(), Some("app"));
        assert_eq!(
            mssql_master_conn(c),
            "Server=h,1433;Database=master;User Id=sa;Password=x"
        );
        // Initial Catalog and missing-database forms.
        assert_eq!(
            mssql_db_name("Data Source=h;Initial Catalog=db;User Id=sa").as_deref(),
            Some("db")
        );
        assert!(mssql_master_conn("Server=h;User Id=sa").ends_with(";Database=master"));
    }
}
