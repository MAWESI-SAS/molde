//! # molde-scaffold
//!
//! Database-first: reads the schema of an existing database and generates the
//! `.model` files (molde language).
//!
//! - [`reader`]: DB catalog → [`molde_core::DatabaseModel`] (per engine).
//! - [`build_model_files`]: canonicalized model → `.model` files.

pub mod reader;

pub use molde_lang::ModelFile;
pub use reader::ReadError;

use molde_core::model::{Column, DatabaseModel};
use molde_providers::Provider;

/// Database-first pipeline toward the molde language: connects, reads the model,
/// canonicalizes it for clean output and emits the `.model` files.
pub async fn build_model_files(
    url: &str,
    provider: Provider,
    schema: Option<&str>,
) -> Result<Vec<ModelFile>, ReadError> {
    let mut model = reader::read_model(url, provider, schema).await?;
    canonicalize_for_models(&mut model);
    Ok(molde_lang::emit_project(&model))
}

/// Reduces the noise of the `.model` generated from a real DB:
/// - drops the exact `store_type` when it is the conventional one for the logical
///   type (it is re-derived on apply; exotic types like jsonb/vector keep it
///   via `dbtype=`), just as the C# scaffold omits `HasColumnType`;
/// - drops each table's `schema` when it matches the default schema.
pub fn canonicalize_for_models(model: &mut DatabaseModel) {
    let default_schema = model.default_schema.clone();
    for t in &mut model.tables {
        if t.schema == default_schema {
            t.schema = None;
        }
        for c in &mut t.columns {
            if c.clr_type.is_some() && exotic_store_type(c).is_none() {
                c.store_type = None;
            }
            // precision/scale only apply to decimals; on integers, Postgres
            // exposes numeric_precision (bits) which is noise here.
            if c.clr_type.as_deref() != Some("System.Decimal") {
                c.precision = None;
                c.scale = None;
            }
        }
        for fk in &mut t.foreign_keys {
            if fk.principal_schema == default_schema {
                fk.principal_schema = None;
            }
        }
    }
}

/// Returns the `store_type` only if it is NOT conventional for its logical type
/// (jsonb, arrays, citext, vector(N), tsvector, inet…). Conventional types
/// (varchar, integer, numeric, uuid, timestamp…) are derived from the logical
/// type on apply, so they are omitted in the `.model`.
fn exotic_store_type(col: &Column) -> Option<&str> {
    let st = col.store_type.as_deref()?;
    let base = st
        .split('(')
        .next()
        .unwrap_or(st)
        .trim()
        .to_ascii_lowercase();
    const CONVENTIONAL: &[&str] = &[
        "character varying",
        "varchar",
        "text",
        "char",
        "character",
        "nvarchar",
        "nchar",
        "integer",
        "int",
        "bigint",
        "smallint",
        "tinyint",
        "int2",
        "int4",
        "int8",
        "boolean",
        "bool",
        "bit",
        "real",
        "double precision",
        "float",
        "double",
        "numeric",
        "decimal",
        "money",
        "smallmoney",
        "uuid",
        "uniqueidentifier",
        "date",
        "timestamp",
        "timestamp without time zone",
        "timestamp with time zone",
        "timestamptz",
        "datetime",
        "datetime2",
        "datetimeoffset",
        "time",
        "time without time zone",
        "bytea",
        "varbinary",
        "binary",
        "blob",
        "image",
    ];
    if CONVENTIONAL.contains(&base.as_str()) {
        None
    } else {
        Some(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use molde_core::diff::Operation;
    use molde_core::model::{Column, PrimaryKey, Table};
    use molde_providers::SqlGenerator;
    use sqlx::any::{install_default_drivers, AnyPoolOptions};

    fn col(name: &str, clr: &str, nullable: bool, identity: bool) -> Column {
        Column {
            name: name.into(),
            store_type: None,
            clr_type: Some(clr.into()),
            is_nullable: nullable,
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

    #[test]
    fn canonicalize_cleans_noise_and_keeps_exotics() {
        use molde_core::model::DatabaseModel;
        let mut m = DatabaseModel::empty();
        m.default_schema = Some("public".into());

        let mut id = col("Id", "System.Int32", false, true);
        id.precision = Some(32); // Postgres noise for integers
        id.scale = Some(0);
        let mut name = col("Name", "System.String", false, false);
        name.store_type = Some("character varying(200)".into()); // conventional → dropped
        name.max_length = Some(200);
        let mut total = col("Total", "System.Decimal", false, false);
        total.store_type = Some("numeric(18,2)".into()); // conventional → dropped
        total.precision = Some(18);
        total.scale = Some(2); // decimal → kept
        let mut meta = col("Meta", "System.String", true, false);
        meta.store_type = Some("jsonb".into()); // exotic → kept as dbtype

        m.tables.push(Table {
            name: "Customer".into(),
            schema: Some("public".into()), // == default → dropped
            clr_type: None,
            comment: None,
            columns: vec![id, name, total, meta],
            primary_key: None,
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
            seed_key: vec![],
        });

        canonicalize_for_models(&mut m);
        let t = &m.tables[0];
        assert_eq!(t.schema, None);
        let id = t.column("Id").unwrap();
        assert_eq!(id.precision, None, "integer precision is noise");
        let name = t.column("Name").unwrap();
        assert_eq!(name.store_type, None, "conventional varchar is dropped");
        assert_eq!(name.max_length, Some(200));
        let total = t.column("Total").unwrap();
        assert_eq!(total.store_type, None, "conventional numeric is dropped");
        assert_eq!(total.precision, Some(18), "decimal precision is kept");
        assert_eq!(total.scale, Some(2));
        let meta = t.column("Meta").unwrap();
        assert_eq!(meta.store_type.as_deref(), Some("jsonb"), "exotic is kept");
    }

    /// Real round-trip: creates a table in SQLite with the provider, reads it with
    /// the reader and generates C#. Exercises providers + reader + codegen together.
    #[tokio::test]
    async fn round_trip_sqlite() {
        let path = std::env::temp_dir().join(format!("molde_scaffold_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        // 1. Create the schema with the SQLite provider.
        let table = Table {
            name: "Customer".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: vec![
                col("Id", "System.Int32", false, true),
                col("Name", "System.String", false, false),
                col("Email", "System.String", true, false),
            ],
            primary_key: Some(PrimaryKey {
                name: "pk_customer".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
            seed_key: vec![],
        };

        install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();

        let ddl = molde_providers::SqliteGenerator::new()
            .emit(&Operation::CreateTable { table })
            .unwrap();
        for stmt in ddl {
            sqlx::query(&stmt).execute(&pool).await.unwrap();
        }
        sqlx::query("CREATE UNIQUE INDEX ix_customer_email ON \"Customer\" (\"Email\");")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        // 2. Read the model back.
        let model = reader::read_model(&url, Provider::Sqlite, None)
            .await
            .unwrap();
        let customer = model.table(None, "Customer").expect("Customer table");
        assert_eq!(customer.columns.len(), 3);
        assert!(
            customer.column("Id").unwrap().is_identity,
            "Id must be identity"
        );
        assert!(!customer.column("Name").unwrap().is_nullable);
        assert!(customer.column("Email").unwrap().is_nullable);
        assert_eq!(customer.primary_key.as_ref().unwrap().columns, vec!["Id"]);
        let idx = customer
            .indexes
            .iter()
            .find(|i| i.name == "ix_customer_email")
            .expect("index read");
        assert!(idx.is_unique);

        // 3. Emit `.model` (canonicalized) and check the molde output.
        let mut canon = model.clone();
        canonicalize_for_models(&mut canon);
        let files = molde_lang::emit_project(&canon);
        let entity = &files
            .iter()
            .find(|f| f.name == "Customer.model")
            .unwrap()
            .contents;
        assert!(entity.contains("Id: long")); // INTEGER → long in SQLite
        assert!(entity.contains("Name: string"));
        assert!(entity.contains("Email: string? unique"));

        let _ = std::fs::remove_file(&path);
    }

    /// Real rebuild in SQLite: a column type change on a table with data is
    /// applied via RebuildTable while preserving the rows.
    #[tokio::test]
    async fn sqlite_rebuild_preserves_data() {
        use molde_core::diff::diff;
        use molde_core::model::DatabaseModel;
        use sqlx::Row;

        let path = std::env::temp_dir().join(format!("molde_rebuild_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let person = |age_clr: &str| Table {
            name: "Person".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: vec![
                col("Id", "System.Int32", false, true),
                col("Age", age_clr, true, false),
            ],
            primary_key: Some(PrimaryKey {
                name: "pk_person".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
            seed_key: vec![],
        };
        let mut old = DatabaseModel::empty();
        old.tables.push(person("System.String")); // Age TEXT
        let mut new = DatabaseModel::empty();
        new.tables.push(person("System.Int64")); // Age INTEGER

        install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let gen = molde_providers::SqliteGenerator::new();

        // Create the old table + one row.
        for stmt in gen
            .emit(&Operation::CreateTable {
                table: old.tables[0].clone(),
            })
            .unwrap()
        {
            sqlx::query(&stmt).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO \"Person\" (\"Age\") VALUES ('42');")
            .execute(&pool)
            .await
            .unwrap();

        // Apply the diff (includes RebuildTable).
        let ops = diff(&old, &new);
        assert!(ops
            .iter()
            .any(|o| matches!(o, Operation::RebuildTable { .. })));
        for stmt in gen.emit_all(&ops).unwrap() {
            sqlx::query(&stmt).execute(&pool).await.unwrap();
        }

        // The row survives the rebuild.
        let row = sqlx::query("SELECT COUNT(*) AS n FROM \"Person\";")
            .fetch_one(&pool)
            .await
            .unwrap();
        let n: i64 = row.try_get("n").unwrap();
        assert_eq!(n, 1, "the row must survive the rebuild");
        pool.close().await;

        // The re-read model has the rebuilt table with 2 columns.
        let model = reader::read_model(&url, Provider::Sqlite, None)
            .await
            .unwrap();
        assert_eq!(model.table(None, "Person").unwrap().columns.len(), 2);

        let _ = std::fs::remove_file(&path);
    }
}
