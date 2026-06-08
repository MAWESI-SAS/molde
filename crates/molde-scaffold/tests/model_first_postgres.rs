//! End-to-end model-first cycle against Postgres (ignored by default: requires a DB).
//!
//! Starts from hand-written `.model` files (the molde language), parses them to IR
//! with `molde-lang`, derives the operations with `diff(empty, ir)`, applies them to
//! the target via `PostgresGenerator`, reads the real schema back and verifies that
//! it is equivalent to the declared model. Closes the loop `.model` → migration → apply
//! 100% in Rust, without .NET.
//!
//! Run:
//! ```text
//! DST_DATABASE_URL=postgres://… \
//!   cargo test -p molde-scaffold --test model_first_postgres -- --ignored --nocapture
//! ```

use molde_core::diff::diff;
use molde_core::model::DatabaseModel;
use molde_providers::{PostgresGenerator, Provider, SqlGenerator};
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
#[ignore = "requires DST_DATABASE_URL (Postgres)"]
async fn model_first_cycle_postgres() {
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    // 1. `.model` (molde) → IR.
    let files: Vec<(&str, &str)> = vec![
        ("database.model", DATABASE_MODEL),
        ("Customer.model", CUSTOMER_MODEL),
        ("Order.model", ORDER_MODEL),
    ];
    let mut model = molde_lang::parse_project(&files).expect("parsing .model");
    model.normalize();

    // 2. Apply to the target: diff operations against an empty schema.
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("connecting to DST");

    // Clean slate: the test must be re-runnable against the same DB.
    for stmt in ["DROP SCHEMA public CASCADE", "CREATE SCHEMA public"] {
        sqlx::query(stmt)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("resetting schema:\n{stmt}\nerror: {e}"));
    }

    let ops = diff(&DatabaseModel::empty(), &model);
    let gen = PostgresGenerator::new();
    for op in &ops {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("applying:\n{stmt}\nerror: {e}"));
        }
    }

    // 3. Read the real schema back and canonicalize it to the logical form (like the
    //    scaffold): this way the engine types (`integer`, `text`, `varchar(200)`…)
    //    map back to logical types and the comparison is against the declared `.model`.
    let mut back = molde_scaffold::reader::read_model(&dst, Provider::Postgres, Some("public"))
        .await
        .expect("reading DST back");
    back.normalize();
    molde_scaffold::canonicalize_for_models(&mut back);
    back.normalize();

    let declared = table(&model, "Customer");
    let actual = table(&back, "Customer");
    assert_eq!(declared.columns, actual.columns, "Customer columns");
    assert_eq!(
        declared.primary_key, actual.primary_key,
        "Customer identity PK"
    );
    assert!(
        actual.indexes.iter().any(|ix| ix.is_unique),
        "Customer.Email unique index"
    );

    let dorder = table(&model, "Order");
    let aorder = table(&back, "Order");
    assert_eq!(dorder.columns, aorder.columns, "Order columns");
    assert_eq!(
        dorder.foreign_keys, aorder.foreign_keys,
        "Order → Customer cascade FK"
    );
}

fn table<'a>(m: &'a DatabaseModel, name: &str) -> &'a molde_core::model::Table {
    m.tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("table '{name}' missing"))
}
