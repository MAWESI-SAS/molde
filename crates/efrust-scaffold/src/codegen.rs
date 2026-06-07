//! Generación de C# a partir del [`DatabaseModel`]: una clase POCO por tabla y
//! un `DbContext`. Puro (modelo → texto), sin acceso a BD.
//!
//! Los nombres de BD se normalizan a PascalCase para C# (`created_at` →
//! `CreatedAt`); cuando el nombre C# difiere del de BD se emite `ToTable` /
//! `HasColumnName` para mapear de vuelta. Se generan además las propiedades de
//! navegación (referencia y colección) a partir de las claves foráneas.

use std::fmt::Write;

use efrust_core::model::{DatabaseModel, ForeignKey, ReferentialAction, Table};

use crate::csharp::{self, pascalize};

/// Opciones de generación.
#[derive(Debug, Clone)]
pub struct CodegenOptions {
    pub namespace: String,
    pub context_name: String,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            namespace: "App.Data".to_string(),
            context_name: "AppDbContext".to_string(),
        }
    }
}

/// Un archivo generado (ruta relativa al directorio de salida + contenido).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: String,
    pub contents: String,
}

/// Genera todos los archivos: una entidad por tabla + el DbContext + (si aplica)
/// un artefacto `.sql` con los objetos de BD no modelables por EF.
pub fn generate(model: &DatabaseModel, opts: &CodegenOptions) -> Vec<GeneratedFile> {
    let mut files: Vec<GeneratedFile> = model
        .tables
        .iter()
        .map(|t| GeneratedFile {
            relative_path: format!("{}.cs", class_name(t)),
            contents: entity_class(t, model, opts),
        })
        .collect();

    // Objetos no modelables por EF (funciones, triggers, índices por expresión).
    let db_objects = db_objects_sql(model);

    files.push(GeneratedFile {
        relative_path: format!("{}.cs", opts.context_name),
        contents: db_context(model, opts, db_objects.is_some()),
    });

    if let Some(sql) = db_objects {
        files.push(GeneratedFile {
            relative_path: format!("{}.DbObjects.sql", opts.context_name),
            contents: sql,
        });
    }

    files
}

/// Nombre de la clase C# para una tabla (PascalCase del nombre de BD).
fn class_name(table: &Table) -> String {
    pascalize(&table.name)
}

/// Nombre de la propiedad C# para una columna de BD.
fn prop_name(column: &str) -> String {
    pascalize(column)
}

fn entity_class(table: &Table, model: &DatabaseModel, opts: &CodegenOptions) -> String {
    let mut members: Vec<String> = Vec::new();

    for col in &table.columns {
        members.push(format!(
            "    public {} {} {{ get; set; }}{}",
            csharp::property_type(col),
            prop_name(&col.name),
            csharp::property_initializer(col),
        ));
    }

    // Navegaciones de referencia (FKs de esta tabla → entidad principal).
    for fk in &table.foreign_keys {
        let nav = reference_nav_name(table, fk);
        let principal = pascalize(&fk.principal_table);
        if fk_required(table, fk) {
            members.push(format!(
                "    public virtual {principal} {nav} {{ get; set; }} = null!;"
            ));
        } else {
            members.push(format!(
                "    public virtual {principal}? {nav} {{ get; set; }}"
            ));
        }
    }

    // Navegaciones de colección (FKs entrantes desde otras tablas).
    for (dependent, fk) in incoming_fks(model, &table.name) {
        let coll = collection_nav_name(dependent, &table.name, fk);
        let dep_class = class_name(dependent);
        members.push(format!(
            "    public virtual ICollection<{dep_class}> {coll} {{ get; set; }} = new List<{dep_class}>();"
        ));
    }

    let mut s = String::new();
    let _ = writeln!(s, "using System;");
    let _ = writeln!(s, "using System.Collections.Generic;");
    let _ = writeln!(s);
    let _ = writeln!(s, "namespace {};", opts.namespace);
    let _ = writeln!(s);
    let _ = writeln!(s, "public partial class {}", class_name(table));
    let _ = writeln!(s, "{{");
    let _ = writeln!(s, "{}", members.join("\n\n"));
    let _ = writeln!(s, "}}");
    s
}

fn db_context(model: &DatabaseModel, opts: &CodegenOptions, has_db_objects: bool) -> String {
    let ctx = &opts.context_name;
    let mut s = String::new();
    let _ = writeln!(s, "using System;");
    let _ = writeln!(s, "using System.Collections.Generic;");
    let _ = writeln!(s, "using Microsoft.EntityFrameworkCore;");
    let _ = writeln!(s);
    let _ = writeln!(s, "namespace {};", opts.namespace);
    let _ = writeln!(s);
    let _ = writeln!(s, "public partial class {ctx} : DbContext");
    let _ = writeln!(s, "{{");
    let _ = writeln!(s, "    public {ctx}()");
    let _ = writeln!(s, "    {{");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    public {ctx}(DbContextOptions<{ctx}> options)");
    let _ = writeln!(s, "        : base(options)");
    let _ = writeln!(s, "    {{");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);

    for table in &model.tables {
        let _ = writeln!(
            s,
            "    public virtual DbSet<{}> {} {{ get; set; }}",
            class_name(table),
            csharp::pluralize(&class_name(table)),
        );
    }

    let _ = writeln!(s);
    let _ = writeln!(s, "    protected override void OnModelCreating(ModelBuilder modelBuilder)");
    let _ = writeln!(s, "    {{");
    for (i, table) in model.tables.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(s);
        }
        write_entity_config(&mut s, table, model);
    }
    if has_db_objects {
        let _ = writeln!(s);
        let _ = writeln!(s, "        // NOTA: esta base de datos contiene objetos que EF Core no modela");
        let _ = writeln!(s, "        // (funciones, triggers e índices por expresión). Se exportaron a");
        let _ = writeln!(s, "        // '{}.DbObjects.sql'; aplícalos fuera del modelo (p. ej. con", opts.context_name);
        let _ = writeln!(s, "        // migrationBuilder.Sql(...) en una migración).");
    }

    let _ = writeln!(s);
    let _ = writeln!(s, "        OnModelCreatingPartial(modelBuilder);");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    partial void OnModelCreatingPartial(ModelBuilder modelBuilder);");
    let _ = writeln!(s, "}}");
    s
}

fn write_entity_config(s: &mut String, table: &Table, model: &DatabaseModel) {
    let cls = class_name(table);
    let _ = writeln!(s, "        modelBuilder.Entity<{cls}>(entity =>");
    let _ = writeln!(s, "        {{");

    if let Some(pk) = &table.primary_key {
        let key = key_selector(&pk.columns);
        let _ = writeln!(s, "            entity.HasKey({key}).HasName(\"{}\");", pk.name);
    }

    // Tabla (siempre, con el nombre de BD; con esquema si difiere del por defecto).
    match (&table.schema, &model.default_schema) {
        (Some(sc), def) if Some(sc) != def.as_ref() => {
            let _ = writeln!(s, "            entity.ToTable(\"{}\", \"{sc}\");", table.name);
        }
        _ => {
            let _ = writeln!(s, "            entity.ToTable(\"{}\");", table.name);
        }
    }

    // Propiedades: HasColumnName si el nombre C# difiere del de BD, + HasMaxLength
    // + HasComputedColumnSql para columnas generadas (p. ej. tsvector STORED).
    for col in &table.columns {
        let pname = prop_name(&col.name);
        let mut chain = String::new();
        if pname != col.name {
            let _ = write!(chain, ".HasColumnName(\"{}\")", col.name);
        }
        if let Some(expr) = &col.computed_sql {
            let _ = write!(
                chain,
                ".HasComputedColumnSql(\"{}\", stored: {})",
                escape_cs(expr),
                col.computed_stored
            );
        } else if let Some(n) = col.max_length {
            let _ = write!(chain, ".HasMaxLength({n})");
        }
        if !chain.is_empty() {
            let _ = writeln!(s, "            entity.Property(e => e.{pname}){chain};");
        }
    }

    for idx in &table.indexes {
        // Los índices por expresión no se pueden expresar con Fluent API; van al
        // artefacto .sql.
        if idx.expression.is_some() || idx.columns.is_empty() {
            continue;
        }
        let target = key_selector(&idx.columns);
        let mut chain = String::new();
        if idx.is_unique {
            chain.push_str(".IsUnique()");
        }
        if let Some(f) = &idx.filter {
            let _ = write!(chain, ".HasFilter(\"{}\")", escape_cs(f));
        }
        if let Some(m) = &idx.method {
            let _ = write!(chain, ".HasMethod(\"{m}\")");
        }
        if !idx.operators.is_empty() {
            let ops = idx
                .operators
                .iter()
                .map(|o| format!("\"{o}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(chain, ".HasOperators({ops})");
        }
        let _ = writeln!(s, "            entity.HasIndex({target}, \"{}\"){chain};", idx.name);
    }

    // Relaciones (lado dependiente): HasOne / WithMany.
    for fk in &table.foreign_keys {
        let ref_nav = reference_nav_name(table, fk);
        let coll_nav = collection_nav_name(table, &fk.principal_table, fk);
        let fk_target = key_selector_with(&fk.columns, "d");
        let _ = writeln!(s, "            entity.HasOne(d => d.{ref_nav}).WithMany(p => p.{coll_nav})");
        let _ = writeln!(s, "                .HasForeignKey({fk_target})");
        if let Some(behavior) = delete_behavior(fk.on_delete) {
            let _ = writeln!(s, "                .OnDelete(DeleteBehavior.{behavior})");
        }
        let _ = writeln!(s, "                .HasConstraintName(\"{}\");", fk.name);
    }

    let _ = writeln!(s, "        }});");
}

/// Selector de columnas `e => e.X` o `e => new {{ e.A, e.B }}` (PascalCase).
fn key_selector(columns: &[String]) -> String {
    key_selector_with(columns, "e")
}

fn key_selector_with(columns: &[String], param: &str) -> String {
    if columns.len() == 1 {
        format!("{param} => {param}.{}", prop_name(&columns[0]))
    } else {
        let cols = columns
            .iter()
            .map(|c| format!("{param}.{}", prop_name(c)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{param} => new {{ {cols} }}")
    }
}

// --- Helpers de navegación ---------------------------------------------------

fn incoming_fks<'a>(model: &'a DatabaseModel, table_name: &str) -> Vec<(&'a Table, &'a ForeignKey)> {
    let mut out = Vec::new();
    for t in &model.tables {
        for fk in &t.foreign_keys {
            if fk.principal_table == table_name {
                out.push((t, fk));
            }
        }
    }
    out
}

fn reference_nav_name(table: &Table, fk: &ForeignKey) -> String {
    let principal = pascalize(&fk.principal_table);
    let same = table
        .foreign_keys
        .iter()
        .filter(|f| f.principal_table == fk.principal_table)
        .count();
    if same <= 1 {
        principal
    } else {
        format!("{principal}{}", strip_id(&prop_name(fk.columns.first().map(String::as_str).unwrap_or(""))))
    }
}

fn collection_nav_name(dependent: &Table, principal_name: &str, fk: &ForeignKey) -> String {
    let base = csharp::pluralize(&class_name(dependent));
    let same = dependent
        .foreign_keys
        .iter()
        .filter(|f| f.principal_table == principal_name)
        .count();
    if same <= 1 {
        base
    } else {
        format!("{base}{}", strip_id(&prop_name(fk.columns.first().map(String::as_str).unwrap_or(""))))
    }
}

fn fk_required(table: &Table, fk: &ForeignKey) -> bool {
    fk.columns
        .iter()
        .all(|c| table.column(c).map(|col| !col.is_nullable).unwrap_or(true))
}

fn strip_id(name: &str) -> String {
    name.strip_suffix("Id").filter(|s| !s.is_empty()).unwrap_or(name).to_string()
}

fn delete_behavior(action: ReferentialAction) -> Option<&'static str> {
    match action {
        ReferentialAction::Cascade => Some("Cascade"),
        ReferentialAction::SetNull => Some("SetNull"),
        ReferentialAction::Restrict => Some("Restrict"),
        ReferentialAction::NoAction | ReferentialAction::SetDefault => None,
    }
}

/// Escapa una cadena para incrustarla en un literal C# entre comillas dobles.
fn escape_cs(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Genera el artefacto SQL con los objetos que EF no modela: funciones, índices
/// por expresión (full-text/funcionales) y triggers. Devuelve `None` si no hay
/// ninguno. La generación del `CREATE INDEX` se delega al provider de Postgres
/// para que coincida exactamente con la que aplica `database update`.
fn db_objects_sql(model: &DatabaseModel) -> Option<String> {
    use efrust_core::diff::Operation;
    use efrust_providers::{PostgresGenerator, SqlGenerator};

    let expr_indexes: Vec<(&Table, &efrust_core::model::Index)> = model
        .tables
        .iter()
        .flat_map(|t| {
            t.indexes
                .iter()
                .filter(|i| i.expression.is_some())
                .map(move |i| (t, i))
        })
        .collect();

    let has_triggers = model.tables.iter().any(|t| !t.triggers.is_empty());
    if model.functions.is_empty() && expr_indexes.is_empty() && !has_triggers {
        return None;
    }

    let gen = PostgresGenerator::new();
    let mut s = String::new();
    let _ = writeln!(s, "-- Objetos de base de datos no modelables por EF Core.");
    let _ = writeln!(s, "-- Generado por `efrust scaffold`. Aplicar fuera del modelo EF");
    let _ = writeln!(s, "-- (p. ej. con migrationBuilder.Sql(...) en una migración).");

    if !model.functions.is_empty() {
        let _ = writeln!(s, "\n-- Funciones");
        for f in &model.functions {
            let _ = writeln!(s, "{};", f.definition.trim_end_matches(';'));
        }
    }

    if !expr_indexes.is_empty() {
        let _ = writeln!(s, "\n-- Índices (por expresión / full-text)");
        for (t, idx) in &expr_indexes {
            let op = Operation::CreateIndex {
                schema: t.schema.clone(),
                table: t.name.clone(),
                index: (*idx).clone(),
            };
            if let Ok(stmts) = gen.emit(&op) {
                for stmt in stmts {
                    let _ = writeln!(s, "{stmt}");
                }
            }
        }
    }

    if has_triggers {
        let _ = writeln!(s, "\n-- Triggers");
        for t in &model.tables {
            for tg in &t.triggers {
                let _ = writeln!(s, "{};", tg.definition.trim_end_matches(';'));
            }
        }
    }

    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use efrust_core::model::{Column, ForeignKey, Index, PrimaryKey, ReferentialAction, Table};

    fn col(name: &str, clr: &str, nullable: bool, max_len: Option<i32>) -> Column {
        Column {
            name: name.into(),
            store_type: None,
            clr_type: Some(clr.into()),
            is_nullable: nullable,
            is_identity: false,
            max_length: max_len,
            precision: None,
            scale: None,
            default_value_sql: None,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        }
    }

    fn model() -> DatabaseModel {
        let mut m = DatabaseModel::empty();
        m.tables.push(Table {
            name: "Customer".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: vec![col("Id", "System.Int32", false, None), col("Name", "System.String", false, Some(200))],
            primary_key: Some(PrimaryKey { name: "PK_Customer".into(), columns: vec!["Id".into()] }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
        });
        m.tables.push(Table {
            name: "Order".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: vec![col("Id", "System.Int32", false, None), col("CustomerId", "System.Int32", false, None)],
            primary_key: Some(PrimaryKey { name: "PK_Order".into(), columns: vec!["Id".into()] }),
            foreign_keys: vec![ForeignKey {
                name: "FK_Order_Customer".into(),
                columns: vec!["CustomerId".into()],
                principal_table: "Customer".into(),
                principal_schema: None,
                principal_columns: vec!["Id".into()],
                on_delete: ReferentialAction::Cascade,
            }],
            indexes: vec![Index { name: "IX_Order_CustomerId".into(), columns: vec!["CustomerId".into()], is_unique: false, filter: None, method: None, operators: vec![], expression: None }],
            triggers: vec![],
        });
        m
    }

    #[test]
    fn genera_navegaciones_y_relacion() {
        let files = generate(&model(), &CodegenOptions::default());
        let order = &files.iter().find(|f| f.relative_path == "Order.cs").unwrap().contents;
        assert!(order.contains("public virtual Customer Customer { get; set; } = null!;"));
        let customer = &files.iter().find(|f| f.relative_path == "Customer.cs").unwrap().contents;
        assert!(customer.contains("public virtual ICollection<Order> Orders { get; set; } = new List<Order>();"));
        let ctx = &files.iter().find(|f| f.relative_path == "AppDbContext.cs").unwrap().contents;
        assert!(ctx.contains("entity.HasOne(d => d.Customer).WithMany(p => p.Orders)"));
        assert!(ctx.contains(".HasForeignKey(d => d.CustomerId)"));
        assert!(ctx.contains(".HasConstraintName(\"FK_Order_Customer\");"));
    }

    #[test]
    fn normaliza_nombres_snake_case() {
        let mut m = DatabaseModel::empty();
        m.tables.push(Table {
            name: "customer_order".into(),
            schema: None,
            clr_type: None,
            comment: None,
            columns: vec![
                col("id", "System.Int32", false, None),
                col("created_at", "System.DateTime", false, None),
            ],
            primary_key: Some(PrimaryKey { name: "pk_customer_order".into(), columns: vec!["id".into()] }),
            foreign_keys: vec![],
            indexes: vec![],
            triggers: vec![],
        });
        let files = generate(&m, &CodegenOptions::default());

        let entity = &files.iter().find(|f| f.relative_path == "CustomerOrder.cs").unwrap().contents;
        assert!(entity.contains("public partial class CustomerOrder"));
        assert!(entity.contains("public int Id { get; set; }"));
        assert!(entity.contains("public DateTime CreatedAt { get; set; }"));

        let ctx = &files.iter().find(|f| f.relative_path == "AppDbContext.cs").unwrap().contents;
        assert!(ctx.contains("public virtual DbSet<CustomerOrder> CustomerOrders"));
        assert!(ctx.contains("entity.ToTable(\"customer_order\");"));
        assert!(ctx.contains("entity.HasKey(e => e.Id).HasName(\"pk_customer_order\");"));
        // La columna renombrada mapea de vuelta con HasColumnName.
        assert!(ctx.contains("entity.Property(e => e.CreatedAt).HasColumnName(\"created_at\");"));
    }

    fn search_model() -> DatabaseModel {
        use efrust_core::model::{DbFunction, Index, Trigger, TriggerEvent, TriggerTiming};
        let mut m = DatabaseModel::empty();
        m.default_schema = Some("public".into());

        let mut embedding = col("embedding", "Pgvector.Vector", false, None);
        embedding.store_type = Some("vector(384)".into());
        let mut search = col("search", "NpgsqlTypes.NpgsqlTsVector", false, None);
        search.store_type = Some("tsvector".into());
        search.computed_sql = Some("to_tsvector('english'::regconfig, body)".into());
        search.computed_stored = true;

        m.tables.push(Table {
            name: "documents".into(),
            schema: Some("public".into()),
            clr_type: None,
            comment: None,
            columns: vec![
                col("id", "System.Int32", false, None),
                col("body", "System.String", true, None),
                embedding,
                search,
            ],
            primary_key: Some(PrimaryKey { name: "pk_documents".into(), columns: vec!["id".into()] }),
            foreign_keys: vec![],
            indexes: vec![
                // Índice vectorial HNSW con operator class.
                Index {
                    name: "ix_documents_embedding".into(),
                    columns: vec!["embedding".into()],
                    is_unique: false,
                    filter: None,
                    method: Some("hnsw".into()),
                    operators: vec!["vector_cosine_ops".into()],
                    expression: None,
                },
                // Índice GIN por expresión (full-text).
                Index {
                    name: "ix_documents_fts".into(),
                    columns: vec![],
                    is_unique: false,
                    filter: None,
                    method: Some("gin".into()),
                    operators: vec![],
                    expression: Some("to_tsvector('english'::regconfig, body)".into()),
                },
            ],
            triggers: vec![Trigger {
                name: "trg_normalize".into(),
                table: "documents".into(),
                schema: Some("public".into()),
                timing: TriggerTiming::Before,
                events: vec![TriggerEvent::Insert],
                function: Some("public.normalize_body".into()),
                definition: "CREATE TRIGGER trg_normalize BEFORE INSERT ON public.documents FOR EACH ROW EXECUTE FUNCTION normalize_body()".into(),
            }],
        });
        m.functions.push(DbFunction {
            name: "normalize_body".into(),
            schema: Some("public".into()),
            definition: "CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$".into(),
        });
        m
    }

    #[test]
    fn mapea_tipos_vector_y_tsvector() {
        let files = generate(&search_model(), &CodegenOptions::default());
        let doc = &files.iter().find(|f| f.relative_path == "Documents.cs").unwrap().contents;
        assert!(doc.contains("public Pgvector.Vector Embedding { get; set; } = null!;"), "got: {doc}");
        assert!(doc.contains("public NpgsqlTypes.NpgsqlTsVector Search { get; set; } = null!;"), "got: {doc}");
    }

    #[test]
    fn columna_generada_emite_has_computed_column_sql() {
        let files = generate(&search_model(), &CodegenOptions::default());
        let ctx = &files.iter().find(|f| f.relative_path == "AppDbContext.cs").unwrap().contents;
        assert!(ctx.contains(
            "entity.Property(e => e.Search).HasColumnName(\"search\").HasComputedColumnSql(\"to_tsvector('english'::regconfig, body)\", stored: true);"
        ), "got: {ctx}");
    }

    #[test]
    fn indice_vectorial_emite_metodo_y_operadores() {
        let files = generate(&search_model(), &CodegenOptions::default());
        let ctx = &files.iter().find(|f| f.relative_path == "AppDbContext.cs").unwrap().contents;
        assert!(ctx.contains(
            "entity.HasIndex(e => e.Embedding, \"ix_documents_embedding\").HasMethod(\"hnsw\").HasOperators(\"vector_cosine_ops\");"
        ), "got: {ctx}");
        // El índice por expresión NO va al Fluent API.
        assert!(!ctx.contains("ix_documents_fts"));
    }

    #[test]
    fn artefacto_sql_contiene_funcion_indice_y_trigger() {
        let files = generate(&search_model(), &CodegenOptions::default());
        let sql = &files
            .iter()
            .find(|f| f.relative_path == "AppDbContext.DbObjects.sql")
            .expect("debe existir el artefacto .sql")
            .contents;
        assert!(sql.contains("CREATE OR REPLACE FUNCTION public.normalize_body()"), "got: {sql}");
        assert!(sql.contains("CREATE INDEX \"ix_documents_fts\" ON \"public\".\"documents\" USING gin (to_tsvector('english'::regconfig, body));"), "got: {sql}");
        assert!(sql.contains("CREATE TRIGGER trg_normalize BEFORE INSERT ON public.documents"), "got: {sql}");
        // El context referencia el artefacto.
        let ctx = &files.iter().find(|f| f.relative_path == "AppDbContext.cs").unwrap().contents;
        assert!(ctx.contains("AppDbContext.DbObjects.sql"));
    }
}
