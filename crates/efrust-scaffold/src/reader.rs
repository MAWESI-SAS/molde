//! Lectura del esquema de una base de datos existente → [`DatabaseModel`].
//!
//! Específico por motor: SQLite usa `PRAGMA`, PostgreSQL usa `information_schema`.
//! Produce el mismo IR que el sidecar, de modo que el resto del sistema (codegen,
//! diff) es agnóstico del origen.

use std::collections::BTreeMap;

use efrust_core::model::{Column, DatabaseModel, ForeignKey, Index, PrimaryKey, ReferentialAction, Table};
use efrust_providers::Provider;
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::{AnyPool, Row};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

type SsClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("error de base de datos: {0}")]
    Db(#[from] sqlx::Error),
    #[error("error de SQL Server: {0}")]
    Mssql(#[from] tiberius::error::Error),
    #[error("error de red conectando a SQL Server: {0}")]
    Io(#[from] std::io::Error),
}

/// Tablas internas que nunca se incluyen en el scaffold.
const SKIP_TABLES: &[&str] = &["__EFMigrationsHistory"];

/// Conecta y lee el modelo completo de la base de datos.
pub async fn read_model(
    url: &str,
    provider: Provider,
    schema: Option<&str>,
) -> Result<DatabaseModel, ReadError> {
    // SQL Server usa tiberius (fuera del driver Any de sqlx).
    if matches!(provider, Provider::SqlServer) {
        return read_sqlserver(url, schema.unwrap_or("dbo")).await;
    }

    install_default_drivers();
    let pool = AnyPoolOptions::new().max_connections(1).connect(url).await?;
    match provider {
        Provider::Sqlite => read_sqlite(&pool).await,
        Provider::Postgres => read_postgres(&pool, schema.unwrap_or("public")).await,
        Provider::MySql => read_mysql(&pool, schema).await,
        Provider::SqlServer => unreachable!("SQL Server se maneja antes con tiberius"),
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
    };

    // Columnas (PRAGMA table_info: cid, name, type, notnull, dflt_value, pk).
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
            is_identity: false, // se ajusta tras conocer la PK
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

        // Heurística de identidad: PK simple sobre columna INTEGER (alias de ROWID).
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
        // Las columnas de la PK son NOT NULL (SQLite reporta notnull=0 para las
        // PK INTEGER alias de ROWID, así que lo forzamos aquí).
        for pk_col in &columns {
            if let Some(col) = table.columns.iter_mut().find(|c| &c.name == pk_col) {
                col.is_nullable = false;
            }
        }
        table.primary_key = Some(PrimaryKey {
            name: format!("PK_{name}"),
            columns,
        });
    }

    // Claves foráneas (PRAGMA foreign_key_list: id, seq, table, from, to, on_update, on_delete).
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

        let fk = fks.entry(id).or_insert_with(|| ForeignKey {
            name: format!("FK_{name}_{ref_table}_{id}"),
            columns: Vec::new(),
            principal_table: ref_table,
            principal_schema: None,
            principal_columns: Vec::new(),
            on_delete: parse_action(&on_delete),
        });
        fk.columns.push(from);
        fk.principal_columns.push(to);
    }
    table.foreign_keys = fks.into_values().collect();

    // Índices explícitos (PRAGMA index_list, origin='c' = creado con CREATE INDEX).
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
        });
    }

    Ok(table)
}

/// Mapea un tipo declarado de SQLite a CLR según afinidad (regla de EF Core).
fn sqlite_clr(declared: &str) -> &'static str {
    let t = declared.to_ascii_uppercase();
    if t.contains("INT") {
        "System.Int64" // INTEGER en SQLite es de 64 bits
    } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
        "System.String"
    } else if t.contains("BLOB") || t.is_empty() {
        "System.Byte[]"
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        "System.Double"
    } else {
        // NUMERIC/DECIMAL/BOOLEAN/DATE… EF los guarda como TEXT → string por defecto.
        "System.String"
    }
}

/// Extrae `N` de un tipo declarado tipo `varchar(200)`.
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

    // Tablas.
    // Las columnas de information_schema son de tipo `name`/dominios que el
    // driver Any de sqlx no decodifica; se castean a text/int explícitamente.
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
            },
        );
    }

    // Columnas (todas de una vez, agrupadas por tabla).
    let col_rows = sqlx::query(&format!(
        "SELECT table_name::text AS table_name, column_name::text AS column_name, \
                data_type::text AS data_type, udt_name::text AS udt_name, \
                is_nullable::text AS is_nullable, \
                character_maximum_length::int AS character_maximum_length, \
                numeric_precision::int AS numeric_precision, \
                numeric_scale::int AS numeric_scale, \
                column_default::text AS column_default, \
                is_identity::text AS is_identity \
         FROM information_schema.columns WHERE table_schema = '{sc}' \
         ORDER BY table_name, ordinal_position"
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
        let is_nullable: String = r.try_get("is_nullable")?;
        let max_length: Option<i32> = r.try_get("character_maximum_length").ok().flatten();
        let precision: Option<i32> = r.try_get("numeric_precision").ok().flatten();
        let scale: Option<i32> = r.try_get("numeric_scale").ok().flatten();
        let default: Option<String> = r.try_get("column_default")?;
        let is_identity: String = r.try_get("is_identity").unwrap_or_default();

        let identity = is_identity.eq_ignore_ascii_case("YES")
            || default
                .as_deref()
                .map(|d| d.contains("nextval("))
                .unwrap_or(false);

        table.columns.push(Column {
            name: r.try_get("column_name")?,
            store_type: Some(pg_store_type(&data_type, &udt_name, max_length)),
            clr_type: Some(pg_clr(&data_type, &udt_name).to_string()),
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_identity: identity,
            max_length,
            precision,
            scale,
            // Los defaults de identidad/serial no se reproducen como literal.
            default_value_sql: if identity { None } else { default },
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        });
    }

    // Claves primarias.
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

    // Claves foráneas.
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

    model.tables = tables.into_values().collect();
    Ok(model)
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
        "character varying" | "varchar" | "character" | "char" | "text" | "citext" => {
            "System.String"
        }
        _ => match udt {
            "int2" => "System.Int16",
            "int4" => "System.Int32",
            "int8" => "System.Int64",
            "bool" => "System.Boolean",
            _ => "System.String",
        },
    }
}

fn pg_store_type(data_type: &str, udt: &str, max_length: Option<i32>) -> String {
    match (data_type, max_length) {
        ("character varying", Some(n)) => format!("character varying({n})"),
        ("character", Some(n)) => format!("character({n})"),
        ("USER-DEFINED", _) => udt.to_string(),
        (other, _) => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// MySQL / MariaDB
// ---------------------------------------------------------------------------

/// Lee una columna string de MySQL. El driver Any clasifica las columnas
/// TEXT/BLOB de information_schema como `Blob`, así que se leen como bytes.
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
    // El "esquema" en MySQL es la base de datos; si no se indica, la actual.
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

    // Tablas.
    // Las columnas string de information_schema en MySQL llegan como BLOB al
    // driver Any; se castean a CHAR explícitamente.
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
            },
        );
    }

    // Columnas (numéricos casteados a SIGNED para el driver Any).
    let col_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME, \
                CONVERT(DATA_TYPE USING utf8mb4) AS DATA_TYPE, \
                CONVERT(COLUMN_TYPE USING utf8mb4) AS COLUMN_TYPE, \
                CONVERT(IS_NULLABLE USING utf8mb4) AS IS_NULLABLE, \
                CONVERT(EXTRA USING utf8mb4) AS EXTRA, \
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

        let identity = extra.to_ascii_lowercase().contains("auto_increment");
        let max_length = match data_type.as_str() {
            "varchar" | "char" | "varbinary" | "binary" => {
                max_len.filter(|n| *n > 0 && *n <= i32::MAX as i64).map(|n| n as i32)
            }
            _ => None,
        };

        table.columns.push(Column {
            name: my_str(&r, "COLUMN_NAME")?,
            store_type: Some(if column_type.is_empty() { data_type.clone() } else { column_type }),
            clr_type: Some(mysql_clr(&data_type).to_string()),
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_identity: identity,
            max_length,
            precision: precision.map(|n| n as i32),
            scale: scale.map(|n| n as i32),
            default_value_sql: if identity { None } else { default },
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        });
    }

    // Claves primarias (en MySQL la PK se llama 'PRIMARY').
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
            name: format!("PK_{table_name}"),
            columns: Vec::new(),
        });
        pk.columns.push(column);
    }

    // Claves foráneas.
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

    // Índices (excluyendo la PK).
    let idx_rows = sqlx::query(&format!(
        "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
                CONVERT(INDEX_NAME USING utf8mb4) AS INDEX_NAME, \
                CAST(NON_UNIQUE AS SIGNED) AS NON_UNIQUE, \
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
        match table.indexes.iter_mut().find(|i| i.name == name) {
            Some(ix) => ix.columns.push(column),
            None => table.indexes.push(Index {
                name,
                columns: vec![column],
                is_unique: non_unique == 0,
                filter: None,
            }),
        }
    }

    model.tables = tables.into_values().collect();
    Ok(model)
}

// ---------------------------------------------------------------------------
// SQL Server (vía tiberius)
// ---------------------------------------------------------------------------

async fn ss_query(client: &mut SsClient, sql: &str) -> Result<Vec<tiberius::Row>, ReadError> {
    Ok(client.simple_query(sql.to_string()).await?.into_first_result().await?)
}

async fn read_sqlserver(url: &str, schema: &str) -> Result<DatabaseModel, ReadError> {
    let config = tiberius::Config::from_ado_string(url)?;
    let tcp = tokio::net::TcpStream::connect(config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let mut client = tiberius::Client::connect(config, tcp.compat_write()).await?;

    let sc = escape_literal(schema);
    let mut model = DatabaseModel::empty();
    model.default_schema = Some(schema.to_string());

    // Tablas de usuario (excluye system y la tabla de historial).
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
            },
        );
    }

    // Columnas.
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

    // Claves primarias.
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
        let constraint = r.get::<&str, _>("constraint_name").unwrap_or("").to_string();
        let column = r.get::<&str, _>("column_name").unwrap_or("").to_string();
        let pk = table.primary_key.get_or_insert_with(|| PrimaryKey {
            name: constraint,
            columns: Vec::new(),
        });
        pk.columns.push(column);
    }

    // Claves foráneas (sys.*).
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
        // SQL Server usa NO_ACTION/SET_NULL… con guion bajo.
        let delete_rule = r.get::<&str, _>("delete_rule").unwrap_or("").replace('_', " ");

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

    // Índices (excluyendo la PK).
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
            }),
        }
    }

    model.tables = tables.into_values().collect();
    Ok(model)
}

fn ss_store_type(data_type: &str, max_len: Option<i32>, prec: Option<i32>, scale: Option<i32>) -> String {
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
