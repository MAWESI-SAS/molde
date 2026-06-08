//! The foreign-key index convention (EF-style): every `belongs-to` gets a
//! backing index by default, named `ix_<table>_<cols>` (lowercase, see
//! [`molde_core::conventions`]).
//!
//! Symmetry keeps `.model` files clean and round-trips exact:
//! - the **parser** synthesizes the index when a FK has no covering index and
//!   didn't opt out with `index: false`;
//! - the **emitter** hides that conventional index, and writes `index: false`
//!   when a FK has no covering index (so the opt-out / no-index state survives).

use molde_core::conventions;
use molde_core::model::{Index, Table};

/// Are `cols` the leading columns of the primary key or of an existing index?
/// (EF skips the FK index when the columns are already covered.)
pub(crate) fn columns_covered(table: &Table, cols: &[String]) -> bool {
    if let Some(pk) = &table.primary_key {
        if pk.columns.starts_with(cols) {
            return true;
        }
    }
    table.indexes.iter().any(|ix| ix.columns.starts_with(cols))
}

/// Is `ix` the auto-generated conventional index for one of the table's FKs?
/// Such an index is hidden by the emitter and re-synthesized by the parser.
pub(crate) fn is_conventional_fk_index(table: &Table, ix: &Index) -> bool {
    !ix.is_unique
        && ix.method.is_none()
        && ix.operators.is_empty()
        && ix.filter.is_none()
        && ix.expression.is_none()
        && ix.name == conventions::index_name(&table.name, &ix.columns)
        && table.foreign_keys.iter().any(|fk| fk.columns == ix.columns)
}

/// Add the conventional backing index for each FK that wants one and isn't
/// already covered. `opted_out[i]` corresponds to `table.foreign_keys[i]`.
pub(crate) fn synthesize(table: &mut Table, opted_out: &[bool]) {
    let mut new_indexes: Vec<Index> = Vec::new();
    for (i, fk) in table.foreign_keys.iter().enumerate() {
        if opted_out.get(i).copied().unwrap_or(false) {
            continue;
        }
        if columns_covered(table, &fk.columns)
            || new_indexes.iter().any(|ix| ix.columns == fk.columns)
        {
            continue;
        }
        new_indexes.push(Index {
            name: conventions::index_name(&table.name, &fk.columns),
            columns: fk.columns.clone(),
            is_unique: false,
            method: None,
            operators: Vec::new(),
            filter: None,
            expression: None,
        });
    }
    table.indexes.extend(new_indexes);
}
