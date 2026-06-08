//! Emitter: `DatabaseModel`/`Table` → `.model` text in canonical form.

use std::fmt::Write;

use molde_core::model::{
    Column, DatabaseModel, ForeignKey, Index, ReferentialAction, Table, Trigger, TriggerEvent,
    TriggerTiming,
};

use crate::types::clr_to_logical;
use crate::value::{emit_value, quote_scalar};
use crate::ModelFile;

/// Emits the full project: `database.model` (if there are globals) + one file
/// per table.
pub fn emit_project(model: &DatabaseModel) -> Vec<ModelFile> {
    let mut files = Vec::new();
    let db = emit_database(model);
    if !db.trim().is_empty() {
        files.push(ModelFile {
            name: "database.model".to_string(),
            contents: db,
        });
    }
    for t in &model.tables {
        files.push(ModelFile {
            name: format!("{}.model", t.name),
            contents: emit_entity(t, model),
        });
    }
    files
}

/// Emits the global `database.model` file.
pub fn emit_database(model: &DatabaseModel) -> String {
    let mut s = String::new();
    if let Some(sc) = &model.default_schema {
        let _ = writeln!(s, "schema: {}", quote_scalar(sc));
    }
    if let Some(pv) = &model.product_version {
        let _ = writeln!(s, "product-version: {}", quote_scalar(pv));
    }
    if !model.extensions.is_empty() {
        let exts: Vec<String> = model.extensions.iter().map(|e| quote_scalar(e)).collect();
        let _ = writeln!(s, "extensions: [{}]", exts.join(", "));
    }
    if !model.functions.is_empty() {
        let _ = writeln!(s, "functions:");
        for f in &model.functions {
            let label = match &f.schema {
                Some(sc) => format!("{sc}.{}", f.name),
                None => f.name.clone(),
            };
            let _ = writeln!(s, "  - {label}: |");
            emit_block(&mut s, &f.definition, 6);
        }
    }
    if !model.raw_objects.is_empty() {
        let _ = writeln!(s, "raw:");
        for r in &model.raw_objects {
            let _ = writeln!(s, "  - |");
            emit_block(&mut s, r, 6);
        }
    }
    s
}

/// Emits an entity file in canonical form.
pub fn emit_entity(t: &Table, _model: &DatabaseModel) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", t.name);
    if let Some(clr) = &t.clr_type {
        let _ = writeln!(s, "  clr: {}", quote_scalar(clr));
    }
    if let Some(sc) = &t.schema {
        let _ = writeln!(s, "  schema: {}", quote_scalar(sc));
    }
    if let Some(c) = &t.comment {
        let _ = writeln!(s, "  comment: {}", quote_scalar(c));
    }

    // Single-column unique indexes with a conventional name → `unique` facet.
    let inline_unique: Vec<&str> = t
        .indexes
        .iter()
        .filter(|ix| is_inline_unique(t, ix))
        .filter_map(|ix| ix.columns.first().map(String::as_str))
        .collect();

    // Inline single PK.
    let pk_single = t
        .primary_key
        .as_ref()
        .filter(|pk| pk.columns.len() == 1)
        .map(|pk| (pk.columns[0].as_str(), pk.name.as_str()));

    let _ = writeln!(s, "  fields:");
    for col in &t.columns {
        let pk = pk_single.filter(|(c, _)| *c == col.name).map(|(_, n)| n);
        let uniq = inline_unique.contains(&col.name.as_str());
        let _ = writeln!(s, "    {}", emit_field(t, col, pk, uniq));
    }

    // Composite PK or one not represented inline → `key:` block.
    if let Some(pk) = &t.primary_key {
        if pk.columns.len() > 1 {
            let cols = pk.columns.join(", ");
            if pk.name == molde_core::conventions::pk_name(&t.name) {
                let _ = writeln!(s, "  key: [{cols}]");
            } else {
                let _ = writeln!(s, "  key: [{cols}] name={}", quote_scalar(&pk.name));
            }
        }
    }

    if !t.foreign_keys.is_empty() {
        let _ = writeln!(s, "  belongs-to:");
        for fk in &t.foreign_keys {
            let _ = writeln!(s, "    {}: {}", fk.principal_table, emit_fk(t, fk));
        }
    }

    let block_indexes: Vec<&Index> = t
        .indexes
        .iter()
        .filter(|ix| !is_inline_unique(t, ix))
        .collect();
    if !block_indexes.is_empty() {
        let _ = writeln!(s, "  indexes:");
        for ix in block_indexes {
            let _ = writeln!(s, "    - {}: {}", ix.name, emit_index(ix));
        }
    }

    if !t.triggers.is_empty() {
        let _ = writeln!(s, "  triggers:");
        for tg in &t.triggers {
            emit_trigger(&mut s, tg);
        }
    }

    if !t.seed_data.is_empty() {
        let _ = writeln!(s, "  seed:");
        for row in &t.seed_data {
            let parts: Vec<String> = row
                .iter()
                .map(|(k, v)| format!("{k}: {}", emit_value(v)))
                .collect();
            let _ = writeln!(s, "    - {{{}}}", parts.join(", "));
        }
    }

    s
}

/// Is the index represented as a `unique` facet on its column?
fn is_inline_unique(t: &Table, ix: &Index) -> bool {
    ix.is_unique
        && ix.columns.len() == 1
        && ix.method.is_none()
        && ix.operators.is_empty()
        && ix.filter.is_none()
        && ix.expression.is_none()
        && ix.name == molde_core::conventions::index_name(&t.name, &ix.columns)
}

fn emit_field(_t: &Table, col: &Column, pk: Option<&str>, uniq: bool) -> String {
    let logical = col.clr_type.as_deref().and_then(clr_to_logical);
    let mut emit_dbtype = false;
    let mut type_tok = match logical {
        Some(l) => {
            if col.store_type.is_some() {
                emit_dbtype = true;
            }
            l.to_string()
        }
        None => match &col.store_type {
            Some(st) => quote_scalar(st),
            None => "string".to_string(),
        },
    };
    if col.is_nullable {
        type_tok.push('?');
    }

    let mut facets: Vec<String> = Vec::new();
    if let Some(name) = pk {
        if name == molde_core::conventions::pk_name(&_t.name) {
            facets.push("pk".to_string());
        } else {
            facets.push(format!("pk={}", quote_scalar(name)));
        }
    }
    if col.is_identity {
        facets.push("identity".to_string());
    }
    if uniq {
        facets.push("unique".to_string());
    }
    if let Some(n) = col.max_length {
        facets.push(format!("maxlen={n}"));
    }
    if let Some(p) = col.precision {
        match col.scale {
            Some(sc) => facets.push(format!("precision={p},{sc}")),
            None => facets.push(format!("precision={p}")),
        }
    }
    if let Some(d) = &col.default_value_sql {
        facets.push(format!("default={}", quote_scalar(d)));
    }
    if let Some(c) = &col.computed_sql {
        facets.push(format!("computed={}", quote_scalar(c)));
        if col.computed_stored {
            facets.push("stored".to_string());
        }
    }
    if let Some(c) = &col.collation {
        facets.push(format!("collation={}", quote_scalar(c)));
    }
    if emit_dbtype {
        if let Some(st) = &col.store_type {
            facets.push(format!("dbtype={}", quote_scalar(st)));
        }
    }
    if logical.is_none() {
        if let Some(clr) = &col.clr_type {
            facets.push(format!("clr={}", quote_scalar(clr)));
        }
    }
    if let Some(cm) = &col.comment {
        facets.push(format!("comment={}", quote_scalar(cm)));
    }

    let mut line = format!("{}: {type_tok}", col.name);
    if !facets.is_empty() {
        line.push(' ');
        line.push_str(&facets.join(" "));
    }
    line
}

fn emit_fk(t: &Table, fk: &ForeignKey) -> String {
    let mut parts: Vec<String> = Vec::new();
    // fk
    if fk.columns.len() == 1 {
        parts.push(format!("fk: {}", fk.columns[0]));
    } else {
        parts.push(format!("fk: [{}]", fk.columns.join(", ")));
    }
    // references
    let principal = match &fk.principal_schema {
        Some(ps) => format!("{ps}.{}", fk.principal_table),
        None => fk.principal_table.clone(),
    };
    let refs = if fk.principal_columns.len() == 1 {
        format!("{principal}.{}", fk.principal_columns[0])
    } else {
        format!("{principal}.[{}]", fk.principal_columns.join(", "))
    };
    parts.push(format!("references: {refs}"));
    // onDelete (only if it is not the default value)
    if fk.on_delete != ReferentialAction::NoAction {
        parts.push(format!("onDelete: {}", action_name(fk.on_delete)));
    }
    // name (only if it is not conventional)
    if fk.name != molde_core::conventions::fk_name(&t.name, &fk.principal_table) {
        parts.push(format!("name: {}", quote_scalar(&fk.name)));
    }
    format!("{{{}}}", parts.join(", "))
}

fn emit_index(ix: &Index) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("on: [{}]", ix.columns.join(", ")));
    if ix.is_unique {
        parts.push("unique: true".to_string());
    }
    if let Some(m) = &ix.method {
        parts.push(format!("method: {}", quote_scalar(m)));
    }
    if !ix.operators.is_empty() {
        let ops: Vec<String> = ix.operators.iter().map(|o| quote_scalar(o)).collect();
        parts.push(format!("operators: [{}]", ops.join(", ")));
    }
    if let Some(f) = &ix.filter {
        parts.push(format!("filter: {}", quote_scalar(f)));
    }
    if let Some(e) = &ix.expression {
        parts.push(format!("expression: {}", quote_scalar(e)));
    }
    format!("{{{}}}", parts.join(", "))
}

fn emit_trigger(s: &mut String, tg: &Trigger) {
    let _ = writeln!(s, "    - {}:", tg.name);
    let _ = writeln!(s, "        timing: {}", timing_name(tg.timing));
    let events: Vec<&str> = tg.events.iter().map(|e| event_name(*e)).collect();
    let _ = writeln!(s, "        events: [{}]", events.join(", "));
    if let Some(f) = &tg.function {
        let _ = writeln!(s, "        function: {}", quote_scalar(f));
    }
    let _ = writeln!(s, "        sql: |");
    emit_block(s, &tg.definition, 10);
}

/// Emits multi-line text indented to `indent` spaces (block scalar `|`).
fn emit_block(s: &mut String, text: &str, indent: usize) {
    let pad = " ".repeat(indent);
    for line in text.lines() {
        if line.is_empty() {
            let _ = writeln!(s);
        } else {
            let _ = writeln!(s, "{pad}{line}");
        }
    }
}

pub(crate) fn action_name(a: ReferentialAction) -> &'static str {
    match a {
        ReferentialAction::NoAction => "no_action",
        ReferentialAction::Restrict => "restrict",
        ReferentialAction::Cascade => "cascade",
        ReferentialAction::SetNull => "set_null",
        ReferentialAction::SetDefault => "set_default",
    }
}

fn timing_name(t: TriggerTiming) -> &'static str {
    match t {
        TriggerTiming::Before => "before",
        TriggerTiming::After => "after",
        TriggerTiming::InsteadOf => "instead_of",
    }
}

fn event_name(e: TriggerEvent) -> &'static str {
    match e {
        TriggerEvent::Insert => "insert",
        TriggerEvent::Update => "update",
        TriggerEvent::Delete => "delete",
        TriggerEvent::Truncate => "truncate",
    }
}
