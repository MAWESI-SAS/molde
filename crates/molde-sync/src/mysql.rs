//! MySQL engine for `sync`: reads the live structure from `information_schema` +
//! `SHOW CREATE TABLE`, emits additive DDL, and applies it.
//!
//! MySQL keeps indexes and constraints **inline** in each table's `CREATE` text
//! and its DDL is **not transactional**, so this engine targets the common
//! additive cases: new tables (emitted verbatim from `SHOW CREATE TABLE`, with
//! `FOREIGN_KEY_CHECKS` disabled so inline FKs don't depend on creation order),
//! new columns, new views, and migration history. Separate sync of indexes,
//! constraints, functions, procedures and triggers on existing tables is not yet
//! covered (those need delimiter-aware, order-aware handling).

use anyhow::{Context, Result};
use sqlx::mysql::MySqlRow;
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};

/// Read a column as `String`, tolerating MySQL's `information_schema` returning
/// text columns with a binary collation (VARBINARY), which sqlx won't decode
/// straight into `String`.
fn mstr(row: &MySqlRow, idx: usize) -> String {
    row.try_get::<String, _>(idx).unwrap_or_else(|_| {
        row.try_get::<Vec<u8>, _>(idx)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    })
}

/// Same as [`mstr`] but for a nullable column.
fn mopt(row: &MySqlRow, idx: usize) -> Option<String> {
    if let Ok(v) = row.try_get::<Option<String>, _>(idx) {
        return v;
    }
    row.try_get::<Option<Vec<u8>>, _>(idx)
        .ok()
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

use crate::engine::SyncEngine;
use crate::schema::{ColumnInfo, DbSchema, DiffResult, TableInfo, ViewInfo};

const HISTORY_TABLE: &str = "__EFMigrationsHistory";

pub struct MysqlEngine;

#[async_trait::async_trait]
impl SyncEngine for MysqlEngine {
    fn name(&self) -> &'static str {
        "mysql"
    }

    async fn read_schema(&self, conn: &str) -> Result<DbSchema> {
        let pool = connect(conn).await?;
        let mut schema = DbSchema::default();
        read_tables(&pool, &mut schema).await?;
        read_columns(&pool, &mut schema).await?;
        read_views(&pool, &mut schema).await?;
        read_history(&pool, &mut schema).await?;
        pool.close().await;
        Ok(schema)
    }

    fn write_ddl(&self, diff: &DiffResult) -> String {
        build_ddl(diff)
    }

    async fn apply(&self, conn: &str, body: &str) -> Result<()> {
        // MySQL DDL is not transactional, so a BEGIN/COMMIT wrapper would not make
        // this atomic; the body already toggles FOREIGN_KEY_CHECKS itself.
        let pool = connect(conn).await?;
        let result = sqlx::raw_sql(body)
            .execute(&pool)
            .await
            .context("applying the sync script");
        pool.close().await;
        result.map(|_| ())
    }

    fn redact(&self, conn: &str) -> String {
        redact_url(conn)
    }
}

async fn connect(conn: &str) -> Result<MySqlPool> {
    MySqlPoolOptions::new()
        .max_connections(1)
        .connect(conn)
        .await
        .context("connecting to the database")
}

// ---- Readers ----

async fn read_tables(pool: &MySqlPool, schema: &mut DbSchema) -> Result<()> {
    let names: Vec<String> = sqlx::query(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' \
           AND table_name <> '__EFMigrationsHistory' ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .context("reading tables")?
    .iter()
    .map(|r| mstr(r, 0))
    .collect();

    for name in names {
        let row = sqlx::query(&format!("SHOW CREATE TABLE `{}`", name.replace('`', "``")))
            .fetch_one(pool)
            .await
            .with_context(|| format!("reading CREATE of {name}"))?;
        // SHOW CREATE TABLE returns (Table, `Create Table`).
        let create_sql = mstr(&row, 1);
        schema.tables.insert(
            name.clone(),
            TableInfo {
                name,
                columns: Vec::new(),
                create_sql: Some(create_sql),
            },
        );
    }
    Ok(())
}

async fn read_columns(pool: &MySqlPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "SELECT table_name, column_name, column_type, is_nullable, column_default, extra \
               FROM information_schema.columns \
               WHERE table_schema = DATABASE() AND table_name <> '__EFMigrationsHistory' \
               ORDER BY table_name, ordinal_position";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading columns")?
    {
        let table = mstr(&row, 0);
        let is_nullable = mstr(&row, 3);
        let extra = mstr(&row, 5);
        let column = ColumnInfo {
            name: mstr(&row, 1),
            type_: mstr(&row, 2),
            not_null: is_nullable.eq_ignore_ascii_case("NO"),
            default: mopt(&row, 4),
            is_generated: extra.to_uppercase().contains("GENERATED"),
        };
        if let Some(t) = schema.tables.get_mut(&table) {
            t.columns.push(column);
        }
    }
    Ok(())
}

async fn read_views(pool: &MySqlPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "SELECT table_name, view_definition, table_schema FROM information_schema.views \
               WHERE table_schema = DATABASE() ORDER BY table_name";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading views")?
    {
        // MySQL fully-qualifies the view body with the source database
        // (`` `src`.`Customer` ``). Strip that qualifier so the view resolves in
        // whatever database it is created in (the target), not the source.
        let db = mstr(&row, 2);
        let definition = mstr(&row, 1).replace(&format!("`{db}`."), "");
        let info = ViewInfo {
            name: mstr(&row, 0),
            definition,
        };
        schema.views.insert(info.name.clone(), info);
    }
    Ok(())
}

async fn read_history(pool: &MySqlPool, schema: &mut DbSchema) -> Result<()> {
    let exists = sqlx::query(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_name = '__EFMigrationsHistory'",
    )
    .fetch_optional(pool)
    .await
    .context("checking the migration history table")?;
    if exists.is_none() {
        return Ok(());
    }
    for row in sqlx::query(
        "SELECT `MigrationId`, `ProductVersion` FROM `__EFMigrationsHistory` ORDER BY `MigrationId`",
    )
    .fetch_all(pool)
    .await
    .context("reading migration history")?
    {
        schema
            .migration_history
            .insert(mstr(&row, 0), mstr(&row, 1));
    }
    Ok(())
}

// ---- DDL writer ----

fn build_ddl(diff: &DiffResult) -> String {
    let mut s = String::new();

    let needs_fk_guard = !diff.new_tables.is_empty();
    if needs_fk_guard {
        s.push_str("SET FOREIGN_KEY_CHECKS = 0;\n\n");
    }

    if !diff.new_tables.is_empty() {
        section(&mut s, "NEW TABLES");
        for table in &diff.new_tables {
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
                "ALTER TABLE `{table}` ADD COLUMN {};\n",
                column_ddl(col)
            ));
        }
        s.push('\n');
    }

    if !diff.new_views.is_empty() {
        section(&mut s, "VIEWS");
        for view in &diff.new_views {
            s.push_str(&format!(
                "CREATE OR REPLACE VIEW `{}` AS {};\n",
                view.name,
                view.definition.trim_end().trim_end_matches(';')
            ));
        }
        s.push('\n');
    }

    if !diff.new_history_rows.is_empty() {
        section(&mut s, "MIGRATION HISTORY");
        s.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS `{HISTORY_TABLE}` (\n\
             \x20   `MigrationId` varchar(150) NOT NULL,\n\
             \x20   `ProductVersion` varchar(32) NOT NULL,\n\
             \x20   PRIMARY KEY (`MigrationId`)\n);\n"
        ));
        for (migration_id, product_version) in &diff.new_history_rows {
            s.push_str(&format!(
                "INSERT IGNORE INTO `{HISTORY_TABLE}` (`MigrationId`, `ProductVersion`) \
                 VALUES ('{}', '{}');\n",
                escape(migration_id),
                escape(product_version)
            ));
        }
        s.push('\n');
    }

    if needs_fk_guard {
        s.push_str("SET FOREIGN_KEY_CHECKS = 1;\n");
    }

    format!("{}\n", s.trim_end())
}

fn column_ddl(c: &ColumnInfo) -> String {
    let mut s = format!("`{}` {}", c.name, c.type_);
    if c.not_null {
        s.push_str(" NOT NULL");
    }
    if let Some(d) = &c.default {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    s
}

fn inject_if_not_exists(sql: &str, keyword: &str) -> String {
    let trimmed = sql.trim_start();
    if trimmed.to_uppercase().contains("IF NOT EXISTS") {
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

fn redact_url(conn: &str) -> String {
    if let Some((scheme, rest)) = conn.split_once("://") {
        if let Some((creds, tail)) = rest.split_once('@') {
            let user = creds.split(':').next().unwrap_or(creds);
            let hostdb = tail.split('?').next().unwrap_or(tail);
            return format!("{scheme}://{user}:***@{hostdb}");
        }
        let hostdb = rest.split('?').next().unwrap_or(rest);
        return format!("{scheme}://{hostdb}");
    }
    conn.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_keeps_create_and_guards_fks() {
        let mut diff = DiffResult::default();
        diff.new_tables.push(TableInfo {
            name: "t".into(),
            columns: vec![],
            create_sql: Some("CREATE TABLE `t` (`id` int NOT NULL)".into()),
        });
        let sql = build_ddl(&diff);
        assert!(sql.starts_with("SET FOREIGN_KEY_CHECKS = 0;"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS `t`"));
        assert!(sql.trim_end().ends_with("SET FOREIGN_KEY_CHECKS = 1;"));
    }
}
