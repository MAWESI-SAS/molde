//! Canonical formatting of a `.model` file: parses and re-emits in the flat
//! canonical form. It is the same operation used by the CLI (`molde fmt`) and the
//! language server. Idempotent: `format_model(format_model(x)) == format_model(x)`.

use molde_core::model::DatabaseModel;

use crate::emit::{emit_database, emit_entity};
use crate::error::Result;
use crate::parse::{parse_database, parse_entity};

/// The name of the global file of a models project.
pub const DATABASE_FILE: &str = "database.model";

/// Formats the contents of a `.model` file to its canonical form.
///
/// Dispatch depends on the name: `database.model` is treated as the globals file
/// (schema/extensions/functions/raw); any other, as an entity.
/// Returns `MoldeError` (with line/column) if the contents do not parse.
pub fn format_model(name: &str, src: &str) -> Result<String> {
    if name == DATABASE_FILE {
        let g = parse_database(src)?;
        let mut m = DatabaseModel::empty();
        m.default_schema = g.default_schema;
        m.product_version = g.product_version;
        m.extensions = g.extensions;
        m.functions = g.functions;
        m.raw_objects = g.raw_objects;
        Ok(emit_database(&m))
    } else {
        let t = parse_entity(src)?;
        Ok(emit_entity(&t, &DatabaseModel::empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_entity_to_canonical() {
        // Messy input (extra spaces, unaligned columns).
        let messy = "Customer\n  fields:\n    Id:    int   pk identity\n    Email:  string?   unique maxlen=200\n";
        let out = format_model("Customer.model", messy).unwrap();
        // Re-formatting is idempotent.
        assert_eq!(format_model("Customer.model", &out).unwrap(), out);
        // The semantic content is preserved (same columns and facets).
        assert!(out.contains("Id:"));
        assert!(out.contains("identity"));
        assert!(out.contains("Email:"));
        assert!(out.contains("maxlen=200"));
    }

    #[test]
    fn formats_database_model() {
        let src = "schema: public\nextensions: [pg_trgm, unaccent]\n";
        let out = format_model(DATABASE_FILE, src).unwrap();
        assert_eq!(format_model(DATABASE_FILE, &out).unwrap(), out);
        assert!(out.contains("schema: public"));
        assert!(out.contains("pg_trgm"));
    }

    #[test]
    fn parse_error_propagates_with_line() {
        let bad = "Customer\n  fields:\n    Email: string nope\n";
        let err = format_model("Customer.model", bad).unwrap_err();
        assert_eq!(err.line, 3);
    }
}
