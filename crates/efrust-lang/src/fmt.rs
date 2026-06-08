//! Formateo canónico de un archivo `.model`: parsea y re-emite en la forma
//! canónica plana. Es la misma operación que usa el CLI (`efrust fmt`) y el
//! language server. Idempotente: `format_model(format_model(x)) == format_model(x)`.

use efrust_core::model::DatabaseModel;

use crate::emit::{emit_database, emit_entity};
use crate::error::Result;
use crate::parse::{parse_database, parse_entity};

/// El nombre del archivo global de un proyecto de modelos.
pub const DATABASE_FILE: &str = "database.model";

/// Formatea el contenido de un archivo `.model` a su forma canónica.
///
/// El despacho depende del nombre: `database.model` se trata como el archivo de
/// globales (esquema/extensiones/funciones/raw); cualquier otro, como una entidad.
/// Devuelve `EfmError` (con línea/columna) si el contenido no parsea.
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
    fn formatea_entidad_a_canonico() {
        // Entrada desordenada (espacios extra, columnas sin alinear).
        let messy = "Customer\n  fields:\n    Id:    int   pk identity\n    Email:  string?   unique maxlen=200\n";
        let out = format_model("Customer.model", messy).unwrap();
        // Re-formatear es idempotente.
        assert_eq!(format_model("Customer.model", &out).unwrap(), out);
        // El contenido semántico se conserva (mismas columnas y facetas).
        assert!(out.contains("Id:"));
        assert!(out.contains("identity"));
        assert!(out.contains("Email:"));
        assert!(out.contains("maxlen=200"));
    }

    #[test]
    fn formatea_database_model() {
        let src = "schema: public\nextensions: [pg_trgm, unaccent]\n";
        let out = format_model(DATABASE_FILE, src).unwrap();
        assert_eq!(format_model(DATABASE_FILE, &out).unwrap(), out);
        assert!(out.contains("schema: public"));
        assert!(out.contains("pg_trgm"));
    }

    #[test]
    fn error_de_parseo_se_propaga_con_linea() {
        let bad = "Customer\n  fields:\n    Email: string nope\n";
        let err = format_model("Customer.model", bad).unwrap_err();
        assert_eq!(err.line, 3);
    }
}
