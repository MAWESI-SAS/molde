//! Round-trip e2e contra Postgres + pgvector (ignorado por defecto: requiere BD).
//!
//! Lee el esquema de `SRC_DATABASE_URL` → IR, lo recrea en `DST_DATABASE_URL`
//! vía las operaciones del diff emitidas por `PostgresGenerator`, vuelve a leer
//! el destino y verifica que el modelo es equivalente. Cubre columnas vector y
//! tsvector generadas, índices HNSW/GIN (incluido por expresión), funciones y
//! triggers.
//!
//! Ejecutar:
//! ```text
//! SRC_DATABASE_URL=postgres://… DST_DATABASE_URL=postgres://… \
//!   cargo test -p efrust-scaffold --test roundtrip_postgres -- --ignored --nocapture
//! ```

use efrust_core::diff::diff;
use efrust_core::model::DatabaseModel;
use efrust_providers::{PostgresGenerator, Provider, SqlGenerator};
use sqlx::any::{install_default_drivers, AnyPoolOptions};

#[tokio::test]
#[ignore = "requiere SRC_DATABASE_URL y DST_DATABASE_URL (Postgres + pgvector)"]
async fn round_trip_postgres_objetos_de_busqueda() {
    let src = std::env::var("SRC_DATABASE_URL").expect("SRC_DATABASE_URL");
    let dst = std::env::var("DST_DATABASE_URL").expect("DST_DATABASE_URL");

    // 1. Leer el modelo de origen.
    let mut src_model =
        efrust_scaffold::reader::read_model(&src, Provider::Postgres, Some("public"))
            .await
            .expect("leyendo SRC");
    src_model.normalize();

    // 2. Recrear en destino: extensión + operaciones del diff (orden seguro).
    install_default_drivers();
    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(&dst)
        .await
        .expect("conectando DST");

    // La extensión `vector` NO se crea a mano: el diff debe anteponer
    // EnsureExtension y el PostgresGenerator emitir el CREATE EXTENSION.
    let ops = diff(&DatabaseModel::empty(), &src_model);
    assert!(
        matches!(ops.first(), Some(efrust_core::diff::Operation::EnsureExtension { .. })),
        "se esperaba EnsureExtension al inicio del diff"
    );
    let gen = PostgresGenerator::new();
    for op in &ops {
        for stmt in gen.emit(op).expect("emit SQL") {
            sqlx::query(&stmt)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("aplicando:\n{stmt}\nerror: {e}"));
        }
    }

    // 3. Releer el destino y comparar.
    let mut dst_model = efrust_scaffold::reader::read_model(&dst, Provider::Postgres, Some("public"))
        .await
        .expect("leyendo DST");
    dst_model.normalize();

    let st = src_model.tables[0].clone();
    let dt = dst_model.tables[0].clone();
    assert_eq!(st.columns, dt.columns, "columnas (vector/tsvector generadas)");
    assert_eq!(st.indexes, dt.indexes, "índices (hnsw/gin/expresión/parcial)");
    assert_eq!(st.triggers, dt.triggers, "triggers");
    assert_eq!(src_model.functions, dst_model.functions, "funciones");
    assert_eq!(src_model, dst_model, "modelo completo equivalente");
}
