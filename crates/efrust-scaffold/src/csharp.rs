//! Utilidades de generación de C#: mapeo de tipos CLR → C#, pluralización y
//! formato de tipos de propiedad. Puro (sin dependencias de BD), fácil de testear.

use efrust_core::model::Column;

/// Mapea un tipo CLR (`System.Int32`) a (palabra clave C#, es_tipo_referencia).
/// Los tipos referencia no anulables se inicializan con `= null!;`.
pub fn clr_to_csharp(clr: &str) -> (&'static str, bool) {
    match clr {
        "System.Boolean" => ("bool", false),
        "System.Byte" => ("byte", false),
        "System.Int16" => ("short", false),
        "System.Int32" => ("int", false),
        "System.Int64" => ("long", false),
        "System.Single" => ("float", false),
        "System.Double" => ("double", false),
        "System.Decimal" => ("decimal", false),
        "System.DateTime" => ("DateTime", false),
        "System.DateTimeOffset" => ("DateTimeOffset", false),
        "System.TimeSpan" => ("TimeSpan", false),
        "System.Guid" => ("Guid", false),
        "System.String" => ("string", true),
        "System.Byte[]" => ("byte[]", true),
        // Tipos nativos Npgsql/pgvector (el CLR ya viene cualificado).
        "NpgsqlTypes.NpgsqlTsVector" => ("NpgsqlTypes.NpgsqlTsVector", true),
        "Pgvector.Vector" => ("Pgvector.Vector", true),
        _ => ("object", true),
    }
}

/// Devuelve el tipo C# completo de una columna, incluyendo `?` si es anulable.
pub fn property_type(column: &Column) -> String {
    let clr = column.clr_type.as_deref().unwrap_or("System.Object");
    let (kw, _is_ref) = clr_to_csharp(clr);
    if column.is_nullable {
        format!("{kw}?")
    } else {
        kw.to_string()
    }
}

/// Inicializador para tipos referencia requeridos (`= null!;`); vacío en otro caso.
pub fn property_initializer(column: &Column) -> &'static str {
    let clr = column.clr_type.as_deref().unwrap_or("System.Object");
    let (_kw, is_ref) = clr_to_csharp(clr);
    if is_ref && !column.is_nullable {
        " = null!;"
    } else {
        ""
    }
}

/// Pluralización aproximada estilo EF para los nombres de `DbSet`.
/// (La normalización completa de nombres queda para una fase posterior.)
pub fn pluralize(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix('y') {
        let last_vowel = stem.chars().last().map(is_vowel).unwrap_or(false);
        if !last_vowel {
            return format!("{}ies", &word[..word.len() - 1]);
        }
    }
    if lower.ends_with('s')
        || lower.ends_with('x')
        || lower.ends_with('z')
        || lower.ends_with("ch")
        || lower.ends_with("sh")
    {
        return format!("{word}es");
    }
    format!("{word}s")
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Convierte un identificador de BD a PascalCase para C#.
/// `created_at` → `CreatedAt`, `customer` → `Customer`, `Customer` → `Customer`.
/// Divide por `_`, espacio y `-`, capitaliza cada segmento y conserva el resto.
pub fn pascalize(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(|c: char| c == '_' || c == ' ' || c == '-') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        name.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluraliza_casos_comunes() {
        assert_eq!(pluralize("Customer"), "Customers");
        assert_eq!(pluralize("Category"), "Categories");
        assert_eq!(pluralize("Box"), "Boxes");
        assert_eq!(pluralize("Day"), "Days"); // vocal antes de 'y'
        assert_eq!(pluralize("Address"), "Addresses");
    }

    #[test]
    fn pascaliza_casos_comunes() {
        assert_eq!(pascalize("created_at"), "CreatedAt");
        assert_eq!(pascalize("customer"), "Customer");
        assert_eq!(pascalize("Customer"), "Customer");
        assert_eq!(pascalize("order_item_detail"), "OrderItemDetail");
        assert_eq!(pascalize("CustomerId"), "CustomerId");
    }
}
