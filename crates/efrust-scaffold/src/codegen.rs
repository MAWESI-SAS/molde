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

/// Genera todos los archivos: una entidad por tabla + el DbContext.
pub fn generate(model: &DatabaseModel, opts: &CodegenOptions) -> Vec<GeneratedFile> {
    let mut files: Vec<GeneratedFile> = model
        .tables
        .iter()
        .map(|t| GeneratedFile {
            relative_path: format!("{}.cs", class_name(t)),
            contents: entity_class(t, model, opts),
        })
        .collect();

    files.push(GeneratedFile {
        relative_path: format!("{}.cs", opts.context_name),
        contents: db_context(model, opts),
    });

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

fn db_context(model: &DatabaseModel, opts: &CodegenOptions) -> String {
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

    // Propiedades: HasColumnName si el nombre C# difiere del de BD, + HasMaxLength.
    for col in &table.columns {
        let pname = prop_name(&col.name);
        let mut chain = String::new();
        if pname != col.name {
            let _ = write!(chain, ".HasColumnName(\"{}\")", col.name);
        }
        if let Some(n) = col.max_length {
            let _ = write!(chain, ".HasMaxLength({n})");
        }
        if !chain.is_empty() {
            let _ = writeln!(s, "            entity.Property(e => e.{pname}){chain};");
        }
    }

    for idx in &table.indexes {
        let target = key_selector(&idx.columns);
        let unique = if idx.is_unique { ".IsUnique()" } else { "" };
        let _ = writeln!(s, "            entity.HasIndex({target}, \"{}\"){unique};", idx.name);
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
            indexes: vec![Index { name: "IX_Order_CustomerId".into(), columns: vec!["CustomerId".into()], is_unique: false, filter: None }],
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
}
