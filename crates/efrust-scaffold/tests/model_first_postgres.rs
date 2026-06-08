//! Ciclo model-first e2e contra Postgres (ignorado por defecto: requiere BD).
//!
//! Parte de archivos `.model` escritos a mano (el lenguaje EFM), los parsea a IR
//! con `efrust-lang`, deriva las operaciones con `diff(empty, ir)`, las aplica al
//! destino vía `PostgresGenerator`, vuelve a leer el esquema real y verifica que
//! es equivalente al modelo declarado. Cierra el lazo `.model` → migración → apply
//! 100% en Rust, sin .NET.
//!
//! Ejecutar:
//! ```text
//! DST_DATABASE_URL=postgres://… \
//!   cargo test -p efrust-scaffold --test model_first_postgres -- --ignored --nocapture
//! ```

use efrust_core::diff::diff;
use efrust_core::model::DatabaseModel;
use efrust_providers::{PostgresGenerator, Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};

const DATABASE_MODEL: &str = "\
schema: public
";

const CUSTOMER_MODEL: &str = "\
Customer:
  fields:
    Id: int pk identity
    Email: string unique maxlen=200
    DisplayName: string?
    CreatedAt: datetime default=now()
";

const ORDER_MODEL: &str = "\
Order:
  fields:
    Id: int pk identity
    CustomerId: int
    Total: decimal precision=18,2
  belongs-to:
    customer: {fk: [CustomerId], references: Customer.Id, onDelete: cascade}
";

#[tokio::test]
#[ignore = "requiere DST_DATABASE_URL (Postgres)"]
async fn ciclo_model_first_postgres() {
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    // 1. `.model` (EFM) → IR.
    let files: Vec<(&str, &str)> = vec![
        ("database.model", DATABASE_MODEL),
        ("Customer.model", CUSTOMER_MODEL),
        ("Order.model", ORDER_MODEL),
    ];
    let mut model = efrust_lang::parse_project(&files).expect("parseando .model");
    model.normalize();

    // 2. Aplicar al destino: operaciones del diff contra un esquema vacío.
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("conectando DST");

    // Slate limpio: el test debe poder re-ejecutarse contra la misma BD.
    for stmt in ["DROP SCHEMA public CASCADE", "CREATE SCHEMA public"] {
        sqlx::query(stmt)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("reseteando schema:\n{stmt}\nerror: {e}"));
    }

    let ops = diff(&DatabaseModel::empty(), &model);
    let gen = PostgresGenerator::new();
    for op in &ops {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("aplicando:\n{stmt}\nerror: {e}"));
        }
    }

    // 3. Releer el esquema real y canonicalizarlo a la forma lógica (igual que el
    //    scaffold): así los tipos del motor (`integer`, `text`, `varchar(200)`…)
    //    vuelven a tipos lógicos y la comparación es contra el `.model` declarado.
    let mut back = efrust_scaffold::reader::read_model(&dst, Provider::Postgres, Some("public"))
        .await
        .expect("releyendo DST");
    back.normalize();
    efrust_scaffold::canonicalize_for_models(&mut back);
    back.normalize();

    let declared = table(&model, "Customer");
    let actual = table(&back, "Customer");
    assert_eq!(declared.columns, actual.columns, "columnas de Customer");
    assert_eq!(
        declared.primary_key, actual.primary_key,
        "PK identity de Customer"
    );
    assert!(
        actual.indexes.iter().any(|ix| ix.is_unique),
        "índice único de Customer.Email"
    );

    let dorder = table(&model, "Order");
    let aorder = table(&back, "Order");
    assert_eq!(dorder.columns, aorder.columns, "columnas de Order");
    assert_eq!(
        dorder.foreign_keys, aorder.foreign_keys,
        "FK cascade de Order → Customer"
    );
}

fn table<'a>(m: &'a DatabaseModel, name: &str) -> &'a efrust_core::model::Table {
    m.tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("tabla '{name}' ausente"))
}
