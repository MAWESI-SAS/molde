//! End-to-end round-trip against SQL Server (ignored by default: requires a DB via TDS).
//!
//! Reads `SRC_CONNECTION` (ADO string) → IR, recreates it in `DST_CONNECTION` via the
//! diff operations emitted by `SqlServerGenerator`, reads the target back
//! and verifies that the PERSISTED computed columns are preserved.
//!
//! Run:
//! ```text
//! SRC_CONNECTION='Server=…;Database=appdb;…' DST_CONNECTION='Server=…;Database=rtdb;…' \
//!   cargo test -p molde-scaffold --test roundtrip_sqlserver -- --ignored --nocapture
//! ```

use molde_core::diff::diff;
use molde_core::model::DatabaseModel;
use molde_providers::{Provider, SqlGenerator, SqlServerGenerator};
use tiberius::Client;
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

async fn connect(ado: &str) -> Client<tokio_util::compat::Compat<TcpStream>> {
    let config = tiberius::Config::from_ado_string(ado).expect("ADO string");
    let tcp = TcpStream::connect(config.get_addr()).await.expect("tcp");
    tcp.set_nodelay(true).ok();
    Client::connect(config, tcp.compat_write())
        .await
        .expect("tds connect")
}

#[tokio::test]
#[ignore = "requires SRC_CONNECTION and DST_CONNECTION (SQL Server)"]
async fn round_trip_sqlserver_computed_persisted() {
    let src = std::env::var("SRC_CONNECTION").expect("SRC_CONNECTION");
    let dst = std::env::var("DST_CONNECTION").expect("DST_CONNECTION");

    let mut src_model = molde_scaffold::reader::read_model(&src, Provider::SqlServer, Some("dbo"))
        .await
        .expect("reading SRC");
    src_model.normalize();

    let st = &src_model.tables[0];
    assert!(
        st.columns
            .iter()
            .any(|c| c.computed_sql.is_some() && c.computed_stored),
        "the source must have a PERSISTED computed column"
    );

    // Recreate in the target via tiberius (same driver as the reader).
    let mut client = connect(&dst).await;
    let gen = SqlServerGenerator::new();
    for op in &diff(&DatabaseModel::empty(), &src_model) {
        for stmt in gen.emit(op).expect("emit SQL") {
            client
                .simple_query(stmt.clone())
                .await
                .unwrap_or_else(|e| panic!("applying:\n{stmt}\nerror: {e}"));
        }
    }

    let mut dst_model = molde_scaffold::reader::read_model(&dst, Provider::SqlServer, Some("dbo"))
        .await
        .expect("reading DST");
    dst_model.normalize();

    let dt = &dst_model.tables[0];
    let sc = st
        .columns
        .iter()
        .find(|c| c.computed_sql.is_some())
        .unwrap();
    let dc = dt
        .columns
        .iter()
        .find(|c| c.computed_sql.is_some())
        .expect("computed column in target");
    assert_eq!(sc.name, dc.name);
    assert_eq!(
        sc.computed_stored, dc.computed_stored,
        "PERSISTED preserved"
    );
}
