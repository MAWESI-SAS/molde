//! SQL Server engine for `sync`: reads the live structure from `sys.*` /
//! `INFORMATION_SCHEMA` (over TDS via tiberius), reconstructs additive DDL, and
//! applies it atomically. Everything is read from the catalog (not from any model
//! or migration files), so it reflects the live database.
//!
//! Two SQL Server specifics shape the emitted DDL:
//!
//! * `CREATE VIEW/FUNCTION/PROCEDURE/TRIGGER` must be the first statement of its
//!   batch, so they cannot be wrapped in `IF … BEGIN … END`. Instead they are run
//!   as dynamic SQL (`EXEC sp_executesql @sql`) guarded by an `IF OBJECT_ID(...)
//!   IS NULL`. The definition text is split into `N'…'` literals of ≤2000 chars
//!   and concatenated, because a single string literal longer than 4000 chars is
//!   silently truncated.
//! * DDL *is* transactional here (unlike MySQL), so [`apply`](SqlServerEngine::apply)
//!   wraps the whole script in `SET XACT_ABORT ON; BEGIN TRANSACTION … COMMIT`.
//!
//! The engine targets the `dbo` schema. Reconstructing types/identity/computed
//! columns mirrors the SQL Server path of `molde-scaffold`.

use anyhow::{Context, Result};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::engine::SyncEngine;
use crate::schema::{
    ColumnInfo, ConstraintInfo, DbSchema, DiffResult, IndexInfo, RoutineInfo, TableInfo,
    TriggerInfo, ViewInfo,
};

const HISTORY_TABLE: &str = "__EFMigrationsHistory";
const SCHEMA: &str = "dbo";

type SsClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

pub struct SqlServerEngine;

#[async_trait::async_trait]
impl SyncEngine for SqlServerEngine {
    fn name(&self) -> &'static str {
        "sqlserver"
    }

    async fn read_schema(&self, conn: &str) -> Result<DbSchema> {
        let mut client = connect(conn).await?;
        let mut schema = DbSchema::default();
        read_columns(&mut client, &mut schema).await?;
        read_constraints(&mut client, &mut schema).await?;
        read_indexes(&mut client, &mut schema).await?;
        read_functions(&mut client, &mut schema).await?;
        read_triggers(&mut client, &mut schema).await?;
        read_views(&mut client, &mut schema).await?;
        read_history(&mut client, &mut schema).await?;
        Ok(schema)
    }

    fn write_ddl(&self, diff: &DiffResult) -> String {
        build_ddl(diff)
    }

    async fn apply(&self, conn: &str, body: &str) -> Result<()> {
        let mut client = connect(conn).await?;
        // SQL Server DDL is transactional. XACT_ABORT guarantees the whole
        // script rolls back on any error in the batch.
        let script = format!("SET XACT_ABORT ON;\nBEGIN TRANSACTION;\n{body}\nCOMMIT;");
        client
            .simple_query(script)
            .await
            .context("applying the sync script")?
            .into_results()
            .await
            .context("applying the sync script")?;
        Ok(())
    }

    fn redact(&self, conn: &str) -> String {
        redact_ado(conn)
    }

    fn wrap_script(&self, body: &str) -> String {
        format!("SET XACT_ABORT ON;\nBEGIN TRANSACTION;\n\n{body}\nCOMMIT;\n")
    }
}

async fn connect(conn: &str) -> Result<SsClient> {
    let config = tiberius::Config::from_ado_string(conn)
        .context("parsing the SQL Server ADO connection string")?;
    let tcp = tokio::net::TcpStream::connect(config.get_addr())
        .await
        .context("connecting to SQL Server")?;
    tcp.set_nodelay(true).ok();
    tiberius::Client::connect(config, tcp.compat_write())
        .await
        .context("opening the SQL Server session")
}

async fn ss_query(client: &mut SsClient, sql: &str) -> Result<Vec<tiberius::Row>> {
    Ok(client
        .simple_query(sql.to_string())
        .await
        .with_context(|| format!("querying: {}", sql.lines().next().unwrap_or("")))?
        .into_first_result()
        .await?)
}

fn s(row: &tiberius::Row, col: &str) -> String {
    row.get::<&str, _>(col).unwrap_or("").to_string()
}

// ---- Readers ----

async fn read_columns(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    // User tables (excluding system tables and the history table).
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name FROM sys.tables t \
             WHERE t.is_ms_shipped = 0 AND t.name <> '{HISTORY_TABLE}' \
               AND SCHEMA_NAME(t.schema_id) = '{SCHEMA}' ORDER BY t.name"
        ),
    )
    .await?
    {
        let name = s(&r, "table_name");
        schema.tables.insert(
            name.clone(),
            TableInfo {
                name,
                columns: Vec::new(),
                create_sql: None,
            },
        );
    }

    // Identity columns (seed/increment) keyed by `table|column`.
    let mut identity: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name, c.name AS column_name, \
                    CAST(ic.seed_value AS bigint) AS seed, \
                    CAST(ic.increment_value AS bigint) AS incr \
             FROM sys.identity_columns ic \
             JOIN sys.tables t ON t.object_id = ic.object_id \
             JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
             WHERE SCHEMA_NAME(t.schema_id) = '{SCHEMA}'"
        ),
    )
    .await?
    {
        let key = format!("{}|{}", s(&r, "table_name"), s(&r, "column_name"));
        let seed = r.get::<i64, _>("seed").unwrap_or(1);
        let incr = r.get::<i64, _>("incr").unwrap_or(1);
        identity.insert(key, (seed, incr));
    }

    // Computed columns (definition + whether PERSISTED) keyed by `table|column`.
    let mut computed: std::collections::BTreeMap<String, (String, bool)> = Default::default();
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name, c.name AS column_name, \
                    cc.definition AS definition, CAST(cc.is_persisted AS int) AS is_persisted \
             FROM sys.computed_columns cc \
             JOIN sys.tables t ON t.object_id = cc.object_id \
             JOIN sys.columns c ON c.object_id = cc.object_id AND c.column_id = cc.column_id \
             WHERE SCHEMA_NAME(t.schema_id) = '{SCHEMA}'"
        ),
    )
    .await?
    {
        let key = format!("{}|{}", s(&r, "table_name"), s(&r, "column_name"));
        let persisted = r.get::<i32, _>("is_persisted").unwrap_or(0) == 1;
        computed.insert(key, (s(&r, "definition"), persisted));
    }

    // Columns (in ordinal order). INFORMATION_SCHEMA.COLUMNS also lists view
    // columns, so we only keep those for tables we know about.
    for r in ss_query(
        client,
        &format!(
            "SELECT c.TABLE_NAME AS table_name, c.COLUMN_NAME AS column_name, \
                    c.DATA_TYPE AS data_type, c.IS_NULLABLE AS is_nullable, \
                    CAST(c.CHARACTER_MAXIMUM_LENGTH AS int) AS max_len, \
                    CAST(c.NUMERIC_PRECISION AS int) AS num_prec, \
                    CAST(c.NUMERIC_SCALE AS int) AS num_scale, \
                    c.COLUMN_DEFAULT AS column_default \
             FROM INFORMATION_SCHEMA.COLUMNS c WHERE c.TABLE_SCHEMA = '{SCHEMA}' \
             ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"
        ),
    )
    .await?
    {
        let table = s(&r, "table_name");
        let Some(t) = schema.tables.get_mut(&table) else {
            continue;
        };
        let col_name = s(&r, "column_name");
        let key = format!("{table}|{col_name}");

        if let Some((definition, _persisted)) = computed.get(&key) {
            // Computed column: store the expression as the "default" and mark it
            // generated, mirroring the Postgres engine's generated columns.
            t.columns.push(ColumnInfo {
                name: col_name,
                type_: String::new(),
                not_null: false,
                default: Some(definition.clone()),
                is_generated: true,
            });
            continue;
        }

        let data_type = s(&r, "data_type");
        let max_len = r.get::<i32, _>("max_len");
        let prec = r.get::<i32, _>("num_prec");
        let scale = r.get::<i32, _>("num_scale");
        let mut type_ = store_type(&data_type, max_len, prec, scale);
        if let Some((seed, incr)) = identity.get(&key) {
            type_.push_str(&format!(" IDENTITY({seed},{incr})"));
        }
        let default = r
            .get::<&str, _>("column_default")
            .map(|d| d.to_string())
            .filter(|d| !d.is_empty());
        t.columns.push(ColumnInfo {
            name: col_name,
            type_,
            not_null: !s(&r, "is_nullable").eq_ignore_ascii_case("YES"),
            default,
            is_generated: false,
        });
    }
    Ok(())
}

async fn read_constraints(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    // Primary keys and unique constraints (sys.key_constraints).
    let mut keys: std::collections::BTreeMap<String, (String, char, Vec<String>)> =
        Default::default();
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name, kc.name AS constraint_name, kc.type AS ctype, \
                    c.name AS column_name \
             FROM sys.key_constraints kc \
             JOIN sys.tables t ON t.object_id = kc.parent_object_id \
             JOIN sys.index_columns ic ON ic.object_id = kc.parent_object_id \
                  AND ic.index_id = kc.unique_index_id \
             JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
             WHERE SCHEMA_NAME(t.schema_id) = '{SCHEMA}' AND t.name <> '{HISTORY_TABLE}' \
             ORDER BY t.name, kc.name, ic.key_ordinal"
        ),
    )
    .await?
    {
        let name = s(&r, "constraint_name");
        let kind = if s(&r, "ctype").trim() == "PK" {
            'p'
        } else {
            'u'
        };
        keys.entry(name)
            .or_insert_with(|| (s(&r, "table_name"), kind, Vec::new()))
            .2
            .push(s(&r, "column_name"));
    }
    for (name, (table, kind, cols)) in keys {
        let kw = if kind == 'p' { "PRIMARY KEY" } else { "UNIQUE" };
        let info = ConstraintInfo {
            table,
            name,
            kind,
            definition: format!("{kw} ({})", join_cols(&cols)),
        };
        schema.constraints.insert(info.key(), info);
    }

    // Check constraints (sys.check_constraints).
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name, cc.name AS constraint_name, cc.definition AS definition \
             FROM sys.check_constraints cc \
             JOIN sys.tables t ON t.object_id = cc.parent_object_id \
             WHERE SCHEMA_NAME(t.schema_id) = '{SCHEMA}' AND t.name <> '{HISTORY_TABLE}'"
        ),
    )
    .await?
    {
        let info = ConstraintInfo {
            table: s(&r, "table_name"),
            name: s(&r, "constraint_name"),
            kind: 'c',
            definition: format!("CHECK {}", s(&r, "definition")),
        };
        schema.constraints.insert(info.key(), info);
    }

    // Foreign keys (sys.foreign_keys).
    let mut fks: std::collections::BTreeMap<String, FkBuild> = Default::default();
    for r in ss_query(
        client,
        &format!(
            "SELECT fk.name AS fk_name, tp.name AS table_name, cp.name AS column_name, \
                    tr.name AS ref_table, cr.name AS ref_column, \
                    fk.delete_referential_action_desc AS del, \
                    fk.update_referential_action_desc AS upd \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.tables tp ON tp.object_id = fk.parent_object_id \
             JOIN sys.columns cp ON cp.object_id = tp.object_id AND cp.column_id = fkc.parent_column_id \
             JOIN sys.tables tr ON tr.object_id = fk.referenced_object_id \
             JOIN sys.columns cr ON cr.object_id = tr.object_id AND cr.column_id = fkc.referenced_column_id \
             WHERE SCHEMA_NAME(tp.schema_id) = '{SCHEMA}' \
             ORDER BY fk.name, fkc.constraint_column_id"
        ),
    )
    .await?
    {
        let name = s(&r, "fk_name");
        let fk = fks.entry(name).or_insert_with(|| FkBuild {
            table: s(&r, "table_name"),
            ref_table: s(&r, "ref_table"),
            cols: Vec::new(),
            ref_cols: Vec::new(),
            on_delete: s(&r, "del"),
            on_update: s(&r, "upd"),
        });
        fk.cols.push(s(&r, "column_name"));
        fk.ref_cols.push(s(&r, "ref_column"));
    }
    for (name, fk) in fks {
        let mut def = format!(
            "FOREIGN KEY ({}) REFERENCES [{SCHEMA}].[{}] ({})",
            join_cols(&fk.cols),
            fk.ref_table,
            join_cols(&fk.ref_cols),
        );
        if let Some(action) = referential_action(&fk.on_delete) {
            def.push_str(&format!(" ON DELETE {action}"));
        }
        if let Some(action) = referential_action(&fk.on_update) {
            def.push_str(&format!(" ON UPDATE {action}"));
        }
        let info = ConstraintInfo {
            table: fk.table,
            name,
            kind: 'f',
            definition: def,
        };
        schema.constraints.insert(info.key(), info);
    }
    Ok(())
}

struct FkBuild {
    table: String,
    ref_table: String,
    cols: Vec<String>,
    ref_cols: Vec<String>,
    on_delete: String,
    on_update: String,
}

async fn read_indexes(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    // Indexes that don't back a PK/unique constraint (those ride on the constraint).
    let mut idx: std::collections::BTreeMap<String, (String, bool, Vec<String>)> =
        Default::default();
    for r in ss_query(
        client,
        &format!(
            "SELECT t.name AS table_name, i.name AS index_name, \
                    CAST(i.is_unique AS int) AS is_unique, \
                    c.name AS column_name, CAST(ic.is_descending_key AS int) AS is_desc \
             FROM sys.indexes i \
             JOIN sys.tables t ON t.object_id = i.object_id \
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns c ON c.object_id = i.object_id AND c.column_id = ic.column_id \
             WHERE i.is_primary_key = 0 AND i.is_unique_constraint = 0 AND i.name IS NOT NULL \
               AND ic.is_included_column = 0 AND t.is_ms_shipped = 0 \
               AND t.name <> '{HISTORY_TABLE}' AND SCHEMA_NAME(t.schema_id) = '{SCHEMA}' \
             ORDER BY t.name, i.name, ic.key_ordinal"
        ),
    )
    .await?
    {
        let table = s(&r, "table_name");
        let name = s(&r, "index_name");
        let unique = r.get::<i32, _>("is_unique").unwrap_or(0) == 1;
        let desc = r.get::<i32, _>("is_desc").unwrap_or(0) == 1;
        let col = format!(
            "[{}]{}",
            s(&r, "column_name"),
            if desc { " DESC" } else { "" }
        );
        idx.entry(format!("{table}|{name}"))
            .or_insert_with(|| (table, unique, Vec::new()))
            .2
            .push(col);
    }
    for (key, (table, unique, cols)) in idx {
        let name = key
            .split_once('|')
            .map(|(_, n)| n)
            .unwrap_or(&key)
            .to_string();
        let uq = if unique { "UNIQUE " } else { "" };
        let info = IndexInfo {
            definition: format!(
                "CREATE {uq}INDEX [{name}] ON [{SCHEMA}].[{table}] ({})",
                cols.join(", ")
            ),
            name,
            table,
        };
        schema.indexes.insert(key, info);
    }
    Ok(())
}

async fn read_functions(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    // Scalar/table functions and stored procedures, by their full module text.
    for r in ss_query(
        client,
        &format!(
            "SELECT o.name AS name, OBJECT_DEFINITION(o.object_id) AS definition \
             FROM sys.objects o \
             WHERE o.type IN ('FN','IF','TF','P') AND o.is_ms_shipped = 0 \
               AND SCHEMA_NAME(o.schema_id) = '{SCHEMA}' ORDER BY o.name"
        ),
    )
    .await?
    {
        let definition = s(&r, "definition");
        if definition.is_empty() {
            continue; // encrypted/unavailable module — skip
        }
        let info = RoutineInfo {
            name: s(&r, "name"),
            arguments: String::new(),
            definition,
        };
        schema.functions.insert(info.key(), info);
    }
    Ok(())
}

async fn read_triggers(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    for r in ss_query(
        client,
        &format!(
            "SELECT tr.name AS trigger_name, t.name AS table_name, \
                    OBJECT_DEFINITION(tr.object_id) AS definition \
             FROM sys.triggers tr \
             JOIN sys.tables t ON t.object_id = tr.parent_id \
             WHERE tr.is_ms_shipped = 0 AND SCHEMA_NAME(t.schema_id) = '{SCHEMA}' \
             ORDER BY t.name, tr.name"
        ),
    )
    .await?
    {
        let definition = s(&r, "definition");
        if definition.is_empty() {
            continue;
        }
        let info = TriggerInfo {
            table: s(&r, "table_name"),
            name: s(&r, "trigger_name"),
            definition,
        };
        schema.triggers.insert(info.key(), info);
    }
    Ok(())
}

async fn read_views(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    for r in ss_query(
        client,
        &format!(
            "SELECT v.name AS name, OBJECT_DEFINITION(v.object_id) AS definition \
             FROM sys.views v WHERE SCHEMA_NAME(v.schema_id) = '{SCHEMA}' ORDER BY v.name"
        ),
    )
    .await?
    {
        let definition = s(&r, "definition");
        if definition.is_empty() {
            continue;
        }
        let info = ViewInfo {
            name: s(&r, "name"),
            definition,
        };
        schema.views.insert(info.name.clone(), info);
    }
    Ok(())
}

async fn read_history(client: &mut SsClient, schema: &mut DbSchema) -> Result<()> {
    let exists = ss_query(
        client,
        &format!("SELECT OBJECT_ID(N'[{SCHEMA}].[{HISTORY_TABLE}]', N'U') AS id"),
    )
    .await?;
    let present = exists
        .first()
        .map(|r| r.get::<i32, _>("id").is_some())
        .unwrap_or(false);
    if !present {
        return Ok(());
    }
    for r in ss_query(
        client,
        &format!(
            "SELECT [MigrationId] AS id, [ProductVersion] AS ver \
             FROM [{SCHEMA}].[{HISTORY_TABLE}] ORDER BY [MigrationId]"
        ),
    )
    .await?
    {
        schema.migration_history.insert(s(&r, "id"), s(&r, "ver"));
    }
    Ok(())
}

// ---- DDL writer (additive, dependency-ordered, idempotent) ----

fn build_ddl(diff: &DiffResult) -> String {
    let mut s = String::new();
    let has_modules = !diff.new_functions.is_empty()
        || !diff.new_triggers.is_empty()
        || !diff.new_views.is_empty();
    if has_modules {
        // Reused across every dynamic-SQL module in this single batch.
        s.push_str("DECLARE @sql nvarchar(max);\n\n");
    }

    if !diff.new_tables.is_empty() {
        section(&mut s, "NEW TABLES");
        for table in &diff.new_tables {
            s.push_str(&format!(
                "IF OBJECT_ID(N'[{SCHEMA}].[{}]', N'U') IS NULL\nBEGIN\n    CREATE TABLE [{SCHEMA}].[{}] (\n",
                table.name, table.name
            ));
            for (i, col) in table.columns.iter().enumerate() {
                let comma = if i < table.columns.len() - 1 { "," } else { "" };
                s.push_str(&format!("        {}{comma}\n", column_ddl(col)));
            }
            s.push_str("    );\nEND;\n\n");
        }
    }

    if !diff.new_columns.is_empty() {
        section(&mut s, "NEW COLUMNS");
        for (table, col) in &diff.new_columns {
            s.push_str(&format!(
                "IF COL_LENGTH(N'[{SCHEMA}].[{table}]', N'{}') IS NULL\n    ALTER TABLE [{SCHEMA}].[{table}] ADD {};\n",
                col.name,
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
                "IF NOT EXISTS (SELECT 1 FROM sys.objects WHERE name = N'{}' \
                 AND parent_object_id = OBJECT_ID(N'[{SCHEMA}].[{}]'))\n    \
                 ALTER TABLE [{SCHEMA}].[{}] ADD CONSTRAINT [{}] {};\n",
                c.name, c.table, c.table, c.name, c.definition
            ));
        }
        s.push('\n');
    }

    if !diff.new_indexes.is_empty() {
        section(&mut s, "INDEXES");
        for idx in &diff.new_indexes {
            s.push_str(&format!(
                "IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = N'{}' \
                 AND object_id = OBJECT_ID(N'[{SCHEMA}].[{}]'))\n    {};\n",
                idx.name, idx.table, idx.definition
            ));
        }
        s.push('\n');
    }

    if !diff.new_functions.is_empty() {
        section(&mut s, "FUNCTIONS & PROCEDURES");
        for fn_ in &diff.new_functions {
            push_module(&mut s, &fn_.name, &fn_.definition);
        }
    }

    if !diff.new_triggers.is_empty() {
        section(&mut s, "TRIGGERS");
        for trg in &diff.new_triggers {
            push_module(&mut s, &trg.name, &trg.definition);
        }
    }

    if !diff.new_views.is_empty() {
        section(&mut s, "VIEWS");
        for view in &diff.new_views {
            push_module(&mut s, &view.name, &view.definition);
        }
    }

    if !diff.new_history_rows.is_empty() {
        section(&mut s, "MIGRATION HISTORY");
        s.push_str(&format!(
            "IF OBJECT_ID(N'[{SCHEMA}].[{HISTORY_TABLE}]', N'U') IS NULL\nBEGIN\n    \
             CREATE TABLE [{SCHEMA}].[{HISTORY_TABLE}] (\n        \
             [MigrationId] nvarchar(150) NOT NULL,\n        \
             [ProductVersion] nvarchar(32) NOT NULL,\n        \
             CONSTRAINT [PK_{HISTORY_TABLE}] PRIMARY KEY ([MigrationId])\n    );\nEND;\n"
        ));
        for (migration_id, product_version) in &diff.new_history_rows {
            s.push_str(&format!(
                "IF NOT EXISTS (SELECT 1 FROM [{SCHEMA}].[{HISTORY_TABLE}] WHERE [MigrationId] = N'{}')\n    \
                 INSERT INTO [{SCHEMA}].[{HISTORY_TABLE}] ([MigrationId], [ProductVersion]) \
                 VALUES (N'{}', N'{}');\n",
                escape(migration_id),
                escape(migration_id),
                escape(product_version)
            ));
        }
        s.push('\n');
    }

    format!("{}\n", s.trim_end())
}

/// Emit a module (view/function/trigger) as guarded dynamic SQL. `CREATE` of
/// these must be first in its batch, so it cannot sit inside `IF … BEGIN … END`;
/// instead it runs via `EXEC sp_executesql`, with the definition built from
/// `N'…'` chunks so it survives the 4000-char literal truncation.
fn push_module(s: &mut String, name: &str, definition: &str) {
    s.push_str(&format!("SET @sql = {};\n", dynamic_literal(definition)));
    s.push_str(&format!(
        "IF OBJECT_ID(N'[{SCHEMA}].[{name}]') IS NULL EXEC sp_executesql @sql;\n\n"
    ));
}

fn column_ddl(c: &ColumnInfo) -> String {
    if c.is_generated {
        let expr = c.default.as_deref().unwrap_or("(NULL)");
        return format!("[{}] AS {expr}", c.name);
    }
    let mut s = format!("[{}] {}", c.name, c.type_);
    if let Some(d) = &c.default {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    s.push_str(if c.not_null { " NOT NULL" } else { " NULL" });
    s
}

fn constraint_rank(kind: char) -> u8 {
    match kind {
        'p' => 0, // primary key
        'u' => 1, // unique
        'c' => 2, // check
        'f' => 3, // foreign key — referenced keys must exist first
        _ => 4,
    }
}

/// Build the SQL Server type text. Identity is appended by the caller.
fn store_type(
    data_type: &str,
    max_len: Option<i32>,
    prec: Option<i32>,
    scale: Option<i32>,
) -> String {
    match data_type {
        "nvarchar" | "varchar" | "nchar" | "char" | "varbinary" | "binary" => match max_len {
            Some(-1) => format!("{data_type}(max)"),
            Some(n) if n > 0 => format!("{data_type}({n})"),
            _ => data_type.to_string(),
        },
        "decimal" | "numeric" => match (prec, scale) {
            (Some(p), Some(sc)) => format!("{data_type}({p},{sc})"),
            _ => data_type.to_string(),
        },
        other => other.to_string(),
    }
}

/// SQL Server's `*_referential_action_desc` is `NO_ACTION`/`CASCADE`/`SET_NULL`/
/// `SET_DEFAULT`. `NO_ACTION` is the default and need not be emitted.
fn referential_action(desc: &str) -> Option<String> {
    match desc.trim() {
        "" | "NO_ACTION" => None,
        other => Some(other.replace('_', " ")),
    }
}

fn join_cols(cols: &[String]) -> String {
    cols.iter()
        .map(|c| format!("[{c}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render `definition` as one or more concatenated `N'…'` literals. A single
/// T-SQL string literal longer than 4000 characters is silently truncated, so we
/// split into ≤2000-char chunks, never cutting an escaped `''` pair.
fn dynamic_literal(definition: &str) -> String {
    let escaped = escape(definition);
    let chars: Vec<char> = escaped.chars().collect();
    if chars.is_empty() {
        return "N''".to_string();
    }
    let mut parts = Vec::new();
    let mut i = 0;
    const MAX: usize = 2000;
    while i < chars.len() {
        let mut end = (i + MAX).min(chars.len());
        if end < chars.len() {
            // Don't split between the two apostrophes of an escaped pair: if the
            // chunk ends with an odd run of apostrophes, hand one to the next chunk.
            let mut q = 0;
            let mut j = end;
            while j > i && chars[j - 1] == '\'' {
                q += 1;
                j -= 1;
            }
            if q % 2 == 1 {
                end -= 1;
            }
        }
        let chunk: String = chars[i..end].iter().collect();
        parts.push(format!("N'{chunk}'"));
        i = end;
    }
    parts.join(" + ")
}

fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

fn section(s: &mut String, title: &str) {
    s.push_str(&format!("-- ===== {title} =====\n"));
}

/// Mask the password in an ADO connection string (`Password=…` / `Pwd=…`).
fn redact_ado(conn: &str) -> String {
    conn.split(';')
        .map(|seg| {
            let key = seg
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if key == "password" || key == "pwd" {
                format!("{}=***", seg.split('=').next().unwrap_or("").trim_end())
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, type_: &str, not_null: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_: type_.into(),
            not_null,
            default: None,
            is_generated: false,
        }
    }

    #[test]
    fn table_and_column_are_idempotent() {
        let mut diff = DiffResult::default();
        diff.new_tables.push(TableInfo {
            name: "Invoice".into(),
            columns: vec![
                col("Id", "int IDENTITY(1,1)", true),
                col("Total", "decimal(18,2)", true),
            ],
            create_sql: None,
        });
        diff.new_columns.push((
            "Customer".into(),
            ColumnInfo {
                name: "Email".into(),
                type_: "nvarchar(256)".into(),
                not_null: false,
                default: None,
                is_generated: false,
            },
        ));
        let sql = build_ddl(&diff);
        assert!(sql.contains("IF OBJECT_ID(N'[dbo].[Invoice]', N'U') IS NULL"));
        assert!(sql.contains("[Id] int IDENTITY(1,1) NOT NULL"));
        assert!(sql.contains("[Total] decimal(18,2) NOT NULL"));
        assert!(sql.contains("IF COL_LENGTH(N'[dbo].[Customer]', N'Email') IS NULL"));
        assert!(sql.contains("ADD [Email] nvarchar(256) NULL"));
    }

    #[test]
    fn constraints_ordered_pk_before_fk() {
        let mut diff = DiffResult::default();
        diff.new_constraints.push(ConstraintInfo {
            table: "Order".into(),
            name: "FK_Order_Customer".into(),
            kind: 'f',
            definition: "FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([Id])".into(),
        });
        diff.new_constraints.push(ConstraintInfo {
            table: "Order".into(),
            name: "PK_Order".into(),
            kind: 'p',
            definition: "PRIMARY KEY ([Id])".into(),
        });
        let sql = build_ddl(&diff);
        assert!(sql.find("PK_Order").unwrap() < sql.find("FK_Order_Customer").unwrap());
        assert!(sql.contains("IF NOT EXISTS (SELECT 1 FROM sys.objects WHERE name = N'PK_Order'"));
    }

    #[test]
    fn module_uses_guarded_dynamic_sql() {
        let mut diff = DiffResult::default();
        diff.new_views.push(ViewInfo {
            name: "vCustomer".into(),
            definition: "CREATE VIEW [dbo].[vCustomer] AS SELECT 1 AS x".into(),
        });
        let sql = build_ddl(&diff);
        assert!(sql.starts_with("DECLARE @sql nvarchar(max);"));
        assert!(sql.contains("SET @sql = N'CREATE VIEW [dbo].[vCustomer] AS SELECT 1 AS x';"));
        assert!(sql.contains("IF OBJECT_ID(N'[dbo].[vCustomer]') IS NULL EXEC sp_executesql @sql;"));
    }

    #[test]
    fn dynamic_literal_escapes_and_chunks_safely() {
        // Quotes are doubled and kept in matching pairs across chunk boundaries.
        let def = format!("a{}b'c", "x".repeat(2500));
        let lit = dynamic_literal(&def);
        assert!(lit.contains(" + "), "long definition must be chunked");
        assert!(lit.contains("''"), "the literal apostrophe must be escaped");
        // No chunk ends with a dangling single apostrophe.
        for part in lit.split(" + ") {
            let inner = part.trim_start_matches("N'").trim_end_matches('\'');
            let trailing = inner.chars().rev().take_while(|&c| c == '\'').count();
            assert_eq!(trailing % 2, 0, "chunk splits an escaped pair: {part}");
        }
    }

    #[test]
    fn redact_masks_password() {
        assert_eq!(
            redact_ado("Server=h,1433;Database=db;User Id=sa;Password=secret;Encrypt=true"),
            "Server=h,1433;Database=db;User Id=sa;Password=***;Encrypt=true"
        );
    }
}
