//! Parser: `.model` text → IR. Accepts the canonical form and the authoring
//! sugar (`owns`, `subtypes`/`discriminator`, `enum[…]`).

use std::collections::BTreeMap;

use molde_core::model::{
    CheckConstraint, Column, DatabaseModel, DbFunction, ForeignKey, Index, PrimaryKey,
    ReferentialAction, Table, Trigger, TriggerEvent, TriggerTiming, View, IR_FORMAT_VERSION,
};
use serde_json::Value;

use crate::error::{MoldeError, Result};
use crate::tree::{parse_tree, Node};
use crate::types::{is_logical, logical_to_clr};
use crate::value::{parse_value, split_top_level, tokenize_ws, unquote};

/// Globals read from `database.model`.
#[derive(Debug, Default)]
pub struct DbGlobals {
    pub default_schema: Option<String>,
    pub product_version: Option<String>,
    pub extensions: Vec<String>,
    pub functions: Vec<DbFunction>,
    pub raw_objects: Vec<String>,
    pub views: Vec<View>,
}

/// Parses a full project: each file `(name, contents)`. The `database.model`
/// file provides the globals; the rest, one table each.
pub fn parse_project(files: &[(&str, &str)]) -> Result<DatabaseModel> {
    let mut model = DatabaseModel::empty();
    model.format_version = IR_FORMAT_VERSION;
    for (name, contents) in files {
        if *name == "database.model" {
            let g = parse_database(contents).map_err(|e| e.in_file(*name))?;
            model.default_schema = g.default_schema;
            model.product_version = g.product_version;
            model.extensions = g.extensions;
            model.functions = g.functions;
            model.raw_objects = g.raw_objects;
            model.views = g.views;
        } else {
            model
                .tables
                .push(parse_entity(contents).map_err(|e| e.in_file(*name))?);
        }
    }
    Ok(model)
}

/// Parses the global `database.model` file.
pub fn parse_database(src: &str) -> Result<DbGlobals> {
    parse_database_inner(src).map_err(|e| e.with_source(src))
}

fn parse_database_inner(src: &str) -> Result<DbGlobals> {
    let nodes = parse_tree(src)?;
    let mut g = DbGlobals::default();
    for node in &nodes {
        match node.key.as_str() {
            "schema" => g.default_schema = Some(unquote(&node.inline)),
            "product-version" => g.product_version = Some(unquote(&node.inline)),
            "extensions" => {
                if let Value::Array(arr) = parse_value(&node.inline, node.line)? {
                    g.extensions = arr.iter().filter_map(value_str).collect();
                }
            }
            "functions" => {
                for item in &node.children {
                    let (label, _) = item.as_kv();
                    let (schema, fname) = split_schema(&label);
                    g.functions.push(DbFunction {
                        name: fname,
                        schema,
                        definition: item.block.clone().unwrap_or_default(),
                    });
                }
            }
            "raw" => {
                for item in &node.children {
                    g.raw_objects.push(item.block.clone().unwrap_or_default());
                }
            }
            "views" => {
                for item in &node.children {
                    let (label, _) = item.as_kv();
                    let (schema, vname) = split_schema(&label);
                    g.views.push(View {
                        name: vname,
                        schema,
                        definition: item.block.clone().unwrap_or_default(),
                    });
                }
            }
            other => {
                return Err(MoldeError::new(
                    node.line,
                    format!("unknown key in database.model: '{other}'"),
                ))
            }
        }
    }
    Ok(g)
}

/// Parses an entity file into a `Table`.
pub fn parse_entity(src: &str) -> Result<Table> {
    parse_entity_inner(src).map_err(|e| e.with_source(src))
}

fn parse_entity_inner(src: &str) -> Result<Table> {
    let nodes = parse_tree(src)?;
    let header = nodes
        .first()
        .ok_or_else(|| MoldeError::new(0, "empty entity file"))?;
    if nodes.len() > 1 {
        return Err(MoldeError::new(
            nodes[1].line,
            "an entity file defines a single table",
        ));
    }
    let table_name = if header.inline.is_empty() {
        header.key.clone()
    } else {
        unquote(&header.inline)
    };

    let mut table = Table {
        name: table_name,
        schema: None,
        clr_type: None,
        comment: None,
        columns: Vec::new(),
        primary_key: None,
        foreign_keys: Vec::new(),
        indexes: Vec::new(),
        triggers: Vec::new(),
        check_constraints: Vec::new(),
        seed_data: Vec::new(),
        seed_key: Vec::new(),
    };

    let mut builds: Vec<ColumnBuild> = Vec::new();
    let mut discriminator: Option<String> = None;
    let mut subtypes_node: Option<&Node> = None;
    let mut key_block: Option<PrimaryKey> = None;
    let mut fk_opted_out: Vec<bool> = Vec::new();

    for section in &header.children {
        match section.key.as_str() {
            "clr" => table.clr_type = Some(unquote(&section.inline)),
            "schema" => table.schema = Some(unquote(&section.inline)),
            "comment" => table.comment = Some(unquote(&section.inline)),
            "discriminator" => discriminator = Some(unquote(&section.inline)),
            "fields" => {
                for f in &section.children {
                    builds.extend(parse_field(f, false)?);
                }
            }
            "key" => key_block = Some(parse_key(&table.name, section)?),
            "belongs-to" => {
                for fk in &section.children {
                    let (fk, opted_out) = parse_fk(&table.name, fk)?;
                    fk_opted_out.push(opted_out);
                    table.foreign_keys.push(fk);
                }
            }
            "indexes" => {
                for ix in &section.children {
                    table.indexes.push(parse_index(ix)?);
                }
            }
            "triggers" => {
                for tg in &section.children {
                    table.triggers.push(parse_trigger(&table, tg)?);
                }
            }
            "checks" => {
                for ck in &section.children {
                    let (name, inline) = ck.as_kv();
                    table.check_constraints.push(CheckConstraint {
                        name,
                        expression: unquote(&inline),
                    });
                }
            }
            "seed" => {
                for row in &section.children {
                    table.seed_data.push(parse_seed_row(row)?);
                }
            }
            "seed-key" => {
                if let Value::Array(arr) = parse_value(&section.inline, section.line)? {
                    table.seed_key = arr.iter().filter_map(value_str).collect();
                }
            }
            "subtypes" => subtypes_node = Some(section),
            other => {
                return Err(MoldeError::new(
                    section.line,
                    format!("unknown section in entity: '{other}'"),
                ))
            }
        }
    }

    // TPH sugar: discriminator column + subtype columns (nullable).
    if let Some(disc) = &discriminator {
        if !builds.iter().any(|b| b.col.name == *disc) {
            builds.push(ColumnBuild {
                col: Column {
                    name: disc.clone(),
                    store_type: None,
                    clr_type: Some("System.String".to_string()),
                    is_nullable: false,
                    is_identity: false,
                    max_length: None,
                    precision: None,
                    scale: None,
                    default_value_sql: None,
                    computed_sql: None,
                    computed_stored: false,
                    collation: None,
                    comment: None,
                },
                pk: None,
                unique: false,
            });
        }
    }
    if let Some(sub) = subtypes_node {
        for st in &sub.children {
            let (_name, inner) = st.as_kv();
            for part in split_top_level(inner.trim_matches(|c| c == '{' || c == '}'), ',') {
                if part.is_empty() {
                    continue;
                }
                let (fname, frest) = part
                    .split_once(':')
                    .ok_or_else(|| MoldeError::new(st.line, "invalid subtype field"))?;
                let mut cb = parse_field_parts(fname.trim(), frest.trim(), st.line, true)?;
                for b in &mut cb {
                    if !builds.iter().any(|x| x.col.name == b.col.name) {
                        builds.push(b.clone());
                    }
                }
            }
        }
    }

    // Dump columns + inline unique indexes + PK.
    let mut pk_cols: Vec<String> = Vec::new();
    let mut pk_name: Option<String> = None;
    for b in builds {
        if let Some(name) = &b.pk {
            pk_cols.push(b.col.name.clone());
            if let Some(n) = name {
                pk_name = Some(n.clone());
            }
        }
        if b.unique {
            table.indexes.push(Index {
                name: molde_core::conventions::index_name(
                    &table.name,
                    std::slice::from_ref(&b.col.name),
                ),
                columns: vec![b.col.name.clone()],
                is_unique: true,
                filter: None,
                method: None,
                operators: Vec::new(),
                expression: None,
            });
        }
        table.columns.push(b.col);
    }

    table.primary_key = if let Some(pk) = key_block {
        Some(pk)
    } else if !pk_cols.is_empty() {
        Some(PrimaryKey {
            name: pk_name.unwrap_or_else(|| molde_core::conventions::pk_name(&table.name)),
            columns: pk_cols,
        })
    } else {
        None
    };

    // EF-style convention: each foreign key gets a backing index unless it
    // already has a covering one or opted out with `index: false`.
    crate::fk_index::synthesize(&mut table, &fk_opted_out);

    Ok(table)
}

/// Intermediate result of parsing a field.
#[derive(Debug, Clone)]
struct ColumnBuild {
    col: Column,
    /// `Some(None)` = PK with conventional name; `Some(Some(n))` = PK with name.
    pk: Option<Option<String>>,
    unique: bool,
}

/// Parses a field line `Name: type facets…`. May produce several columns
/// (owned types). `force_nullable` is used for TPH subtype columns.
fn parse_field(node: &Node, force_nullable: bool) -> Result<Vec<ColumnBuild>> {
    let (name, rest) = node.as_kv();
    parse_field_parts(&name, &rest, node.line, force_nullable)
}

fn parse_field_parts(
    name: &str,
    rest: &str,
    line: usize,
    force_nullable: bool,
) -> Result<Vec<ColumnBuild>> {
    let tokens = tokenize_ws(rest);
    if tokens.is_empty() {
        return Err(MoldeError::new(
            line,
            format!("field '{name}' without type"),
        ));
    }

    // Owned type: `owns Type {fields…}` → prefixed columns.
    if tokens[0] == "owns" {
        let inner = tokens.iter().skip(2).cloned().collect::<Vec<_>>().join(" ");
        let inner = inner.trim().trim_matches(|c| c == '{' || c == '}');
        let mut out = Vec::new();
        for part in split_top_level(inner, ',') {
            if part.is_empty() {
                continue;
            }
            let (oname, orest) = part
                .split_once(':')
                .ok_or_else(|| MoldeError::new(line, "invalid owned field"))?;
            let mut cb = parse_field_parts(oname.trim(), orest.trim(), line, false)?;
            for b in &mut cb {
                b.col.name = format!("{name}_{}", b.col.name);
            }
            out.extend(cb);
        }
        return Ok(out);
    }

    // Enum: `enum[A,B] as=string|int` → scalar column; values discarded.
    let (type_token, facet_tokens): (String, Vec<String>) = if tokens[0].starts_with("enum[") {
        let nullable = tokens[0].ends_with('?');
        let mut as_type = "string".to_string();
        let mut facets = Vec::new();
        for t in &tokens[1..] {
            if let Some(v) = t.strip_prefix("as=") {
                as_type = v.to_string();
            } else {
                facets.push(t.clone());
            }
        }
        let tt = if nullable {
            format!("{as_type}?")
        } else {
            as_type
        };
        (tt, facets)
    } else {
        (tokens[0].clone(), tokens[1..].to_vec())
    };

    let mut cb = parse_column(name, &type_token, &facet_tokens, line)?;
    if force_nullable {
        cb.col.is_nullable = true;
    }
    Ok(vec![cb])
}

fn parse_column(
    name: &str,
    type_token: &str,
    facets: &[String],
    line: usize,
) -> Result<ColumnBuild> {
    let (type_text, nullable, was_quoted) = parse_type_token(type_token);

    let mut col = Column {
        name: name.to_string(),
        store_type: None,
        clr_type: None,
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
    };

    if !was_quoted && is_logical(&type_text) {
        col.clr_type = logical_to_clr(&type_text).map(String::from);
    } else {
        col.store_type = Some(type_text);
    }

    let mut pk: Option<Option<String>> = None;
    let mut unique = false;
    for f in facets {
        if f == "pk" {
            pk = Some(None);
        } else if let Some(v) = f.strip_prefix("pk=") {
            pk = Some(Some(unquote(v)));
        } else if f == "identity" {
            col.is_identity = true;
        } else if f == "unique" {
            unique = true;
        } else if f == "stored" {
            col.computed_stored = true;
        } else if let Some(v) = f.strip_prefix("maxlen=") {
            col.max_length = Some(
                v.parse()
                    .map_err(|_| MoldeError::new(line, format!("invalid maxlen: '{v}'")))?,
            );
        } else if let Some(v) = f.strip_prefix("precision=") {
            let mut it = v.split(',');
            let p = it
                .next()
                .and_then(|x| x.trim().parse().ok())
                .ok_or_else(|| MoldeError::new(line, format!("invalid precision: '{v}'")))?;
            col.precision = Some(p);
            if let Some(s) = it.next() {
                col.scale = Some(
                    s.trim()
                        .parse()
                        .map_err(|_| MoldeError::new(line, format!("invalid scale: '{s}'")))?,
                );
            }
        } else if let Some(v) = f.strip_prefix("default=") {
            col.default_value_sql = Some(unquote(v));
        } else if let Some(v) = f.strip_prefix("computed=") {
            col.computed_sql = Some(unquote(v));
        } else if let Some(v) = f.strip_prefix("collation=") {
            col.collation = Some(unquote(v));
        } else if let Some(v) = f.strip_prefix("dbtype=") {
            col.store_type = Some(unquote(v));
        } else if let Some(v) = f.strip_prefix("clr=") {
            col.clr_type = Some(unquote(v));
        } else if let Some(v) = f.strip_prefix("comment=") {
            col.comment = Some(unquote(v));
        } else if f.starts_with("as=") {
            // ignored outside enum
        } else {
            return Err(MoldeError::new(line, format!("unknown facet: '{f}'")));
        }
    }

    Ok(ColumnBuild { col, pk, unique })
}

/// Splits the type token into `(text, nullable, was_quoted)`.
fn parse_type_token(tok: &str) -> (String, bool, bool) {
    if tok.starts_with('"') {
        // Find the closing quote, respecting escapes.
        let bytes: Vec<char> = tok.chars().collect();
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == '\\' {
                i += 2;
                continue;
            }
            if bytes[i] == '"' {
                break;
            }
            i += 1;
        }
        let quoted: String = bytes[..=i.min(bytes.len() - 1)].iter().collect();
        let rest: String = bytes[(i + 1).min(bytes.len())..].iter().collect();
        let nullable = rest == "?";
        (unquote(&quoted), nullable, true)
    } else if let Some(stripped) = tok.strip_suffix('?') {
        (stripped.to_string(), true, false)
    } else {
        (tok.to_string(), false, false)
    }
}

fn parse_key(table_name: &str, node: &Node) -> Result<PrimaryKey> {
    let tokens = tokenize_ws(&node.inline);
    let mut columns = Vec::new();
    let mut name = molde_core::conventions::pk_name(table_name);
    for t in &tokens {
        if let Some(v) = t.strip_prefix("name=") {
            name = unquote(v);
        } else if t.starts_with('[') {
            let inner = t.trim_matches(|c| c == '[' || c == ']');
            columns = split_top_level(inner, ',')
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    if columns.is_empty() {
        return Err(MoldeError::new(node.line, "'key:' block without columns"));
    }
    Ok(PrimaryKey { name, columns })
}

/// Returns the foreign key and whether it opted out of the auto-index
/// (`index: false`).
fn parse_fk(table_name: &str, node: &Node) -> Result<(ForeignKey, bool)> {
    let (nav, inline) = node.as_kv();
    let obj = parse_value(&inline, node.line)?;
    let map = obj
        .as_object()
        .ok_or_else(|| MoldeError::new(node.line, "expected an object in belongs-to"))?;

    let columns = value_strlist(map.get("fk"))
        .ok_or_else(|| MoldeError::new(node.line, "FK without 'fk'"))?;
    let refs = map
        .get("references")
        .and_then(value_str)
        .ok_or_else(|| MoldeError::new(node.line, "FK without 'references'"))?;
    let (principal_schema, principal_table, principal_columns) =
        parse_references(&refs, node.line)?;
    let on_delete = match map.get("onDelete").and_then(value_str) {
        Some(s) => parse_action(&s, node.line)?,
        None => ReferentialAction::NoAction,
    };
    let on_update = match map.get("onUpdate").and_then(value_str) {
        Some(s) => parse_action(&s, node.line)?,
        None => ReferentialAction::NoAction,
    };
    let name = map
        .get("name")
        .and_then(value_str)
        .unwrap_or_else(|| molde_core::conventions::fk_name(table_name, &principal_table));
    // `index: false` opts out of the auto-generated backing index.
    let opted_out = matches!(map.get("index").and_then(|v| v.as_bool()), Some(false));
    let _ = nav;

    Ok((
        ForeignKey {
            name,
            columns,
            principal_table,
            principal_schema,
            principal_columns,
            on_delete,
            on_update,
        },
        opted_out,
    ))
}

/// Parses `[schema.]Table.Col` or `[schema.]Table.[A, B]`.
fn parse_references(s: &str, line: usize) -> Result<(Option<String>, String, Vec<String>)> {
    if let Some(pos) = s.find(".[") {
        let left = &s[..pos];
        let cols_raw = s[pos + 2..].trim_end_matches(']');
        let cols: Vec<String> = split_top_level(cols_raw, ',')
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect();
        let (schema, table) = split_principal(left);
        Ok((schema, table, cols))
    } else {
        let pos = s
            .rfind('.')
            .ok_or_else(|| MoldeError::new(line, format!("invalid reference: '{s}'")))?;
        let left = &s[..pos];
        let col = s[pos + 1..].to_string();
        let (schema, table) = split_principal(left);
        Ok((schema, table, vec![col]))
    }
}

fn split_principal(left: &str) -> (Option<String>, String) {
    match left.rsplit_once('.') {
        Some((sc, tbl)) => (Some(sc.to_string()), tbl.to_string()),
        None => (None, left.to_string()),
    }
}

fn parse_index(node: &Node) -> Result<Index> {
    let (name, inline) = node.as_kv();
    let obj = parse_value(&inline, node.line)?;
    let map = obj
        .as_object()
        .ok_or_else(|| MoldeError::new(node.line, "expected an index object"))?;
    let columns = value_strlist(map.get("on")).unwrap_or_default();
    Ok(Index {
        name,
        columns,
        is_unique: map.get("unique").and_then(Value::as_bool).unwrap_or(false),
        filter: map.get("filter").and_then(value_str),
        method: map.get("method").and_then(value_str),
        operators: value_strlist(map.get("operators")).unwrap_or_default(),
        expression: map.get("expression").and_then(value_str),
    })
}

fn parse_trigger(table: &Table, node: &Node) -> Result<Trigger> {
    let (name, _) = node.as_kv();
    let timing = node
        .child("timing")
        .map(|n| parse_timing(&n.inline, n.line))
        .transpose()?
        .ok_or_else(|| MoldeError::new(node.line, "trigger without 'timing'"))?;
    let events = match node.child("events") {
        Some(n) => {
            let v = parse_value(&n.inline, n.line)?;
            let mut evs = Vec::new();
            if let Value::Array(arr) = v {
                for e in arr {
                    if let Some(s) = value_str(&e) {
                        evs.push(parse_event(&s, n.line)?);
                    }
                }
            }
            evs
        }
        None => Vec::new(),
    };
    let function = node.child("function").map(|n| unquote(&n.inline));
    let definition = node
        .child("sql")
        .and_then(|n| n.block.clone())
        .unwrap_or_default();

    Ok(Trigger {
        name,
        table: table.name.clone(),
        schema: table.schema.clone(),
        timing,
        events,
        function,
        definition,
    })
}

fn parse_seed_row(node: &Node) -> Result<BTreeMap<String, Value>> {
    // A seed item is `- {..}`: the object is in the raw text of the item.
    let v = parse_value(&node.inline, node.line)?;
    let map = v
        .as_object()
        .ok_or_else(|| MoldeError::new(node.line, "invalid seed row"))?;
    Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn parse_action(s: &str, line: usize) -> Result<ReferentialAction> {
    Ok(match s {
        "no_action" | "none" => ReferentialAction::NoAction,
        "restrict" => ReferentialAction::Restrict,
        "cascade" => ReferentialAction::Cascade,
        "set_null" => ReferentialAction::SetNull,
        "set_default" => ReferentialAction::SetDefault,
        other => {
            return Err(MoldeError::new(
                line,
                format!("invalid onDelete: '{other}'"),
            ))
        }
    })
}

fn parse_timing(s: &str, line: usize) -> Result<TriggerTiming> {
    Ok(match s.trim() {
        "before" => TriggerTiming::Before,
        "after" => TriggerTiming::After,
        "instead_of" => TriggerTiming::InsteadOf,
        other => return Err(MoldeError::new(line, format!("invalid timing: '{other}'"))),
    })
}

fn parse_event(s: &str, line: usize) -> Result<TriggerEvent> {
    Ok(match s.trim() {
        "insert" => TriggerEvent::Insert,
        "update" => TriggerEvent::Update,
        "delete" => TriggerEvent::Delete,
        "truncate" => TriggerEvent::Truncate,
        other => return Err(MoldeError::new(line, format!("invalid event: '{other}'"))),
    })
}

fn split_schema(label: &str) -> (Option<String>, String) {
    match label.split_once('.') {
        Some((sc, name)) => (Some(sc.to_string()), name.to_string()),
        None => (None, label.to_string()),
    }
}

fn value_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_strlist(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        Some(Value::Array(a)) => Some(a.iter().filter_map(value_str).collect()),
        Some(other) => value_str(other).map(|s| vec![s]),
        None => None,
    }
}
