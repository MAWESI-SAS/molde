//! SQLite engine for `sync`: reads the live structure from `sqlite_master` +
//! `PRAGMA table_info`, emits additive DDL, and applies it atomically.
//!
//! SQLite has no separate extensions, functions, or `ALTER TABLE ADD CONSTRAINT`:
//! constraints live inline in each table's `CREATE` text. So a new table is
//! emitted from its stored `CREATE TABLE` (which already carries its constraints),
//! and only new columns / indexes / triggers / views are synced on existing tables.

use anyhow::{Context, Result};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::engine::SyncEngine;
use crate::schema::{
    ColumnInfo, DbSchema, DiffResult, IndexInfo, TableInfo, TriggerInfo, ViewInfo,
};

const HISTORY_TABLE: &str = "__EFMigrationsHistory";

pub struct SqliteEngine;

#[async_trait::async_trait]
impl SyncEngine for SqliteEngine {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    async fn read_schema(&self, conn: &str) -> Result<DbSchema> {
        let pool = connect(conn).await?;
        let mut schema = DbSchema::default();
        read_tables(&pool, &mut schema).await?;
        read_indexes(&pool, &mut schema).await?;
        read_triggers(&pool, &mut schema).await?;
        read_views(&pool, &mut schema).await?;
        read_history(&pool, &mut schema).await?;
        pool.close().await;
        Ok(schema)
    }

    fn write_ddl(&self, diff: &DiffResult) -> String {
        build_ddl(diff)
    }

    async fn apply(&self, conn: &str, body: &str) -> Result<()> {
        let pool = connect(conn).await?;
        let script = format!("BEGIN;\n{body}\nCOMMIT;");
        let result = sqlx::raw_sql(&script)
            .execute(&pool)
            .await
            .context("applying the sync script");
        pool.close().await;
        result.map(|_| ())
    }

    fn redact(&self, conn: &str) -> String {
        // SQLite connections carry no password.
        conn.to_string()
    }
}

async fn connect(conn: &str) -> Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect(conn)
        .await
        .context("opening the SQLite database")
}

// ---- Readers ----

async fn read_tables(pool: &SqlitePool, schema: &mut DbSchema) -> Result<()> {
    let sql = "SELECT name, sql FROM sqlite_master \
               WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 AND name <> '__EFMigrationsHistory' \
               ORDER BY name";
    let tables: Vec<(String, Option<String>)> = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading tables")?
        .into_iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();

    for (name, create_sql) in tables {
        let pragma = format!("PRAGMA table_info(\"{}\")", name.replace('"', "\"\""));
        let columns = sqlx::query(&pragma)
            .fetch_all(pool)
            .await
            .with_context(|| format!("reading columns of {name}"))?
            .into_iter()
            .map(|r| {
                let not_null: i64 = r.get(3);
                ColumnInfo {
                    name: r.get(1),
                    type_: r.get(2),
                    not_null: not_null != 0,
                    default: r.get(4),
                    is_generated: false,
                }
            })
            .collect();
        schema.tables.insert(
            name.clone(),
            TableInfo {
                name,
                columns,
                create_sql,
            },
        );
    }
    Ok(())
}

async fn read_indexes(pool: &SqlitePool, schema: &mut DbSchema) -> Result<()> {
    // `sql IS NULL` for the implicit indexes backing PK/UNIQUE — those come with
    // the table, so we skip them.
    let sql = "SELECT name, tbl_name, sql FROM sqlite_master \
               WHERE type = 'index' AND sql IS NOT NULL ORDER BY name";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading indexes")?
    {
        let info = IndexInfo {
            name: row.get(0),
            table: row.get(1),
            definition: row.get(2),
        };
        schema.indexes.insert(info.name.clone(), info);
    }
    Ok(())
}

async fn read_triggers(pool: &SqlitePool, schema: &mut DbSchema) -> Result<()> {
    let sql = "SELECT name, tbl_name, sql FROM sqlite_master \
               WHERE type = 'trigger' ORDER BY name";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading triggers")?
    {
        let info = TriggerInfo {
            name: row.get(0),
            table: row.get(1),
            definition: row.get(2),
        };
        schema.triggers.insert(info.key(), info);
    }
    Ok(())
}

async fn read_views(pool: &SqlitePool, schema: &mut DbSchema) -> Result<()> {
    let sql = "SELECT name, sql FROM sqlite_master WHERE type = 'view' ORDER BY name";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading views")?
    {
        let info = ViewInfo {
            name: row.get(0),
            definition: row.get(1),
        };
        schema.views.insert(info.name.clone(), info);
    }
    Ok(())
}

async fn read_history(pool: &SqlitePool, schema: &mut DbSchema) -> Result<()> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '__EFMigrationsHistory'",
    )
    .fetch_optional(pool)
    .await
    .context("checking the migration history table")?;
    if exists.is_none() {
        return Ok(());
    }
    for row in sqlx::query(
        "SELECT \"MigrationId\", \"ProductVersion\" FROM \"__EFMigrationsHistory\" ORDER BY \"MigrationId\"",
    )
    .fetch_all(pool)
    .await
    .context("reading migration history")?
    {
        schema.migration_history.insert(row.get(0), row.get(1));
    }
    Ok(())
}

// ---- DDL writer ----

fn build_ddl(diff: &DiffResult) -> String {
    let mut s = String::new();

    if !diff.new_tables.is_empty() {
        section(&mut s, "NEW TABLES");
        for table in &diff.new_tables {
            // SQLite tables carry their constraints inline in the stored CREATE.
            if let Some(create) = &table.create_sql {
                s.push_str(&ensure_semicolon(&inject_if_not_exists(
                    create,
                    "CREATE TABLE",
                )));
                s.push('\n');
            }
        }
        s.push('\n');
    }

    if !diff.new_columns.is_empty() {
        section(&mut s, "NEW COLUMNS");
        for (table, col) in &diff.new_columns {
            s.push_str(&format!(
                "ALTER TABLE \"{table}\" ADD COLUMN {};\n",
                column_ddl(col)
            ));
        }
        s.push('\n');
    }

    if !diff.new_indexes.is_empty() {
        section(&mut s, "INDEXES");
        for idx in &diff.new_indexes {
            let def = inject_if_not_exists(&idx.definition, "CREATE UNIQUE INDEX");
            let def = inject_if_not_exists(&def, "CREATE INDEX");
            s.push_str(&ensure_semicolon(&def));
            s.push('\n');
        }
        s.push('\n');
    }

    if !diff.new_triggers.is_empty() {
        section(&mut s, "TRIGGERS");
        for trg in &diff.new_triggers {
            s.push_str(&ensure_semicolon(&inject_if_not_exists(
                &trg.definition,
                "CREATE TRIGGER",
            )));
            s.push('\n');
        }
        s.push('\n');
    }

    if !diff.new_views.is_empty() {
        section(&mut s, "VIEWS");
        for view in &diff.new_views {
            s.push_str(&ensure_semicolon(&inject_if_not_exists(
                &view.definition,
                "CREATE VIEW",
            )));
            s.push('\n');
        }
        s.push('\n');
    }

    if !diff.new_history_rows.is_empty() {
        section(&mut s, "MIGRATION HISTORY");
        s.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS \"{HISTORY_TABLE}\" (\n\
             \x20   \"MigrationId\" TEXT NOT NULL CONSTRAINT \"PK_{HISTORY_TABLE}\" PRIMARY KEY,\n\
             \x20   \"ProductVersion\" TEXT NOT NULL\n);\n"
        ));
        for (migration_id, product_version) in &diff.new_history_rows {
            s.push_str(&format!(
                "INSERT OR IGNORE INTO \"{HISTORY_TABLE}\" (\"MigrationId\", \"ProductVersion\") \
                 VALUES ('{}', '{}');\n",
                escape(migration_id),
                escape(product_version)
            ));
        }
        s.push('\n');
    }

    format!("{}\n", s.trim_end())
}

fn column_ddl(c: &ColumnInfo) -> String {
    let mut s = format!("\"{}\" {}", c.name, c.type_);
    if let Some(d) = &c.default {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    if c.not_null {
        s.push_str(" NOT NULL");
    }
    s
}

/// Inject `IF NOT EXISTS` after `keyword` if not already present.
fn inject_if_not_exists(sql: &str, keyword: &str) -> String {
    let trimmed = sql.trim_start();
    if trimmed.contains("IF NOT EXISTS") {
        return sql.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix(keyword) {
        format!("{keyword} IF NOT EXISTS{rest}")
    } else {
        sql.to_string()
    }
}

fn ensure_semicolon(sql: &str) -> String {
    let sql = sql.trim_end();
    if sql.ends_with(';') {
        sql.to_string()
    } else {
        format!("{sql};")
    }
}

fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

fn section(s: &mut String, title: &str) {
    s.push_str(&format!("-- ===== {title} =====\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_if_not_exists_once() {
        assert_eq!(
            inject_if_not_exists("CREATE TABLE \"X\" (a INT)", "CREATE TABLE"),
            "CREATE TABLE IF NOT EXISTS \"X\" (a INT)"
        );
        // Already present → unchanged.
        assert_eq!(
            inject_if_not_exists("CREATE TABLE IF NOT EXISTS \"X\" (a INT)", "CREATE TABLE"),
            "CREATE TABLE IF NOT EXISTS \"X\" (a INT)"
        );
    }

    #[test]
    fn new_table_uses_stored_create_sql() {
        let mut diff = DiffResult::default();
        diff.new_tables.push(TableInfo {
            name: "Note".into(),
            columns: vec![],
            create_sql: Some("CREATE TABLE \"Note\" (\"Id\" INTEGER PRIMARY KEY)".into()),
        });
        let sql = build_ddl(&diff);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"Note\""));
    }
}
