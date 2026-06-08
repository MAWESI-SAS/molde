//! Round-trip e2e contra MySQL (ignorado por defecto: requiere BD).
//!
//! Lee el esquema de `SRC_DATABASE_URL` → IR, lo recrea en `DST_DATABASE_URL`
//! vía las operaciones del diff emitidas por `MySqlGenerator`, vuelve a leer el
//! destino y verifica que se preservan las columnas generadas y los índices
//! FULLTEXT.
//!
//! Ejecutar:
//! ```text
//! SRC_DATABASE_URL=mysql://… DST_DATABASE_URL=mysql://… \
//!   cargo test -p molde-scaffold --test roundtrip_mysql -- --ignored --nocapture
//! ```

use molde_core::diff::diff;
use molde_core::model::DatabaseModel;
use molde_providers::{MySqlGenerator, Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};

#[tokio::test]
#[ignore = "requiere SRC_DATABASE_URL y DST_DATABASE_URL (MySQL)"]
async fn round_trip_mysql_busqueda() {
    let src = std::env::var("SRC_DATABASE_URL").expect("SRC_DATABASE_URL");
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    let mut src_model = molde_scaffold::reader::read_model(&src, Provider::MySql, None)
        .await
        .expect("leyendo SRC");
    src_model.normalize();

    // Sanidad del origen: hay una columna generada y un índice FULLTEXT.
    let st = &src_model.tables[0];
    assert!(
        st.columns
            .iter()
            .any(|c| c.computed_sql.is_some() && c.computed_stored),
        "el origen debe tener una columna generada STORED"
    );
    assert!(
        st.indexes
            .iter()
            .any(|i| i.method.as_deref() == Some("fulltext")),
        "el origen debe tener un índice FULLTEXT"
    );

    // Recrear en destino.
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("conectando DST");
    let gen = MySqlGenerator::new();
    for op in &diff(&DatabaseModel::empty(), &src_model) {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("aplicando:\n{stmt}\nerror: {e}"));
        }
    }

    let mut dst_model = molde_scaffold::reader::read_model(&dst, Provider::MySql, None)
        .await
        .expect("leyendo DST");
    dst_model.normalize();

    let dt = &dst_model.tables[0];
    assert_eq!(
        st.columns, dt.columns,
        "columnas (incl. generada) preservadas"
    );
    assert_eq!(
        st.indexes, dt.indexes,
        "índices (incl. FULLTEXT) preservados"
    );
}
