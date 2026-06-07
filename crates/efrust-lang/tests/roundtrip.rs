//! Round-trip `IR → .model → IR` y verificación del azúcar de escritura.

use std::collections::BTreeMap;

use efrust_core::model::{
    Column, DatabaseModel, DbFunction, ForeignKey, Index, PrimaryKey, ReferentialAction, Table,
    Trigger, TriggerEvent, TriggerTiming,
};
use efrust_lang::{emit_project, parse_entity, parse_project};
use serde_json::json;

fn col(name: &str, clr: &str, nullable: bool) -> Column {
    Column {
        name: name.into(),
        store_type: None,
        clr_type: Some(clr.into()),
        is_nullable: nullable,
        is_identity: false,
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

fn sample() -> DatabaseModel {
    let mut m = DatabaseModel::empty();
    m.default_schema = Some("public".into());
    m.product_version = Some("9.0.0".into());
    m.extensions = vec!["pg_trgm".into(), "unaccent".into(), "vector".into()];
    m.functions = vec![DbFunction {
        name: "normalize_body".into(),
        schema: Some("public".into()),
        definition: "CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger\nLANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$".into(),
    }];
    m.raw_objects = vec!["CREATE FULLTEXT CATALOG ft AS DEFAULT".into()];

    // Customer
    let mut name = col("Name", "System.String", false);
    name.max_length = Some(200);
    let mut email = col("Email", "System.String", true);
    email.max_length = Some(320);
    m.tables.push(Table {
        name: "Customer".into(),
        schema: None,
        clr_type: None,
        comment: Some("clientes".into()),
        columns: vec![
            col("Id", "System.Int32", false),
            name,
            email,
            col("Contact_Phone", "System.String", true),
        ],
        primary_key: Some(PrimaryKey {
            name: "PK_Customer".into(),
            columns: vec!["Id".into()],
        }),
        foreign_keys: vec![],
        indexes: vec![Index {
            name: "IX_Customer_Email".into(),
            columns: vec!["Email".into()],
            is_unique: true,
            filter: None,
            method: None,
            operators: vec![],
            expression: None,
        }],
        triggers: vec![],
        seed_data: vec![
            row(&[
                ("Id", json!(1)),
                ("Name", json!("ACME")),
                ("Email", json!("acme@example.com")),
            ]),
            row(&[
                ("Id", json!(2)),
                ("Name", json!("Globex")),
                ("Email", json!(null)),
            ]),
        ],
    });

    // Order
    let mut id = col("Id", "System.Int32", false);
    id.is_identity = true;
    let mut total = col("Total", "System.Decimal", false);
    total.precision = Some(18);
    total.scale = Some(2);
    let mut status = col("Status", "System.String", false);
    status.max_length = Some(20);
    let mut flag = col("Flag", "System.Boolean", false);
    flag.default_value_sql = Some("false".into());
    m.tables.push(Table {
        name: "Order".into(),
        schema: None,
        clr_type: None,
        comment: None,
        columns: vec![
            id,
            col("CustomerId", "System.Int32", false),
            total,
            status,
            flag,
        ],
        primary_key: Some(PrimaryKey {
            name: "PK_Order".into(),
            columns: vec!["Id".into()],
        }),
        foreign_keys: vec![ForeignKey {
            name: "FK_Order_Customer".into(),
            columns: vec!["CustomerId".into()],
            principal_table: "Customer".into(),
            principal_schema: None,
            principal_columns: vec!["Id".into()],
            on_delete: ReferentialAction::Cascade,
        }],
        indexes: vec![],
        triggers: vec![],
        seed_data: vec![],
    });

    // Document (esquema propio + tipos nativos + computed + dbtype + índices + trigger)
    let mut did = col("Id", "System.Int32", false);
    did.is_identity = true;
    let mut embedding = Column {
        name: "Embedding".into(),
        store_type: Some("vector(1536)".into()),
        clr_type: None,
        is_nullable: false,
        ..col("Embedding", "x", false)
    };
    embedding.clr_type = None;
    let mut search = col("Search", "x", false);
    search.clr_type = None;
    search.store_type = Some("tsvector".into());
    let mut slug = col("Slug", "System.String", false);
    slug.computed_sql = Some("lower(body)".into());
    slug.computed_stored = true;
    let mut meta = col("Meta", "System.String", true);
    meta.store_type = Some("jsonb".into());
    m.tables.push(Table {
        name: "Document".into(),
        schema: Some("audit".into()),
        clr_type: Some("App.Doc".into()),
        comment: None,
        columns: vec![did, col("Body", "System.String", false), embedding, search, slug, meta],
        primary_key: Some(PrimaryKey {
            name: "PK_Document".into(),
            columns: vec!["Id".into()],
        }),
        foreign_keys: vec![],
        indexes: vec![
            Index {
                name: "ix_docs_fts".into(),
                columns: vec![],
                is_unique: false,
                filter: None,
                method: Some("gin".into()),
                operators: vec![],
                expression: Some("to_tsvector('english'::regconfig, body)".into()),
            },
            Index {
                name: "ix_emb".into(),
                columns: vec!["Embedding".into()],
                is_unique: false,
                filter: Some("Body IS NOT NULL".into()),
                method: Some("hnsw".into()),
                operators: vec!["vector_cosine_ops".into()],
                expression: None,
            },
        ],
        triggers: vec![Trigger {
            name: "trg_normalize".into(),
            table: "Document".into(),
            schema: Some("audit".into()),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert, TriggerEvent::Update],
            function: Some("normalize_body".into()),
            definition: "CREATE TRIGGER trg_normalize BEFORE INSERT OR UPDATE ON audit.documents\nFOR EACH ROW EXECUTE FUNCTION normalize_body()".into(),
        }],
        seed_data: vec![],
    });

    // TenantItem (PK compuesta con nombre no convencional + FK con set_null)
    m.tables.push(Table {
        name: "TenantItem".into(),
        schema: None,
        clr_type: None,
        comment: None,
        columns: vec![
            col("TenantId", "System.Int32", false),
            col("Id", "System.Int32", false),
            col("OwnerId", "System.Int32", true),
        ],
        primary_key: Some(PrimaryKey {
            name: "PK_Item".into(),
            columns: vec!["TenantId".into(), "Id".into()],
        }),
        foreign_keys: vec![ForeignKey {
            name: "fk_owner".into(),
            columns: vec!["OwnerId".into()],
            principal_table: "Customer".into(),
            principal_schema: None,
            principal_columns: vec!["Id".into()],
            on_delete: ReferentialAction::SetNull,
        }],
        indexes: vec![],
        triggers: vec![],
        seed_data: vec![],
    });

    m.normalize();
    m
}

fn row(pairs: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn round_trip_ir_dsl_ir() {
    let original = sample();
    let files = emit_project(&original);
    // Vista de depuración: imprime los archivos si el test falla.
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f.name.as_str(), f.contents.as_str()))
        .collect();
    let mut parsed = parse_project(&refs).expect("parse");
    parsed.normalize();

    if parsed != original {
        for f in &files {
            eprintln!("===== {} =====\n{}", f.name, f.contents);
        }
    }
    assert_eq!(parsed, original);
}

#[test]
fn emite_archivos_esperados() {
    let files = emit_project(&sample());
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"database.model"));
    assert!(names.contains(&"Customer.model"));
    assert!(names.contains(&"Order.model"));
    assert!(names.contains(&"Document.model"));
}

#[test]
fn azucar_owns_se_expande_a_columnas_planas() {
    let src = "Customer\n  fields:\n    Id: int pk\n    Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}\n";
    let t = parse_entity(src).unwrap();
    let names: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Id", "Contact_Phone", "Contact_City"]);
    let city = t.columns.iter().find(|c| c.name == "Contact_City").unwrap();
    assert_eq!(city.max_length, Some(80));
    assert!(city.is_nullable);
}

#[test]
fn azucar_enum_se_vuelve_columna_escalar() {
    let src = "Order\n  fields:\n    Id: int pk\n    Status: enum[Pending, Shipped] as=string maxlen=20\n";
    let t = parse_entity(src).unwrap();
    let status = t.columns.iter().find(|c| c.name == "Status").unwrap();
    assert_eq!(status.clr_type.as_deref(), Some("System.String"));
    assert_eq!(status.max_length, Some(20));
}

#[test]
fn azucar_subtypes_funde_en_una_tabla_con_discriminador() {
    let src = "Payment\n  discriminator: Discriminator\n  fields:\n    Id: int pk identity\n    Amount: decimal\n  subtypes:\n    CardPayment: {CardNumber: string}\n    CashPayment: {Note: string?}\n";
    let t = parse_entity(src).unwrap();
    let names: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Discriminator"));
    assert!(names.contains(&"CardNumber"));
    assert!(names.contains(&"Note"));
    // Las columnas de subtipo son nullable.
    let card = t.columns.iter().find(|c| c.name == "CardNumber").unwrap();
    assert!(card.is_nullable);
    let disc = t
        .columns
        .iter()
        .find(|c| c.name == "Discriminator")
        .unwrap();
    assert_eq!(disc.clr_type.as_deref(), Some("System.String"));
    assert!(!disc.is_nullable);
}
