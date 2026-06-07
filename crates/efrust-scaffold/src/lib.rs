//! # efrust-scaffold
//!
//! Database-first: lee el esquema de una base de datos existente y genera los
//! modelos C# + `DbContext`.
//!
//! - [`reader`]: catálogo de la BD → [`efrust_core::DatabaseModel`] (por motor).
//! - [`codegen`]: modelo → archivos C# (puro, sin BD).
//! - [`csharp`]: utilidades de mapeo de tipos y nombres.

pub mod codegen;
pub mod csharp;
pub mod reader;

pub use codegen::{CodegenOptions, GeneratedFile};
pub use reader::ReadError;

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
