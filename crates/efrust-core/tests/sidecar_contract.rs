//! Test del contrato C# ↔ Rust: el JSON emitido por el sidecar .NET debe
//! deserializar 1:1 en `DatabaseModel`. El fixture es salida REAL del sidecar
//! (`efrust-sidecar --assembly SampleModel.dll`), capturada en Fase 3.
//!
//! Si el sidecar cambia la forma del JSON o el IR cambia en Rust, este test
//! falla — es la red de seguridad del contrato.

use efrust_core::{snapshot, ReferentialAction};

#[test]
fn deserializa_salida_real_del_sidecar() {
    let bytes = include_bytes!("fixtures/sidecar_model.json");
    let model = snapshot::from_slice(bytes).expect("el Model IR del sidecar debe deserializar");

    assert_eq!(model.format_version, 1);
    assert!(
        model.product_version.as_deref().unwrap_or("").starts_with("9."),
        "product_version proviene del assembly de EF Core 9"
    );
    assert_eq!(model.tables.len(), 2);

    // Customer: facetas resueltas por Fluent API (HasMaxLength) + índice único.
    let customer = model.table(None, "Customer").expect("tabla Customer");
    assert_eq!(customer.column("Name").unwrap().max_length, Some(200));
    assert!(!customer.column("Name").unwrap().is_nullable);
    assert!(customer.column("Email").unwrap().is_nullable);
    assert!(customer
        .indexes
        .iter()
        .any(|i| i.name == "IX_Customer_Email" && i.is_unique));

    // Order: store_type forzado (HasColumnType), identidad y FK con ON DELETE.
    let order = model.table(None, "Order").expect("tabla Order");
    assert_eq!(
        order.column("Total").unwrap().store_type.as_deref(),
        Some("TEXT"),
        "HasColumnType(\"TEXT\") debe llegar como store_type"
    );
    assert!(order.column("Id").unwrap().is_identity);

    let fk = order.foreign_keys.first().expect("FK de Order");
    assert_eq!(fk.name, "FK_Order_Customer");
    assert_eq!(fk.principal_table, "Customer");
    assert_eq!(fk.columns, vec!["CustomerId"]);
    assert_eq!(fk.on_delete, ReferentialAction::Cascade);
}
