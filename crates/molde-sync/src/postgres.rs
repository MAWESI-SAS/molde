//! PostgreSQL engine for `sync`: reads the live structure from `pg_catalog`,
//! emits additive DDL, and applies it atomically. Everything is read from the
//! catalog (not from any model or migration files), so it reflects the live
//! database regardless of how migrations were run.

use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

use crate::engine::SyncEngine;
use crate::schema::{
    ColumnInfo, ConstraintInfo, DbSchema, DiffResult, IndexInfo, RoutineInfo, TableInfo,
    TriggerInfo, ViewInfo,
};

const HISTORY_TABLE: &str = "__EFMigrationsHistory";

pub struct PostgresEngine;

#[async_trait::async_trait]
impl SyncEngine for PostgresEngine {
    fn name(&self) -> &'static str {
        "postgres"
    }

    async fn read_schema(&self, conn: &str) -> Result<DbSchema> {
        let pool = connect(conn).await?;
        let mut schema = DbSchema::default();
        read_extensions(&pool, &mut schema).await?;
        read_columns(&pool, &mut schema).await?;
        read_constraints(&pool, &mut schema).await?;
        read_indexes(&pool, &mut schema).await?;
        read_functions(&pool, &mut schema).await?;
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
        // Wrapped in an explicit transaction so the whole script is atomic
        // (all-or-nothing) when applied via the simple query protocol.
        let script = format!("BEGIN;\n{body}\nCOMMIT;");
        let result = sqlx::raw_sql(&script)
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

async fn connect(conn: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(1)
        .connect(conn)
        .await
        .context("connecting to the database")
}

// ---- Readers (mirror the pg_catalog queries of the reference tool) ----

async fn read_extensions(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    // The default `plpgsql` is always present; everything else may need creating
    // on the target before the functions/objects that depend on it.
    let sql = "SELECT extname FROM pg_extension WHERE extname <> 'plpgsql' ORDER BY 1";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading extensions")?
    {
        schema.extensions.insert(row.get(0));
    }
    Ok(())
}

async fn read_columns(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "
        SELECT c.relname, a.attname,
               format_type(a.atttypid, a.atttypmod),
               a.attnotnull,
               pg_get_expr(ad.adbin, ad.adrelid),
               a.attgenerated::text
        FROM pg_attribute a
        JOIN pg_class c ON c.oid = a.attrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
        WHERE n.nspname = 'public' AND c.relkind = 'r'
          AND a.attnum > 0 AND NOT a.attisdropped
          AND c.relname <> '__EFMigrationsHistory'
        ORDER BY c.relname, a.attnum";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading columns")?
    {
        let table: String = row.get(0);
        let generated: Option<String> = row.get(5);
        let column = ColumnInfo {
            name: row.get(1),
            type_: row.get(2),
            not_null: row.get(3),
            default: row.get(4),
            is_generated: generated.as_deref() == Some("s"),
        };
        schema
            .tables
            .entry(table.clone())
            .or_insert_with(|| TableInfo {
                name: table,
                columns: Vec::new(),
            })
            .columns
            .push(column);
    }
    Ok(())
}

async fn read_constraints(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "
        SELECT t.relname, c.conname, c.contype::text, pg_get_constraintdef(c.oid)
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = 'public' AND c.conrelid <> 0
          AND t.relname <> '__EFMigrationsHistory'
        ORDER BY 1, 2";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading constraints")?
    {
        let kind: String = row.get(2);
        let info = ConstraintInfo {
            table: row.get(0),
            name: row.get(1),
            kind: kind.chars().next().unwrap_or('?'),
            definition: row.get(3),
        };
        schema.constraints.insert(info.key(), info);
    }
    Ok(())
}

async fn read_indexes(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    // Excludes indexes that back a constraint (those are emitted via the constraint).
    let sql = "
        SELECT ic.relname, tc.relname, pg_get_indexdef(ix.indexrelid)
        FROM pg_index ix
        JOIN pg_class ic ON ic.oid = ix.indexrelid
        JOIN pg_class tc ON tc.oid = ix.indrelid
        JOIN pg_namespace n ON n.oid = ic.relnamespace
        WHERE n.nspname = 'public'
          AND tc.relname <> '__EFMigrationsHistory'
          AND NOT EXISTS (SELECT 1 FROM pg_constraint con WHERE con.conindid = ix.indexrelid)
        ORDER BY 1";
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

async fn read_functions(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    // Skips functions owned by an extension (deptype 'e').
    let sql = "
        SELECT p.proname,
               pg_get_function_identity_arguments(p.oid),
               pg_get_functiondef(p.oid)
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public' AND p.prokind IN ('f', 'p')
          AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e')
        ORDER BY 1, 2";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading functions")?
    {
        let info = RoutineInfo {
            name: row.get(0),
            arguments: row.get(1),
            definition: row.get(2),
        };
        schema.functions.insert(info.key(), info);
    }
    Ok(())
}

async fn read_triggers(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "
        SELECT t.relname, tg.tgname, pg_get_triggerdef(tg.oid)
        FROM pg_trigger tg
        JOIN pg_class t ON t.oid = tg.tgrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = 'public' AND NOT tg.tgisinternal
        ORDER BY 1, 2";
    for row in sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("reading triggers")?
    {
        let info = TriggerInfo {
            table: row.get(0),
            name: row.get(1),
            definition: row.get(2),
        };
        schema.triggers.insert(info.key(), info);
    }
    Ok(())
}

async fn read_views(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    let sql = "
        SELECT viewname, pg_get_viewdef(viewname::regclass, true)
        FROM pg_views WHERE schemaname = 'public'
        ORDER BY 1";
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

async fn read_history(pool: &PgPool, schema: &mut DbSchema) -> Result<()> {
    // No __EFMigrationsHistory yet → treat as empty history.
    let exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.\"__EFMigrationsHistory\"')::text")
            .fetch_one(pool)
            .await
            .context("checking the migration history table")?;
    if exists.is_none() {
        return Ok(());
    }
    let sql = format!(
        "SELECT \"MigrationId\", \"ProductVersion\" FROM \"{HISTORY_TABLE}\" ORDER BY \"MigrationId\""
    );
    for row in sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .context("reading migration history")?
    {
        schema.migration_history.insert(row.get(0), row.get(1));
    }
    Ok(())
}

// ---- DDL writer (additive, dependency-ordered, idempotent) ----

fn build_ddl(diff: &DiffResult) -> String {
    let mut s = String::new();

    if !diff.new_extensions.is_empty() {
        section(&mut s, "EXTENSIONS");
        for ext in &diff.new_extensions {
            s.push_str(&format!("CREATE EXTENSION IF NOT EXISTS \"{ext}\";\n"));
        }
        s.push('\n');
    }

    if !diff.new_functions.is_empty() {
        section(&mut s, "FUNCTIONS");
        for fn_ in &diff.new_functions {
            s.push_str(&ensure_semicolon(&fn_.definition));
            s.push_str("\n\n");
        }
    }

    if !diff.new_tables.is_empty() {
        section(&mut s, "NEW TABLES");
        for table in &diff.new_tables {
            s.push_str(&format!(
                "CREATE TABLE IF NOT EXISTS \"{}\" (\n",
                table.name
            ));
            for (i, col) in table.columns.iter().enumerate() {
                let comma = if i < table.columns.len() - 1 { "," } else { "" };
                s.push_str(&format!("    {}{comma}\n", column_ddl(col)));
            }
            s.push_str(");\n\n");
        }
    }

    if !diff.new_columns.is_empty() {
        section(&mut s, "NEW COLUMNS");
        for (table, col) in &diff.new_columns {
            s.push_str(&format!(
                "ALTER TABLE \"{table}\" ADD COLUMN IF NOT EXISTS {};\n",
                column_ddl(col)
            ));
        }
        s.push('\n');
    }

    if !diff.new_constraints.is_empty() {
        section(&mut s, "CONSTRAINTS");
        let mut constraints: Vec<&ConstraintInfo> = diff.new_constraints.iter().collect();
        constraints.sort_by_key(|c| constraint_rank(c.kind));
        for c in constraints {
            s.push_str(&format!(
                "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" {};\n",
                c.table, c.name, c.definition
            ));
        }
        s.push('\n');
    }

    if !diff.new_indexes.is_empty() {
        section(&mut s, "INDEXES");
        for idx in &diff.new_indexes {
            s.push_str(&ensure_semicolon(&with_index_if_not_exists(
                &idx.definition,
            )));
            s.push('\n');
        }
        s.push('\n');
    }

    if !diff.new_triggers.is_empty() {
        section(&mut s, "TRIGGERS");
        for trg in &diff.new_triggers {
            s.push_str(&ensure_semicolon(&trg.definition));
            s.push('\n');
        }
        s.push('\n');
    }

    if !diff.new_views.is_empty() {
        section(&mut s, "VIEWS");
        for view in &diff.new_views {
            s.push_str(&format!("CREATE OR REPLACE VIEW \"{}\" AS\n", view.name));
            s.push_str(&ensure_semicolon(view.definition.trim_end()));
            s.push_str("\n\n");
        }
    }

    if !diff.new_history_rows.is_empty() {
        section(&mut s, "MIGRATION HISTORY");
        s.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS \"{HISTORY_TABLE}\" (\n"
        ));
        s.push_str("    \"MigrationId\" character varying(150) NOT NULL,\n");
        s.push_str("    \"ProductVersion\" character varying(32) NOT NULL,\n");
        s.push_str(&format!(
            "    CONSTRAINT \"PK_{HISTORY_TABLE}\" PRIMARY KEY (\"MigrationId\")\n"
        ));
        s.push_str(");\n");
        for (migration_id, product_version) in &diff.new_history_rows {
            s.push_str(&format!(
                "INSERT INTO \"{HISTORY_TABLE}\" (\"MigrationId\", \"ProductVersion\") \
                 VALUES ('{}', '{}') ON CONFLICT (\"MigrationId\") DO NOTHING;\n",
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
    match (&c.default, c.is_generated) {
        (Some(expr), true) => s.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED")),
        (Some(expr), false) => s.push_str(&format!(" DEFAULT {expr}")),
        (None, _) => {}
    }
    if c.not_null {
        s.push_str(" NOT NULL");
    }
    s
}

fn constraint_rank(kind: char) -> u8 {
    match kind {
        'p' => 0, // primary key
        'u' => 1, // unique
        'c' => 2, // check
        'f' => 3, // foreign key — last, referenced keys must exist first
        _ => 4,
    }
}

fn with_index_if_not_exists(def: &str) -> String {
    if let Some(rest) = def.strip_prefix("CREATE UNIQUE INDEX ") {
        format!("CREATE UNIQUE INDEX IF NOT EXISTS {rest}")
    } else if let Some(rest) = def.strip_prefix("CREATE INDEX ") {
        format!("CREATE INDEX IF NOT EXISTS {rest}")
    } else {
        def.to_string()
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

/// Postgres connection URL with the password masked.
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
    use crate::schema::TableInfo;

    #[test]
    fn ddl_orders_and_makes_idempotent() {
        let mut diff = DiffResult::default();
        diff.new_tables.push(TableInfo {
            name: "Invoice".into(),
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_: "uuid".into(),
                not_null: true,
                default: Some("gen_random_uuid()".into()),
                is_generated: false,
            }],
        });
        diff.new_indexes.push(IndexInfo {
            name: "ix_invoice".into(),
            table: "Invoice".into(),
            definition: "CREATE INDEX ix_invoice ON public.\"Invoice\" (id)".into(),
        });
        let sql = build_ddl(&diff);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"Invoice\""));
        assert!(sql.contains("\"id\" uuid DEFAULT gen_random_uuid() NOT NULL"));
        assert!(sql.contains("CREATE INDEX IF NOT EXISTS ix_invoice"));
        // Tables come before indexes.
        assert!(sql.find("NEW TABLES").unwrap() < sql.find("INDEXES").unwrap());
    }

    #[test]
    fn redact_hides_password() {
        assert_eq!(
            redact_url("postgres://user:secret@host:5432/db?sslmode=require"),
            "postgres://user:***@host:5432/db"
        );
    }
}
