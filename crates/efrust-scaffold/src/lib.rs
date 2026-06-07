//! # efrust-scaffold
//!
//! Database-first: lee el esquema de una base de datos existente y genera los
//! modelos C# + `DbContext`.
//!
//! - [`reader`]: catálogo de la BD → [`efrust_core::DatabaseModel`] (por motor).
//! - [`codegen`]: modelo → archivos C# (puro, sin BD).
//! - [`csharp`]: utilidades de mapeo de tipos y nombres.
//!
//! Salida nueva: archivos `.model` (lenguaje EFM) vía [`build_model_files`]. El
//! C# queda como salida legacy ([`build_files`]) hasta su retiro.

pub mod codegen;
pub mod csharp;
pub mod reader;

pub use codegen::{CodegenOptions, GeneratedFile};
pub use efrust_lang::ModelFile;
pub use reader::ReadError;

use efrust_core::model::DatabaseModel;
use efrust_providers::Provider;

/// Pipeline completo: conecta, lee el modelo y genera los archivos C#.
pub async fn build_files(
    url: &str,
    provider: Provider,
    schema: Option<&str>,
    opts: &CodegenOptions,
) -> Result<Vec<GeneratedFile>, ReadError> {
    let model = reader::read_model(url, provider, schema).await?;
    // El provider de origen manda sobre el de las opciones (idioms por motor).
    let mut opts = opts.clone();
    opts.provider = provider;
    Ok(codegen::generate(&model, &opts))
}

/// Pipeline database-first hacia el lenguaje EFM: conecta, lee el modelo, lo
/// canoniza para una salida limpia y emite los archivos `.model`.
pub async fn build_model_files(
    url: &str,
    provider: Provider,
    schema: Option<&str>,
) -> Result<Vec<ModelFile>, ReadError> {
    let mut model = reader::read_model(url, provider, schema).await?;
    canonicalize_for_models(&mut model);
    Ok(efrust_lang::emit_project(&model))
}

/// Reduce el ruido del `.model` generado desde una BD real:
/// - quita el `store_type` exacto cuando es el convencional para el tipo lógico
///   (se vuelve a derivar al aplicar; los tipos exóticos como jsonb/vector lo
///   conservan vía `dbtype=`), igual que el scaffold C# omite `HasColumnType`;
/// - quita el `schema` de cada tabla cuando coincide con el esquema por defecto.
pub fn canonicalize_for_models(model: &mut DatabaseModel) {
    let default_schema = model.default_schema.clone();
    for t in &mut model.tables {
        if t.schema == default_schema {
            t.schema = None;
        }
        for c in &mut t.columns {
            if c.clr_type.is_some() && codegen::exotic_store_type(c).is_none() {
                c.store_type = None;
            }
            // precision/scale solo aplican a decimales; en enteros, Postgres
            // expone numeric_precision (bits) que aquí es ruido.
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

#[cfg(test)]
mod tests {
    use super::*;
    use efrust_core::diff::Operation;
    use efrust_core::model::{Column, PrimaryKey, Table};
    use efrust_providers::SqlGenerator;
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
    fn canonicalize_limpia_ruido_y_conserva_exoticos() {
        use efrust_core::model::DatabaseModel;
        let mut m = DatabaseModel::empty();
        m.default_schema = Some("public".into());

        let mut id = col("Id", "System.Int32", false, true);
        id.precision = Some(32); // ruido de Postgres para enteros
        id.scale = Some(0);
        let mut name = col("Name", "System.String", false, false);
        name.store_type = Some("character varying(200)".into()); // convencional → se quita
        name.max_length = Some(200);
        let mut total = col("Total", "System.Decimal", false, false);
        total.store_type = Some("numeric(18,2)".into()); // convencional → se quita
        total.precision = Some(18);
        total.scale = Some(2); // decimal → se conserva
        let mut meta = col("Meta", "System.String", true, false);
        meta.store_type = Some("jsonb".into()); // exótico → se conserva como dbtype

        m.tables.push(Table {
            name: "Customer".into(),
            schema: Some("public".into()), // == default → se quita
            clr_type: None,
            comment: None,
            columns: vec![id, name, total, meta],
            primary_key: None,
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
        });

        canonicalize_for_models(&mut m);
        let t = &m.tables[0];
        assert_eq!(t.schema, None);
        let id = t.column("Id").unwrap();
        assert_eq!(id.precision, None, "precision de entero es ruido");
        let name = t.column("Name").unwrap();
        assert_eq!(name.store_type, None, "varchar convencional se quita");
        assert_eq!(name.max_length, Some(200));
        let total = t.column("Total").unwrap();
        assert_eq!(total.store_type, None, "numeric convencional se quita");
        assert_eq!(
            total.precision,
            Some(18),
            "precision de decimal se conserva"
        );
        assert_eq!(total.scale, Some(2));
        let meta = t.column("Meta").unwrap();
        assert_eq!(
            meta.store_type.as_deref(),
            Some("jsonb"),
            "exótico se conserva"
        );
    }

    /// Round-trip real: crea una tabla en SQLite con el provider, la lee con el
    /// reader y genera C#. Ejercita providers + reader + codegen juntos.
    #[tokio::test]
    async fn round_trip_sqlite() {
        let path = std::env::temp_dir().join(format!("efrust_scaffold_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        // 1. Crear esquema con el provider de SQLite.
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
                name: "PK_Customer".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
        };

        install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();

        let ddl = efrust_providers::SqliteGenerator::new()
            .emit(&Operation::CreateTable { table })
            .unwrap();
        for stmt in ddl {
            sqlx::query(&stmt).execute(&pool).await.unwrap();
        }
        sqlx::query("CREATE UNIQUE INDEX IX_Customer_Email ON \"Customer\" (\"Email\");")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        // 2. Leer el modelo de vuelta.
        let model = reader::read_model(&url, Provider::Sqlite, None)
            .await
            .unwrap();
        let customer = model.table(None, "Customer").expect("tabla Customer");
        assert_eq!(customer.columns.len(), 3);
        assert!(
            customer.column("Id").unwrap().is_identity,
            "Id debe ser identidad"
        );
        assert!(!customer.column("Name").unwrap().is_nullable);
        assert!(customer.column("Email").unwrap().is_nullable);
        assert_eq!(customer.primary_key.as_ref().unwrap().columns, vec!["Id"]);
        let idx = customer
            .indexes
            .iter()
            .find(|i| i.name == "IX_Customer_Email")
            .expect("índice leído");
        assert!(idx.is_unique);

        // 3. Generar C#.
        let files = codegen::generate(&model, &CodegenOptions::default());
        let entity = &files
            .iter()
            .find(|f| f.relative_path == "Customer.cs")
            .unwrap()
            .contents;
        assert!(entity.contains("public partial class Customer"));
        assert!(entity.contains("public long Id { get; set; }")); // INTEGER → long en SQLite
        assert!(entity.contains("public string Name { get; set; } = null!;"));
        assert!(entity.contains("public string? Email { get; set; }"));

        let _ = std::fs::remove_file(&path);
    }

    /// Rebuild real en SQLite: un cambio de tipo de columna sobre una tabla con
    /// datos se aplica vía RebuildTable preservando las filas.
    #[tokio::test]
    async fn sqlite_rebuild_preserva_datos() {
        use efrust_core::diff::diff;
        use efrust_core::model::DatabaseModel;
        use sqlx::Row;

        let path = std::env::temp_dir().join(format!("efrust_rebuild_{}.db", std::process::id()));
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
                name: "PK_Person".into(),
                columns: vec!["Id".into()],
            }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
            seed_data: vec![],
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
        let gen = efrust_providers::SqliteGenerator::new();

        // Crear la tabla vieja + una fila.
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

        // Aplicar el diff (incluye RebuildTable).
        let ops = diff(&old, &new);
        assert!(ops
            .iter()
            .any(|o| matches!(o, Operation::RebuildTable { .. })));
        for stmt in gen.emit_all(&ops).unwrap() {
            sqlx::query(&stmt).execute(&pool).await.unwrap();
        }

        // La fila sobrevive a la reconstrucción.
        let row = sqlx::query("SELECT COUNT(*) AS n FROM \"Person\";")
            .fetch_one(&pool)
            .await
            .unwrap();
        let n: i64 = row.try_get("n").unwrap();
        assert_eq!(n, 1, "la fila debe sobrevivir al rebuild");
        pool.close().await;

        // El modelo releído tiene la tabla reconstruida con 2 columnas.
        let model = reader::read_model(&url, Provider::Sqlite, None)
            .await
            .unwrap();
        assert_eq!(model.table(None, "Person").unwrap().columns.len(), 2);

        let _ = std::fs::remove_file(&path);
    }
}
