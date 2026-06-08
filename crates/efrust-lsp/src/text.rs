//! Utilidades de texto: conversión de posiciones LSP (UTF-16) ↔ índice de
//! carácter, palabra bajo el cursor, extracción de FK y mapeo a símbolos.

use efrust_lang::{EfmError, OutlineItem};
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, Position, Range, SymbolKind, Url,
};

/// Nombre de archivo (último segmento) de un `Url`.
pub fn uri_file_name(uri: &Url) -> String {
    uri.path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or("entity.model")
        .to_string()
}

/// La n-ésima línea (0-based) de `text`, o `""` si no existe.
pub fn nth_line(text: &str, n: usize) -> &str {
    text.lines().nth(n).unwrap_or("")
}

/// Convierte un offset en unidades UTF-16 (lo que envía el cliente LSP) al índice
/// de carácter dentro de `line`.
pub fn utf16_to_char_idx(line: &str, utf16: usize) -> usize {
    let mut units = 0usize;
    for (ci, ch) in line.chars().enumerate() {
        if units >= utf16 {
            return ci;
        }
        units += ch.len_utf16();
    }
    line.chars().count()
}

/// Convierte un índice de carácter al offset UTF-16 correspondiente en `line`.
pub fn char_to_utf16(line: &str, char_idx: usize) -> u32 {
    line.chars()
        .take(char_idx)
        .map(|c| c.len_utf16() as u32)
        .sum()
}

/// Identificador (`[A-Za-z0-9_]`) bajo el cursor (índice de carácter).
pub fn word_at(line: &str, char_idx: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if char_idx > chars.len() {
        return None;
    }
    let is_id = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut start = char_idx;
    while start > 0 && is_id(chars[start - 1]) {
        start -= 1;
    }
    let mut end = char_idx;
    while end < chars.len() && is_id(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

/// Si la línea contiene una FK (`... references: Tabla.col ...`), devuelve la
/// tabla referenciada.
pub fn references_target(line: &str) -> Option<String> {
    let pos = line.find("references:")? + "references:".len();
    let rest = line[pos..].trim_start();
    let table: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if table.is_empty() {
        None
    } else {
        Some(table)
    }
}

/// Rango que cubre todo el documento (para reemplazo total al formatear).
pub fn whole_range(text: &str) -> Range {
    let last_line = text.lines().count().saturating_sub(1);
    let last = text.lines().last().unwrap_or("");
    let end = char_to_utf16(last, last.chars().count());
    Range::new(Position::new(0, 0), Position::new(last_line as u32, end))
}

/// Construye un diagnóstico a partir de un `EfmError`, subrayando desde la
/// columna hasta el final de la línea ofensora.
pub fn diagnostic_from(e: &EfmError, text: &str) -> Diagnostic {
    let line0 = e.line.saturating_sub(1);
    let line_text = nth_line(text, line0);
    let start = e
        .column
        .map(|c| char_to_utf16(line_text, c.saturating_sub(1)))
        .unwrap_or(0);
    let end = char_to_utf16(line_text, line_text.chars().count()).max(start + 1);
    Diagnostic {
        range: Range::new(
            Position::new(line0 as u32, start),
            Position::new(line0 as u32, end),
        ),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("efrust".into()),
        message: e.message.clone(),
        ..Default::default()
    }
}

/// Mapea un `OutlineItem` a un `DocumentSymbol` (recursivo).
pub fn to_symbol(item: &OutlineItem, lines: &[&str], depth: usize) -> DocumentSymbol {
    let line0 = item.line.saturating_sub(1) as u32;
    let line_text = lines.get(line0 as usize).copied().unwrap_or("");
    let end = char_to_utf16(line_text, line_text.chars().count());
    let range = Range::new(Position::new(line0, 0), Position::new(line0, end));
    let kind = match depth {
        0 => SymbolKind::CLASS,     // entidad
        1 => SymbolKind::NAMESPACE, // sección (fields, indexes, …)
        _ => SymbolKind::FIELD,     // campo / ítem
    };
    let detail = if item.detail.trim().is_empty() {
        None
    } else {
        Some(truncate(item.detail.trim(), 60))
    };
    let children: Vec<DocumentSymbol> = item
        .children
        .iter()
        .map(|c| to_symbol(c, lines, depth + 1))
        .collect();
    #[allow(deprecated)]
    DocumentSymbol {
        name: item.label.clone(),
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_target_extrae_la_tabla() {
        let line = "    Tenants: {fk: tenantId, references: Tenants.id, onDelete: cascade}";
        assert_eq!(references_target(line).as_deref(), Some("Tenants"));
        assert_eq!(references_target("    Id: int pk"), None);
    }

    #[test]
    fn word_at_encuentra_identificador() {
        let line = "    Email: string?";
        // En la 'm' de Email (índice 5).
        assert_eq!(word_at(line, 5).as_deref(), Some("Email"));
        // En un espacio.
        assert_eq!(word_at(line, 3), None);
    }

    #[test]
    fn utf16_y_char_idx_son_inversos() {
        // 🛈 = 2 unidades UTF-16. La columna en chars y en UTF-16 difiere tras él.
        let line = "# 🛈 nota: Email";
        // 'E' de Email: índice de carácter.
        let cidx = line.chars().position(|c| c == 'E').unwrap();
        let u = char_to_utf16(line, cidx);
        assert_eq!(utf16_to_char_idx(line, u as usize), cidx);
        assert_eq!(word_at(line, cidx).as_deref(), Some("Email"));
    }
}
