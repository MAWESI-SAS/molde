//! End-to-end round-trip against MySQL (ignored by default: requires a DB).
//!
//! Reads the schema of `SRC_DATABASE_URL` → IR, recreates it in `DST_DATABASE_URL`
//! via the diff operations emitted by `MySqlGenerator`, reads the target back
//! and verifies that the generated columns and the FULLTEXT indexes are
//! preserved.
//!
//! Run:
//! ```text
//! SRC_DATABASE_URL=mysql://… DST_DATABASE_URL=mysql://… \
//!   cargo test -p molde-scaffold --test roundtrip_mysql -- --ignored --nocapture
//! ```

use molde_core::diff::diff;
use molde_core::model::DatabaseModel;
use molde_providers::{MySqlGenerator, Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};

#[tokio::test]
#[ignore = "requires SRC_DATABASE_URL and DST_DATABASE_URL (MySQL)"]
async fn round_trip_mysql_search() {
    let src = std::env::var("SRC_DATABASE_URL").expect("SRC_DATABASE_URL");
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    let mut src_model = molde_scaffold::reader::read_model(&src, Provider::MySql, None)
        .await
        .expect("reading SRC");
    src_model.normalize();

    // Source sanity check: there is a generated column and a FULLTEXT index.
    let st = &src_model.tables[0];
    assert!(
        st.columns
            .iter()
            .any(|c| c.computed_sql.is_some() && c.computed_stored),
        "the source must have a STORED generated column"
    );
    assert!(
        st.indexes
            .iter()
            .any(|i| i.method.as_deref() == Some("fulltext")),
        "the source must have a FULLTEXT index"
    );

    // Recreate in the target.
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("connecting to DST");
    let gen = MySqlGenerator::new();
    for op in &diff(&DatabaseModel::empty(), &src_model) {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("applying:\n{stmt}\nerror: {e}"));
        }
    }

    let mut dst_model = molde_scaffold::reader::read_model(&dst, Provider::MySql, None)
        .await
        .expect("reading DST");
    dst_model.normalize();

    let dt = &dst_model.tables[0];
    assert_eq!(
        st.columns, dt.columns,
        "columns (incl. generated) preserved"
    );
    assert_eq!(st.indexes, dt.indexes, "indexes (incl. FULLTEXT) preserved");
}
