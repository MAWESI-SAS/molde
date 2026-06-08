//! End-to-end round-trip against Postgres + pgvector (ignored by default: requires a DB).
//!
//! Reads the schema of `SRC_DATABASE_URL` → IR, recreates it in `DST_DATABASE_URL`
//! via the diff operations emitted by `PostgresGenerator`, reads the target
//! back and verifies that the model is equivalent. Covers generated vector and
//! tsvector columns, HNSW/GIN indexes (including expression-based), functions and
//! triggers.
//!
//! Run:
//! ```text
//! SRC_DATABASE_URL=postgres://… DST_DATABASE_URL=postgres://… \
//!   cargo test -p molde-scaffold --test roundtrip_postgres -- --ignored --nocapture
//! ```

use molde_core::diff::diff;
use molde_core::model::DatabaseModel;
use molde_providers::{PostgresGenerator, Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};

#[tokio::test]
#[ignore = "requires SRC_DATABASE_URL and DST_DATABASE_URL (Postgres + pgvector)"]
async fn round_trip_postgres_search_objects() {
    let src = std::env::var("SRC_DATABASE_URL").expect("SRC_DATABASE_URL");
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    // 1. Read the source model.
    let mut src_model =
        molde_scaffold::reader::read_model(&src, Provider::Postgres, Some("public"))
            .await
            .expect("reading SRC");
    src_model.normalize();

    // 2. Recreate in the target: extension + diff operations (safe order).
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("connecting to DST");

    // The `vector` extension is NOT created by hand: the diff must prepend
    // EnsureExtension and the PostgresGenerator must emit the CREATE EXTENSION.
    let ops = diff(&DatabaseModel::empty(), &src_model);
    assert!(
        matches!(
            ops.first(),
            Some(molde_core::diff::Operation::EnsureExtension { .. })
        ),
        "expected EnsureExtension at the start of the diff"
    );
    let gen = PostgresGenerator::new();
    for op in &ops {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("applying:\n{stmt}\nerror: {e}"));
        }
    }

    // 3. Read the target back and compare.
    let mut dst_model =
        molde_scaffold::reader::read_model(&dst, Provider::Postgres, Some("public"))
            .await
            .expect("reading DST");
    dst_model.normalize();

    let st = src_model.tables[0].clone();
    let dt = dst_model.tables[0].clone();
    assert_eq!(
        st.columns, dt.columns,
        "columns (generated vector/tsvector)"
    );
    assert_eq!(
        st.indexes, dt.indexes,
        "indexes (hnsw/gin/expression/partial)"
    );
    assert_eq!(st.triggers, dt.triggers, "triggers");
    assert_eq!(src_model.functions, dst_model.functions, "functions");
    assert_eq!(src_model, dst_model, "full model equivalent");
}
