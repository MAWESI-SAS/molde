//! Reading the schema of an existing database → [`DatabaseModel`].
//!
//! Engine-specific: SQLite uses `PRAGMA`, PostgreSQL uses `information_schema`.
//! Produces the same IR ([`DatabaseModel`]) as the molde language parser, so that
//! the rest of the system (`.model` emission, diff) is agnostic of the source.

use std::collections::BTreeMap;

use molde_core::model::{
    Column, DatabaseModel, ForeignKey, Index, PrimaryKey, ReferentialAction, Table,
};
use molde_providers::Provider;
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::{AnyPool, Row};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

type SsClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("SQL Server error: {0}")]
    Mssql(#[from] tiberius::error::Error),
    #[error("network error connecting to SQL Server: {0}")]
    Io(#[from] std::io::Error),
}

/// Internal tables that are never included in the scaffold.
const SKIP_TABLES: &[&str] = &["__EFMigrationsHistory"];

/// Connects and reads the full database model.
pub async fn read_model(
    url: &str,
    provider: Provider,
    schema: Option<&str>,
) -> Result<DatabaseModel, ReadError> {
    // SQL Server uses tiberius (outside sqlx's Any driver).
    if matches!(provider, Provider::SqlServer) {
        return read_sqlserver(url, schema.unwrap_or("dbo")).await;
    }

    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await?;
    match provider {
        Provider::Sqlite => read_sqlite(&pool).await,
        Provider::Postgres => read_postgres(&pool, schema.unwrap_or("public")).await,
        Provider::MySql => read_mysql(&pool, schema).await,
        Provider::SqlServer => unreachable!("SQL Server is handled earlier with tiberius"),
    }
}

fn escape_literal(s: &str) -> String {
    s.replace('\'', "''")
}

fn parse_action(s: &str) -> ReferentialAction {
    match s.to_ascii_uppercase().as_str() {
        "CASCADE" => ReferentialAction::Cascade,
        "SET NULL" => ReferentialAction::SetNull,
        "SET DEFAULT" => ReferentialAction::SetDefault,
        "RESTRICT" => ReferentialAction::Restrict,
        _ => ReferentialAction::NoAction,
    }
}

// ---------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------

async fn read_sqlite(pool: &AnyPool) -> Result<DatabaseModel, ReadError> {
    let mut model = DatabaseModel::empty();

    let table_rows = sqlx::query(
        "SELECT name FROM sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    for row in table_rows {
        let name: String = row.try_get("name")?;
        if SKIP_TABLES.contains(&name.as_str()) {
            continue;
        }
        model.tables.push(read_sqlite_table(pool, &name).await?);
    }

    Ok(model)
}

async fn read_sqlite_table(pool: &AnyPool, name: &str) -> Result<Table, ReadError> {
    let mut table = Table {
        name: name.to_string(),
        schema: None,
        clr_type: None,
        comment: None,
        columns: Vec::new(),
        primary_key: None,
        foreign_keys: Vec::new(),
        indexes: Vec::new(),
        triggers: Vec::new(),
        seed_data: Vec::new(),
        seed_key: Vec::new(),
    };

    // Columns (PRAGMA table_info: cid, name, type, notnull, dflt_value, pk).
    let cols = sqlx::query(&format!("PRAGMA table_info('{}')", escape_literal(name)))
        .fetch_all(pool)
        .await?;

    let mut pk_cols: Vec<(i64, String)> = Vec::new();
    for c in cols {
        let cname: String = c.try_get("name")?;
        let ctype: String = c.try_get("type").unwrap_or_default();
        let notnull: i64 = c.try_get("notnull")?;
        let dflt: Option<String> = c.try_get("dflt_value")?;
        let pk: i64 = c.try_get("pk")?;

        if pk > 0 {
            pk_cols.push((pk, cname.clone()));
        }

        table.columns.push(Column {
            name: cname,
            store_type: (!ctype.is_empty()).then(|| ctype.clone()),
            clr_type: Some(sqlite_clr(&ctype).to_string()),
            is_nullable: notnull == 0,
            is_identity: false, // adjusted after the PK is known
            max_length: sqlite_max_length(&ctype),
            precision: None,
            scale: None,
            default_value_sql: dflt,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        });
    }

    if !pk_cols.is_empty() {
        pk_cols.sort_by_key(|(order, _)| *order);
        let columns: Vec<String> = pk_cols.into_iter().map(|(_, c)| c).collect();

        // Identity heuristic: single-column PK over an INTEGER column (ROWID alias).
        if columns.len() == 1 {
            if let Some(col) = table.columns.iter_mut().find(|c| c.name == columns[0]) {
                let is_int = col
                    .store_type
                    .as_deref()
                    .map(|t| t.to_ascii_uppercase().contains("INT"))
                    .unwrap_or(false);
                col.is_identity = is_int;
            }
        }
        // PK columns are NOT NULL (SQLite reports notnull=0 for INTEGER PKs that
        // are ROWID aliases, so we force it here).
        for pk_col in &columns {
            if let Some(col) = table.columns.iter_mut().find(|c| &c.name == pk_col) {
                col.is_nullable = false;
            }
        }
        table.primary_key = Some(PrimaryKey {
            name: molde_core::conventions::pk_name(name),
            columns,
        });
    }

    // Foreign keys (PRAGMA foreign_key_list: id, seq, table, from, to, on_update, on_delete).
    let fk_rows = sqlx::query(&format!(
        "PRAGMA foreign_key_list('{}')",
        escape_literal(name)
    ))
    .fetch_all(pool)
    .await?;

    let mut fks: BTreeMap<i64, ForeignKey> = BTreeMap::new();
    for r in fk_rows {
        let id: i64 = r.try_get("id")?;
        let ref_table: String = r.try_get("table")?;
        let from: String = r.try_get("from")?;
        let to: String = r.try_get("to")?;
        let on_delete: String = r.try_get("on_delete").unwrap_or_default();

        let fk = fks.entry(id).or_insert_with(|| {
            // Match the authoring convention for the common single-FK case
            // (id 0); disambiguate extra FKs to the same principal with a suffix.
            let base = molde_core::conventions::fk_name(name, &ref_table);
            let fk_name = if id == 0 {
                base
            } else {
                format!("{base}_{id}")
            };
            ForeignKey {
                name: fk_name,
                columns: Vec::new(),
                principal_table: ref_table,
                principal_schema: None,
                principal_columns: Vec::new(),
                on_delete: parse_action(&on_delete),
            }
        });
        fk.columns.push(from);
        fk.principal_columns.push(to);
    }
    table.foreign_keys = fks.into_values().collect();

    // Explicit indexes (PRAGMA index_list, origin='c' = created with CREATE INDEX).
    let idx_rows = sqlx::query(&format!("PRAGMA index_list('{}')", escape_literal(name)))
        .fetch_all(pool)
        .await?;
    for r in idx_rows {
        let idx_name: String = r.try_get("name")?;
        let unique: i64 = r.try_get("unique")?;
        let origin: String = r.try_get("origin").unwrap_or_default();
        if origin != "c" {
            continue;
        }
        let infos = sqlx::query(&format!(
            "PRAGMA index_info('{}')",
            escape_literal(&idx_name)
        ))
        .fetch_all(pool)
        .await?;
        let mut columns = Vec::new();
        for ir in infos {
            columns.push(ir.try_get::<String, _>("name")?);
        }
        table.indexes.push(Index {
            name: idx_name,
            columns,
            is_unique: unique == 1,
            filter: None,
            method: None,
            operators: Vec::new(),
            expression: None,
        });
    }

    Ok(table)
}

/// Maps a declared SQLite type to CLR by affinity (EF Core rule).
fn sqlite_clr(declared: &str) -> &'static str {
    let t = declared.to_ascii_uppercase();
    if t.contains("INT") {
        "System.Int64" // INTEGER in SQLite is 64-bit
    } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
        "System.String"
    } else if t.contains("BLOB") || t.is_empty() {
        "System.Byte[]"
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        "System.Double"
    } else {
        // NUMERIC/DECIMAL/BOOLEAN/DATE… EF stores them as TEXT → string by default.
        "System.String"
    }
}

/// Extracts `N` from a declared type like `varchar(200)`.
fn sqlite_max_length(declared: &str) -> Option<i32> {
    let open = declared.find('(')?;
    let close = declared.find(')')?;
    declared.get(open + 1..close)?.trim().parse::<i32>().ok()
}

// ---------------------------------------------------------------------------
// PostgreSQL
// ---------------------------------------------------------------------------

async fn read_postgres(pool: &AnyPool, schema: &str) -> Result<DatabaseModel, ReadError> {
    let mut model = DatabaseModel::empty();
    model.default_schema = Some(schema.to_string());
    let sc = escape_literal(schema);

    // Tables.
    // information_schema columns are of type `name`/domains that sqlx's Any
    // driver does not decode; they are cast to text/int explicitly.
    let table_rows = sqlx::query(&format!(
        "SELECT table_name::text AS table_name FROM information_schema.tables \
         WHERE table_schema = '{sc}' AND table_type = 'BASE TABLE' ORDER BY table_name"
    ))
    .fetch_all(pool)
    .await?;

    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    for r in table_rows {
        let name: String = r.try_get("table_name")?;
        if SKIP_TABLES.contains(&name.as_str()) {
            continue;
        }
        tables.insert(
            name.clone(),
            Table {
                name,
                schema: Some(schema.to_string()),
                clr_type: None,
                comment: None,
                columns: Vec::new(),
                primary_key: None,
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                triggers: Vec::new(),
                seed_data: Vec::new(),
                seed_key: Vec::new(),
            },
        );
    }

    // Columns (all at once, grouped by table). Joins pg_attribute to obtain
    // `format_type` (exact type, e.g. `vector(384)`) and reads the generated
    // columns (`GENERATED ALWAYS AS ... STORED`, e.g. tsvector).
    let col_rows = sqlx::query(&format!(
        "SELECT c.table_name::text AS table_name, c.column_name::text AS column_name, \
                c.data_type::text AS data_type, c.udt_name::text AS udt_name, \
                c.is_nullable::text AS is_nullable, \
                c.character_maximum_length::int AS character_maximum_length, \
                c.numeric_precision::int AS numeric_precision, \
                c.numeric_scale::int AS numeric_scale, \
                c.column_default::text AS column_default, \
                c.is_identity::text AS is_identity, \
                c.is_generated::text AS is_generated, \
                c.generation_expression::text AS generation_expression, \
                format_type(a.atttypid, a.atttypmod)::text AS full_type \
         FROM information_schema.columns c \
         JOIN pg_namespace n ON n.nspname = c.table_schema \
         JOIN pg_class cl ON cl.relname = c.table_name AND cl.relnamespace = n.oid \
         JOIN pg_attribute a ON a.attrelid = cl.oid AND a.attname = c.column_name \
         WHERE c.table_schema = '{sc}' AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY c.table_name, c.ordinal_position"
    ))
    .fetch_all(pool)
    .await?;

    for r in col_rows {
        let table_name: String = r.try_get("table_name")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let data_type: String = r.try_get("data_type")?;
        let udt_name: String = r.try_get("udt_name").unwrap_or_default();
        let full_type: String = r.try_get("full_type").unwrap_or_default();
        let is_nullable: String = r.try_get("is_nullable")?;
        let max_length: Option<i32> = r.try_get("character_maximum_length").ok().flatten();
        let precision: Option<i32> = r.try_get("numeric_precision").ok().flatten();
        let scale: Option<i32> = r.try_get("numeric_scale").ok().flatten();
        let default: Option<String> = r.try_get("column_default")?;
        let is_identity: String = r.try_get("is_identity").unwrap_or_default();
        let is_generated: String = r.try_get("is_generated").unwrap_or_default();
        let generation_expression: Option<String> =
            r.try_get("generation_expression").ok().flatten();

        let identity = is_identity.eq_ignore_ascii_case("YES")
            || default
                .as_deref()
                .map(|d| d.contains("nextval("))
                .unwrap_or(false);

        // Generated column (STORED): the expression is preserved for round-trip.
        let computed = is_generated.eq_ignore_ascii_case("ALWAYS");
        let computed_sql = if computed {
            generation_expression
        } else {
            None
        };

        // pgvector loses the dimension via information_schema; use format_type.
        let store_type = if udt_name == "vector" {
            full_type.clone()
        } else {
            pg_store_type(&data_type, &udt_name, max_length, precision, scale)
        };

        table.columns.push(Column {
            name: r.try_get("column_name")?,
            store_type: Some(store_type),
            clr_type: Some(pg_clr(&data_type, &udt_name).to_string()),
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_identity: identity,
            max_length,
            precision,
            scale,
            // Identity/serial defaults are not reproduced as a literal.
            default_value_sql: if identity || computed { None } else { default },
            computed_sql,
            computed_stored: computed,
            collation: None,
            comment: None,
        });
    }

    // Primary keys.
    let pk_rows = sqlx::query(&format!(
        "SELECT tc.table_name::text AS table_name, \
                tc.constraint_name::text AS constraint_name, \
                kcu.column_name::text AS column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
         WHERE tc.table_schema = '{sc}' AND tc.constraint_type = 'PRIMARY KEY' \
         ORDER BY tc.table_name, kcu.ordinal_position"
    ))
    .fetch_all(pool)
    .await?;

    for r in pk_rows {
        let table_name: String = r.try_get("table_name")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let constraint: String = r.try_get("constraint_name")?;
        let column: String = r.try_get("column_name")?;
        let pk = table.primary_key.get_or_insert_with(|| PrimaryKey {
            name: constraint,
            columns: Vec::new(),
        });
        pk.columns.push(column);
    }

    // Foreign keys.
    let fk_rows = sqlx::query(&format!(
        "SELECT tc.table_name::text AS table_name, \
                tc.constraint_name::text AS constraint_name, \
                kcu.column_name::text AS column_name, \
                ccu.table_name::text AS foreign_table, \
                ccu.column_name::text AS foreign_column, \
                rc.delete_rule::text AS delete_rule \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
         JOIN information_schema.referential_constraints rc \
           ON tc.constraint_name = rc.constraint_name AND tc.constraint_schema = rc.constraint_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON ccu.constraint_name = tc.constraint_name AND ccu.constraint_schema = tc.constraint_schema \
         WHERE tc.table_schema = '{sc}' AND tc.constraint_type = 'FOREIGN KEY' \
         ORDER BY tc.table_name, tc.constraint_name, kcu.ordinal_position"
    ))
    .fetch_all(pool)
    .await?;

    for r in fk_rows {
        let table_name: String = r.try_get("table_name")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let constraint: String = r.try_get("constraint_name")?;
        let column: String = r.try_get("column_name")?;
        let foreign_table: String = r.try_get("foreign_table")?;
        let foreign_column: String = r.try_get("foreign_column")?;
        let delete_rule: String = r.try_get("delete_rule").unwrap_or_default();

        match table.foreign_keys.iter_mut().find(|f| f.name == constraint) {
            Some(fk) => {
                fk.columns.push(column);
                fk.principal_columns.push(foreign_column);
            }
            None => table.foreign_keys.push(ForeignKey {
                name: constraint,
                columns: vec![column],
                principal_table: foreign_table,
                principal_schema: Some(schema.to_string()),
                principal_columns: vec![foreign_column],
                on_delete: parse_action(&delete_rule),
            }),
        }
    }

    // Indexes (includes GIN/GiST/HNSW/IVFFlat and expression-based). Those
    // backing the PK or a constraint are excluded (represented separately).
    let idx_rows = sqlx::query(&format!(
        "SELECT t.relname::text AS table_name, ic.relname::text AS index_name, \
                ix.indisunique::int AS is_unique, am.amname::text AS method, \
                (ix.indexprs IS NOT NULL)::int AS has_expr, \
                COALESCE(pg_get_expr(ix.indpred, ix.indrelid), '')::text AS filter, \
                pg_get_indexdef(ix.indexrelid)::text AS indexdef \
         FROM pg_index ix \
         JOIN pg_class ic ON ic.oid = ix.indexrelid \
         JOIN pg_class t ON t.oid = ix.indrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_am am ON am.oid = ic.relam \
         WHERE n.nspname = '{sc}' AND NOT ix.indisprimary \
           AND NOT EXISTS (SELECT 1 FROM pg_constraint con WHERE con.conindid = ix.indexrelid) \
         ORDER BY t.relname, ic.relname"
    ))
    .fetch_all(pool)
    .await?;

    for r in idx_rows {
        let table_name: String = r.try_get("table_name")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let amname: String = r.try_get("method").unwrap_or_default();
        let is_unique: i32 = r.try_get("is_unique").unwrap_or(0);
        let has_expr: i32 = r.try_get("has_expr").unwrap_or(0);
        let filter: String = r.try_get("filter").unwrap_or_default();
        let indexdef: String = r.try_get("indexdef").unwrap_or_default();

        let (columns, operators, expression) = parse_pg_index_keys(&indexdef, has_expr != 0);
        // btree is the default method: not annotated, to keep the model clean.
        let method = if amname.eq_ignore_ascii_case("btree") {
            None
        } else {
            Some(amname)
        };
        table.indexes.push(Index {
            name: r.try_get("index_name")?,
            columns,
            is_unique: is_unique != 0,
            filter: if filter.is_empty() {
                None
            } else {
                Some(filter)
            },
            method,
            operators,
            expression,
        });
    }

    // Triggers (EF does not model them; preserved raw).
    let trg_rows = sqlx::query(&format!(
        "SELECT t.relname::text AS table_name, tg.tgname::text AS trigger_name, \
                tg.tgtype::int AS tgtype, pg_get_triggerdef(tg.oid)::text AS def, \
                p.proname::text AS func_name, fn.nspname::text AS func_schema \
         FROM pg_trigger tg \
         JOIN pg_class t ON t.oid = tg.tgrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_proc p ON p.oid = tg.tgfoid \
         JOIN pg_namespace fn ON fn.oid = p.pronamespace \
         WHERE n.nspname = '{sc}' AND NOT tg.tgisinternal \
         ORDER BY t.relname, tg.tgname"
    ))
    .fetch_all(pool)
    .await?;

    for r in trg_rows {
        let table_name: String = r.try_get("table_name")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let tgtype: i32 = r.try_get("tgtype").unwrap_or(0);
        let def: String = r.try_get("def").unwrap_or_default();
        let func_name: String = r.try_get("func_name").unwrap_or_default();
        let func_schema: String = r.try_get("func_schema").unwrap_or_default();

        let (timing, events) = parse_pg_trigger_type(tgtype);
        table.triggers.push(molde_core::model::Trigger {
            name: r.try_get("trigger_name")?,
            table: table_name.clone(),
            schema: Some(schema.to_string()),
            timing,
            events,
            function: Some(format!("{func_schema}.{func_name}")),
            definition: def,
        });
    }

    // All user functions in the schema (not just trigger ones): those used in
    // triggers AND in index expressions (e.g. `immutable_unaccent`).
    // Extension functions (deptype 'e') and C/internal ones are excluded.
    let fn_rows = sqlx::query(&format!(
        "SELECT p.proname::text AS name, n.nspname::text AS schema, \
                pg_get_functiondef(p.oid)::text AS def \
         FROM pg_proc p \
         JOIN pg_namespace n ON n.oid = p.pronamespace \
         JOIN pg_language l ON l.oid = p.prolang \
         WHERE n.nspname = '{sc}' AND p.prokind = 'f' \
           AND l.lanname NOT IN ('c', 'internal') \
           AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e') \
         ORDER BY p.proname"
    ))
    .fetch_all(pool)
    .await?;
    let mut functions = Vec::new();
    for r in fn_rows {
        let def: String = r.try_get("def").unwrap_or_default();
        if def.is_empty() {
            continue;
        }
        functions.push(molde_core::model::DbFunction {
            name: r.try_get("name")?,
            schema: r.try_get::<String, _>("schema").ok(),
            definition: def,
        });
    }

    // Installed extensions (ensured with CREATE EXTENSION IF NOT EXISTS).
    // `plpgsql` ships by default in every DB → omitted.
    let ext_rows = sqlx::query(
        "SELECT extname::text AS name FROM pg_extension \
         WHERE extname <> 'plpgsql' ORDER BY extname",
    )
    .fetch_all(pool)
    .await?;
    let extensions: Vec<String> = ext_rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("name").ok())
        .collect();

    model.tables = tables.into_values().collect();
    model.functions = functions;
    model.extensions = extensions;
    Ok(model)
}

/// Extracts (columns, operator-classes, expression) from the textual `CREATE INDEX`
/// returned by `pg_get_indexdef`. For expression-based indexes it returns the raw
/// expression and leaves the columns empty.
fn parse_pg_index_keys(
    indexdef: &str,
    has_expr: bool,
) -> (Vec<String>, Vec<String>, Option<String>) {
    // The key list is the content between the first '(' after `USING ...` and
    // its balanced ')'.
    let Some(open) = indexdef.find('(') else {
        return (Vec::new(), Vec::new(), None);
    };
    let bytes = indexdef.as_bytes();
    let mut depth = 0i32;
    let mut close = open;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let keylist = &indexdef[open + 1..close];

    if has_expr {
        // Functional/FTS index: the full raw expression is kept.
        return (Vec::new(), Vec::new(), Some(keylist.trim().to_string()));
    }

    // Column index: split by top-level commas; each part is `"col"` or `col`
    // with an optional operator class behind it.
    let mut columns = Vec::new();
    let mut operators = Vec::new();
    let mut any_op = false;
    for part in split_top_level(keylist) {
        let part = part.trim();
        let (col, op) = split_index_key_part(part);
        columns.push(col);
        if op.is_empty() {
            operators.push(String::new());
        } else {
            operators.push(op);
            any_op = true;
        }
    }
    let operators = if any_op { operators } else { Vec::new() };
    (columns, operators, None)
}

/// Splits an index key into (column, operator-class). The column may be quoted
/// with double quotes; the operator class is the first token after it.
fn split_index_key_part(part: &str) -> (String, String) {
    let part = part.trim();
    if let Some(rest) = part.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            let col = rest[..end].to_string();
            let tail = rest[end + 1..].trim();
            let op = tail.split_whitespace().next().unwrap_or("").to_string();
            return (col, op);
        }
    }
    let mut it = part.split_whitespace();
    let col = it.next().unwrap_or("").to_string();
    let op = it.next().unwrap_or("").to_string();
    (col, op)
}

/// Splits by top-level commas (ignores commas inside parentheses).
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].to_string());
    out
}

/// Decodes the `pg_trigger.tgtype` flags into (timing, events).
fn parse_pg_trigger_type(
    tgtype: i32,
) -> (
    molde_core::model::TriggerTiming,
    Vec<molde_core::model::TriggerEvent>,
) {
    use molde_core::model::{TriggerEvent, TriggerTiming};
    let timing = if tgtype & (1 << 1) != 0 {
        TriggerTiming::Before
    } else if tgtype & (1 << 6) != 0 {
        TriggerTiming::InsteadOf
    } else {
        TriggerTiming::After
    };
    let mut events = Vec::new();
    if tgtype & (1 << 2) != 0 {
        events.push(TriggerEvent::Insert);
    }
    if tgtype & (1 << 3) != 0 {
        events.push(TriggerEvent::Delete);
    }
    if tgtype & (1 << 4) != 0 {
        events.push(TriggerEvent::Update);
    }
    if tgtype & (1 << 5) != 0 {
        events.push(TriggerEvent::Truncate);
    }
    (timing, events)
}

fn pg_clr(data_type: &str, udt: &str) -> &'static str {
    match data_type {
        "smallint" => "System.Int16",
        "integer" => "System.Int32",
        "bigint" => "System.Int64",
        "boolean" => "System.Boolean",
        "real" => "System.Single",
        "double precision" => "System.Double",
        "numeric" | "money" => "System.Decimal",
        "uuid" => "System.Guid",
        "date" | "timestamp without time zone" | "timestamp" => "System.DateTime",
        "timestamp with time zone" => "System.DateTimeOffset",
        "bytea" => "System.Byte[]",
        // Full-text search: tsvector → native Npgsql type.
        "tsvector" => "NpgsqlTypes.NpgsqlTsVector",
        "character varying" | "varchar" | "character" | "char" | "text" | "citext" => {
            "System.String"
        }
        _ => match udt {
            "int2" => "System.Int16",
            "int4" => "System.Int32",
            "int8" => "System.Int64",
            "bool" => "System.Boolean",
            "tsvector" => "NpgsqlTypes.NpgsqlTsVector",
            // pgvector: vector(N) column → native Pgvector.Vector type.
            "vector" => "Pgvector.Vector",
            _ => "System.String",
        },
    }
}

fn pg_store_type(
    data_type: &str,
    udt: &str,
    max_length: Option<i32>,
    precision: Option<i32>,
    scale: Option<i32>,
) -> String {
    match data_type {
        "character varying" => match max_length {
            Some(n) => format!("character varying({n})"),
            None => "character varying".to_string(),
        },
        "character" => match max_length {
            Some(n) => format!("character({n})"),
            None => "character".to_string(),
        },
        // numeric/decimal with an explicit constraint: preserve (precision, scale)
        // for an exact round-trip. `numeric` without precision stays unconstrained.
        "numeric" | "decimal" => match (precision, scale) {
            (Some(p), Some(s)) => format!("{data_type}({p},{s})"),
            (Some(p), None) => format!("{data_type}({p})"),
            _ => data_type.to_string(),
        },
        "USER-DEFINED" => udt.to_string(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// MySQL / MariaDB
// ---------------------------------------------------------------------------

/// Reads a string column from MySQL. The Any driver classifies the TEXT/BLOB
/// columns of information_schema as `Blob`, so they are read as bytes.
fn my_str(row: &sqlx::any::AnyRow, col: &str) -> Result<String, sqlx::Error> {
    if let Ok(s) = row.try_get::<String, _>(col) {
        return Ok(s);
    }
    let bytes: Vec<u8> = row.try_get(col)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn my_opt_str(row: &sqlx::any::AnyRow, col: &str) -> Result<Option<String>, sqlx::Error> {
    if let Ok(s) = row.try_get::<Option<String>, _>(col) {
        return Ok(s);
    }
    let bytes: Option<Vec<u8>> = row.try_get(col)?;
    Ok(bytes.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

async fn read_mysql(pool: &AnyPool, schema: Option<&str>) -> Result<DatabaseModel, ReadError> {
    // The "schema" in MySQL is the database; if not given, the current one.
    let db = match schema {
        Some(s) => s.to_string(),
        None => {
            let row = sqlx::query("SELECT CONVERT(DATABASE() USING utf8mb4) AS db")
                .fetch_one(pool)
                .await?;
            my_opt_str(&row, "db")?.unwrap_or_default()
        }
    };
    let sc = escape_literal(&db);

    let mut model = DatabaseModel::empty();
    model.default_schema = Some(db.clone());

    // Tables.
    // The string columns of information_schema in MySQL arrive as BLOB to the
    // Any driver; they are cast to CHAR explicitly.
    let table_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = '{sc}' AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME"
    ))
    .fetch_all(pool)
    .await?;

    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    for r in table_rows {
        let name = my_str(&r, "TABLE_NAME")?;
        if SKIP_TABLES.contains(&name.as_str()) {
            continue;
        }
        tables.insert(
            name.clone(),
            Table {
                name,
                schema: Some(db.clone()),
                clr_type: None,
                comment: None,
                columns: Vec::new(),
                primary_key: None,
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                triggers: Vec::new(),
                seed_data: Vec::new(),
                seed_key: Vec::new(),
            },
        );
    }

    // Columns (numerics cast to SIGNED for the Any driver).
    let col_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME, \
                CONVERT(DATA_TYPE USING utf8mb4) AS DATA_TYPE, \
                CONVERT(COLUMN_TYPE USING utf8mb4) AS COLUMN_TYPE, \
                CONVERT(IS_NULLABLE USING utf8mb4) AS IS_NULLABLE, \
                CONVERT(EXTRA USING utf8mb4) AS EXTRA, \
                CONVERT(GENERATION_EXPRESSION USING utf8mb4) AS GENERATION_EXPRESSION, \
                CAST(CHARACTER_MAXIMUM_LENGTH AS SIGNED) AS CHARACTER_MAXIMUM_LENGTH, \
                CAST(NUMERIC_PRECISION AS SIGNED) AS NUMERIC_PRECISION, \
                CAST(NUMERIC_SCALE AS SIGNED) AS NUMERIC_SCALE, \
                CONVERT(COLUMN_DEFAULT USING utf8mb4) AS COLUMN_DEFAULT \
         FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = '{sc}' \
         ORDER BY TABLE_NAME, ORDINAL_POSITION"
    ))
    .fetch_all(pool)
    .await?;

    for r in col_rows {
        let table_name = my_str(&r, "TABLE_NAME")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let data_type = my_str(&r, "DATA_TYPE")?;
        let column_type = my_str(&r, "COLUMN_TYPE").unwrap_or_default();
        let is_nullable = my_str(&r, "IS_NULLABLE")?;
        let extra = my_str(&r, "EXTRA").unwrap_or_default();
        let max_len: Option<i64> = r.try_get("CHARACTER_MAXIMUM_LENGTH").ok().flatten();
        let precision: Option<i64> = r.try_get("NUMERIC_PRECISION").ok().flatten();
        let scale: Option<i64> = r.try_get("NUMERIC_SCALE").ok().flatten();
        let default: Option<String> = my_opt_str(&r, "COLUMN_DEFAULT").unwrap_or_default();

        let extra_lc = extra.to_ascii_lowercase();
        let identity = extra_lc.contains("auto_increment");
        // Generated columns: EXTRA = 'STORED GENERATED' | 'VIRTUAL GENERATED'.
        let computed_stored = extra_lc.contains("stored generated");
        let computed = computed_stored || extra_lc.contains("virtual generated");
        let gen_expr: Option<String> = my_opt_str(&r, "GENERATION_EXPRESSION").unwrap_or_default();
        let computed_sql = if computed {
            gen_expr.filter(|s| !s.is_empty())
        } else {
            None
        };
        let max_length = match data_type.as_str() {
            "varchar" | "char" | "varbinary" | "binary" => max_len
                .filter(|n| *n > 0 && *n <= i32::MAX as i64)
                .map(|n| n as i32),
            _ => None,
        };

        table.columns.push(Column {
            name: my_str(&r, "COLUMN_NAME")?,
            store_type: Some(if column_type.is_empty() {
                data_type.clone()
            } else {
                column_type
            }),
            clr_type: Some(mysql_clr(&data_type).to_string()),
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_identity: identity,
            max_length,
            precision: precision.map(|n| n as i32),
            scale: scale.map(|n| n as i32),
            default_value_sql: if identity || computed { None } else { default },
            computed_sql,
            computed_stored,
            collation: None,
            comment: None,
        });
    }

    // Primary keys (in MySQL the PK is named 'PRIMARY').
    let pk_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME \
         FROM information_schema.KEY_COLUMN_USAGE \
         WHERE TABLE_SCHEMA = '{sc}' AND CONSTRAINT_NAME = 'PRIMARY' \
         ORDER BY TABLE_NAME, ORDINAL_POSITION"
    ))
    .fetch_all(pool)
    .await?;
    for r in pk_rows {
        let table_name = my_str(&r, "TABLE_NAME")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let column = my_str(&r, "COLUMN_NAME")?;
        let pk = table.primary_key.get_or_insert_with(|| PrimaryKey {
            name: molde_core::conventions::pk_name(&table_name),
            columns: Vec::new(),
        });
        pk.columns.push(column);
    }

    // Foreign keys.
    let fk_rows = sqlx::query(&format!(
        "SELECT CONVERT(k.TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(k.CONSTRAINT_NAME USING utf8mb4) AS CONSTRAINT_NAME, \
                CONVERT(k.COLUMN_NAME USING utf8mb4) AS COLUMN_NAME, \
                CONVERT(k.REFERENCED_TABLE_NAME USING utf8mb4) AS REFERENCED_TABLE_NAME, \
                CONVERT(k.REFERENCED_COLUMN_NAME USING utf8mb4) AS REFERENCED_COLUMN_NAME, \
                CONVERT(r.DELETE_RULE USING utf8mb4) AS DELETE_RULE \
         FROM information_schema.KEY_COLUMN_USAGE k \
         JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
           ON k.CONSTRAINT_NAME = r.CONSTRAINT_NAME AND k.CONSTRAINT_SCHEMA = r.CONSTRAINT_SCHEMA \
         WHERE k.TABLE_SCHEMA = '{sc}' AND k.REFERENCED_TABLE_NAME IS NOT NULL \
         ORDER BY k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION"
    ))
    .fetch_all(pool)
    .await?;
    for r in fk_rows {
        let table_name = my_str(&r, "TABLE_NAME")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let constraint = my_str(&r, "CONSTRAINT_NAME")?;
        let column = my_str(&r, "COLUMN_NAME")?;
        let foreign_table = my_str(&r, "REFERENCED_TABLE_NAME")?;
        let foreign_column = my_str(&r, "REFERENCED_COLUMN_NAME")?;
        let delete_rule = my_str(&r, "DELETE_RULE").unwrap_or_default();

        match table.foreign_keys.iter_mut().find(|f| f.name == constraint) {
            Some(fk) => {
                fk.columns.push(column);
                fk.principal_columns.push(foreign_column);
            }
            None => table.foreign_keys.push(ForeignKey {
                name: constraint,
                columns: vec![column],
                principal_table: foreign_table,
                principal_schema: None,
                principal_columns: vec![foreign_column],
                on_delete: parse_action(&delete_rule),
            }),
        }
    }

    // Indexes (excluding the PK).
    let idx_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(INDEX_NAME USING utf8mb4) AS INDEX_NAME, \
                CAST(NON_UNIQUE AS SIGNED) AS NON_UNIQUE, \
                CONVERT(INDEX_TYPE USING utf8mb4) AS INDEX_TYPE, \
                CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = '{sc}' AND INDEX_NAME <> 'PRIMARY' \
         ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX"
    ))
    .fetch_all(pool)
    .await?;
    for r in idx_rows {
        let table_name = my_str(&r, "TABLE_NAME")?;
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let name = my_str(&r, "INDEX_NAME")?;
        let non_unique: i64 = r.try_get("NON_UNIQUE").unwrap_or(1);
        let column = my_str(&r, "COLUMN_NAME")?;
        // INDEX_TYPE = FULLTEXT|SPATIAL|BTREE|HASH; BTREE/HASH = default method.
        let index_type = my_str(&r, "INDEX_TYPE").unwrap_or_default();
        let method = match index_type.to_ascii_uppercase().as_str() {
            "FULLTEXT" => Some("fulltext".to_string()),
            "SPATIAL" => Some("spatial".to_string()),
            _ => None,
        };
        match table.indexes.iter_mut().find(|i| i.name == name) {
            Some(ix) => ix.columns.push(column),
            None => table.indexes.push(Index {
                name,
                columns: vec![column],
                is_unique: non_unique == 0,
                filter: None,
                method,
                operators: Vec::new(),
                expression: None,
            }),
        }
    }

    model.tables = tables.into_values().collect();
    Ok(model)
}

// ---------------------------------------------------------------------------
// SQL Server (via tiberius)
// ---------------------------------------------------------------------------

async fn ss_query(client: &mut SsClient, sql: &str) -> Result<Vec<tiberius::Row>, ReadError> {
    Ok(client
        .simple_query(sql.to_string())
        .await?
        .into_first_result()
        .await?)
}

async fn read_sqlserver(url: &str, schema: &str) -> Result<DatabaseModel, ReadError> {
    let config = tiberius::Config::from_ado_string(url)?;
    let tcp = tokio::net::TcpStream::connect(config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let mut client = tiberius::Client::connect(config, tcp.compat_write()).await?;

    let sc = escape_literal(schema);
    let mut model = DatabaseModel::empty();
    model.default_schema = Some(schema.to_string());

    // User tables (excludes system tables and the history table).
    let table_rows = ss_query(
        &mut client,
        &format!(
            "SELECT t.name AS table_name FROM sys.tables t \
             WHERE t.is_ms_shipped = 0 AND t.name <> '__EFMigrationsHistory' \
             AND SCHEMA_NAME(t.schema_id) = '{sc}' ORDER BY t.name"
        ),
    )
    .await?;

    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    for r in &table_rows {
        let name = r.get::<&str, _>("table_name").unwrap_or("").to_string();
        tables.insert(
            name.clone(),
            Table {
                name,
                schema: Some(schema.to_string()),
                clr_type: None,
                comment: None,
                columns: Vec::new(),
                primary_key: None,
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                triggers: Vec::new(),
                seed_data: Vec::new(),
                seed_key: Vec::new(),
            },
        );
    }

    // Columns.
    let col_rows = ss_query(
        &mut client,
        &format!(
            "SELECT c.TABLE_NAME AS table_name, c.COLUMN_NAME AS column_name, \
                    c.DATA_TYPE AS data_type, c.IS_NULLABLE AS is_nullable, \
                    CAST(c.CHARACTER_MAXIMUM_LENGTH AS int) AS max_len, \
                    CAST(c.NUMERIC_PRECISION AS int) AS num_prec, \
                    CAST(c.NUMERIC_SCALE AS int) AS num_scale, \
                    CAST(COLUMNPROPERTY(OBJECT_ID(QUOTENAME(c.TABLE_SCHEMA) + '.' + QUOTENAME(c.TABLE_NAME)), \
                         c.COLUMN_NAME, 'IsIdentity') AS int) AS is_identity \
             FROM INFORMATION_SCHEMA.COLUMNS c WHERE c.TABLE_SCHEMA = '{sc}' \
             ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"
        ),
    )
    .await?;

    for r in &col_rows {
        let table_name = r.get::<&str, _>("table_name").unwrap_or("");
        let Some(table) = tables.get_mut(table_name) else {
            continue;
        };
        let data_type = r.get::<&str, _>("data_type").unwrap_or("").to_string();
        let is_nullable = r.get::<&str, _>("is_nullable").unwrap_or("YES");
        let max_len = r.get::<i32, _>("max_len");
        let precision = r.get::<i32, _>("num_prec");
        let scale = r.get::<i32, _>("num_scale");
        let identity = r.get::<i32, _>("is_identity").unwrap_or(0) == 1;

        let is_string = matches!(
            data_type.as_str(),
            "nvarchar" | "varchar" | "nchar" | "char"
        );
        let max_length = if is_string {
            max_len.filter(|n| *n > 0)
        } else {
            None
        };

        table.columns.push(Column {
            name: r.get::<&str, _>("column_name").unwrap_or("").to_string(),
            store_type: Some(ss_store_type(&data_type, max_len, precision, scale)),
            clr_type: Some(ss_clr(&data_type).to_string()),
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_identity: identity,
            max_length,
            precision,
            scale,
            default_value_sql: None,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        });
    }

    // Computed columns (sys.computed_columns): definition + whether PERSISTED.
    let cc_rows = ss_query(
        &mut client,
        &format!(
            "SELECT t.name AS table_name, c.name AS column_name, \
                    cc.definition AS definition, CAST(cc.is_persisted AS int) AS is_persisted \
             FROM sys.computed_columns cc \
             JOIN sys.tables t ON t.object_id = cc.object_id \
             JOIN sys.columns c ON c.object_id = cc.object_id AND c.column_id = cc.column_id \
             WHERE SCHEMA_NAME(t.schema_id) = '{sc}'"
        ),
    )
    .await?;
    for r in &cc_rows {
        let table_name = r.get::<&str, _>("table_name").unwrap_or("");
        let column_name = r.get::<&str, _>("column_name").unwrap_or("");
        let definition = r.get::<&str, _>("definition").unwrap_or("").to_string();
        let persisted = r.get::<i32, _>("is_persisted").unwrap_or(0) == 1;
        if let Some(table) = tables.get_mut(table_name) {
            if let Some(col) = table.columns.iter_mut().find(|c| c.name == column_name) {
                col.computed_sql = Some(definition);
                col.computed_stored = persisted;
                col.default_value_sql = None;
            }
        }
    }

    // Primary keys.
    let pk_rows = ss_query(
        &mut client,
        &format!(
            "SELECT tc.TABLE_NAME AS table_name, kcu.COLUMN_NAME AS column_name, \
                    tc.CONSTRAINT_NAME AS constraint_name \
             FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
             JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu \
               ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
             WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_SCHEMA = '{sc}' \
             ORDER BY tc.TABLE_NAME, kcu.ORDINAL_POSITION"
        ),
    )
    .await?;
    for r in &pk_rows {
        let table_name = r.get::<&str, _>("table_name").unwrap_or("");
        let Some(table) = tables.get_mut(table_name) else {
            continue;
        };
        let constraint = r
            .get::<&str, _>("constraint_name")
            .unwrap_or("")
            .to_string();
        let column = r.get::<&str, _>("column_name").unwrap_or("").to_string();
        let pk = table.primary_key.get_or_insert_with(|| PrimaryKey {
            name: constraint,
            columns: Vec::new(),
        });
        pk.columns.push(column);
    }

    // Foreign keys (sys.*).
    let fk_rows = ss_query(
        &mut client,
        &format!(
            "SELECT fk.name AS fk_name, tp.name AS table_name, cp.name AS column_name, \
                    tr.name AS ref_table, cr.name AS ref_column, \
                    fk.delete_referential_action_desc AS delete_rule \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.tables tp ON tp.object_id = fk.parent_object_id \
             JOIN sys.columns cp ON cp.object_id = tp.object_id AND cp.column_id = fkc.parent_column_id \
             JOIN sys.tables tr ON tr.object_id = fk.referenced_object_id \
             JOIN sys.columns cr ON cr.object_id = tr.object_id AND cr.column_id = fkc.referenced_column_id \
             WHERE SCHEMA_NAME(tp.schema_id) = '{sc}' \
             ORDER BY fk.name, fkc.constraint_column_id"
        ),
    )
    .await?;
    for r in &fk_rows {
        let table_name = r.get::<&str, _>("table_name").unwrap_or("");
        let Some(table) = tables.get_mut(table_name) else {
            continue;
        };
        let name = r.get::<&str, _>("fk_name").unwrap_or("").to_string();
        let column = r.get::<&str, _>("column_name").unwrap_or("").to_string();
        let ref_table = r.get::<&str, _>("ref_table").unwrap_or("").to_string();
        let ref_column = r.get::<&str, _>("ref_column").unwrap_or("").to_string();
        // SQL Server uses NO_ACTION/SET_NULL… with underscores.
        let delete_rule = r
            .get::<&str, _>("delete_rule")
            .unwrap_or("")
            .replace('_', " ");

        match table.foreign_keys.iter_mut().find(|f| f.name == name) {
            Some(fk) => {
                fk.columns.push(column);
                fk.principal_columns.push(ref_column);
            }
            None => table.foreign_keys.push(ForeignKey {
                name,
                columns: vec![column],
                principal_table: ref_table,
                principal_schema: Some(schema.to_string()),
                principal_columns: vec![ref_column],
                on_delete: parse_action(&delete_rule),
            }),
        }
    }

    // Indexes (excluding the PK).
    let idx_rows = ss_query(
        &mut client,
        &format!(
            "SELECT t.name AS table_name, i.name AS index_name, \
                    CAST(i.is_unique AS int) AS is_unique, c.name AS column_name \
             FROM sys.indexes i \
             JOIN sys.tables t ON t.object_id = i.object_id \
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns c ON c.object_id = i.object_id AND c.column_id = ic.column_id \
             WHERE i.is_primary_key = 0 AND i.name IS NOT NULL AND t.is_ms_shipped = 0 \
             AND SCHEMA_NAME(t.schema_id) = '{sc}' \
             ORDER BY t.name, i.name, ic.key_ordinal"
        ),
    )
    .await?;
    for r in &idx_rows {
        let table_name = r.get::<&str, _>("table_name").unwrap_or("");
        let Some(table) = tables.get_mut(table_name) else {
            continue;
        };
        let name = r.get::<&str, _>("index_name").unwrap_or("").to_string();
        let is_unique = r.get::<i32, _>("is_unique").unwrap_or(0) == 1;
        let column = r.get::<&str, _>("column_name").unwrap_or("").to_string();
        match table.indexes.iter_mut().find(|i| i.name == name) {
            Some(ix) => ix.columns.push(column),
            None => table.indexes.push(Index {
                name,
                columns: vec![column],
                is_unique,
                filter: None,
                method: None,
                operators: Vec::new(),
                expression: None,
            }),
        }
    }

    model.tables = tables.into_values().collect();

    // Full-text (best-effort): preserves CREATE FULLTEXT CATALOG/INDEX as raw
    // DDL. The sys.fulltext_* views exist even when the component is not
    // installed (they return empty); if the query fails, it is skipped with a warning.
    model.raw_objects = read_sqlserver_fulltext(&mut client, &sc).await;

    Ok(model)
}

/// Reads SQL Server's full-text indexes and builds their raw DDL
/// (`CREATE FULLTEXT CATALOG` + `CREATE FULLTEXT INDEX … KEY INDEX …`).
/// Best-effort: returns empty if there is no full-text or if the query fails.
async fn read_sqlserver_fulltext(client: &mut SsClient, sc: &str) -> Vec<String> {
    let sql = format!(
        "SELECT SCHEMA_NAME(t.schema_id) AS schema_name, t.name AS table_name, \
                c.name AS column_name, ki.name AS key_index, cat.name AS catalog_name \
         FROM sys.fulltext_index_columns fic \
         JOIN sys.tables t ON t.object_id = fic.object_id \
         JOIN sys.columns c ON c.object_id = fic.object_id AND c.column_id = fic.column_id \
         JOIN sys.fulltext_indexes fti ON fti.object_id = fic.object_id \
         JOIN sys.indexes ki ON ki.object_id = fti.object_id AND ki.index_id = fti.unique_index_id \
         JOIN sys.fulltext_catalogs cat ON cat.fulltext_catalog_id = fti.fulltext_catalog_id \
         WHERE SCHEMA_NAME(t.schema_id) = '{sc}' \
         ORDER BY t.name, fic.column_id"
    );
    let rows = match ss_query(client, &sql).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("could not read SQL Server full-text indexes: {e}");
            return Vec::new();
        }
    };

    // Group columns by (table) preserving catalog and key index.
    let mut by_table: BTreeMap<String, (String, String, String, Vec<String>)> = BTreeMap::new();
    for r in &rows {
        let schema_name = r.get::<&str, _>("schema_name").unwrap_or(sc).to_string();
        let table_name = r.get::<&str, _>("table_name").unwrap_or("").to_string();
        let column_name = r.get::<&str, _>("column_name").unwrap_or("").to_string();
        let key_index = r.get::<&str, _>("key_index").unwrap_or("").to_string();
        let catalog = r.get::<&str, _>("catalog_name").unwrap_or("").to_string();
        let entry = by_table.entry(table_name.clone()).or_insert((
            schema_name,
            key_index,
            catalog,
            Vec::new(),
        ));
        entry.3.push(column_name);
    }

    let mut out = Vec::new();
    let mut catalogs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (table, (schema_name, key_index, catalog, cols)) in &by_table {
        if catalogs.insert(catalog.clone()) {
            out.push(format!(
                "IF NOT EXISTS (SELECT 1 FROM sys.fulltext_catalogs WHERE name = '{catalog}') \
                 CREATE FULLTEXT CATALOG [{catalog}];"
            ));
        }
        let col_list = cols
            .iter()
            .map(|c| format!("[{c}]"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "CREATE FULLTEXT INDEX ON [{schema_name}].[{table}] ({col_list}) \
             KEY INDEX [{key_index}] ON [{catalog}];"
        ));
    }
    out
}

fn ss_store_type(
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
            (Some(p), Some(s)) => format!("{data_type}({p},{s})"),
            _ => data_type.to_string(),
        },
        other => other.to_string(),
    }
}

fn ss_clr(data_type: &str) -> &'static str {
    match data_type {
        "int" => "System.Int32",
        "bigint" => "System.Int64",
        "smallint" => "System.Int16",
        "tinyint" => "System.Byte",
        "bit" => "System.Boolean",
        "real" => "System.Single",
        "float" => "System.Double",
        "decimal" | "numeric" | "money" | "smallmoney" => "System.Decimal",
        "datetime" | "datetime2" | "smalldatetime" | "date" | "time" => "System.DateTime",
        "datetimeoffset" => "System.DateTimeOffset",
        "uniqueidentifier" => "System.Guid",
        "varbinary" | "binary" | "image" | "rowversion" | "timestamp" => "System.Byte[]",
        _ => "System.String",
    }
}

fn mysql_clr(data_type: &str) -> &'static str {
    match data_type {
        "tinyint" | "bit" => "System.Boolean",
        "smallint" => "System.Int16",
        "int" | "integer" | "mediumint" => "System.Int32",
        "bigint" => "System.Int64",
        "float" => "System.Single",
        "double" | "real" => "System.Double",
        "decimal" | "numeric" => "System.Decimal",
        "datetime" | "timestamp" | "date" | "time" | "year" => "System.DateTime",
        "blob" | "longblob" | "mediumblob" | "tinyblob" | "binary" | "varbinary" => "System.Byte[]",
        _ => "System.String",
    }
}
