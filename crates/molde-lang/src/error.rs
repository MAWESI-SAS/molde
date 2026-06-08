//! Errores del lenguaje molde, con diagnósticos legibles: línea, columna y un
//! recorte (snippet) de la línea ofensora con un cursor `^` debajo.

use std::fmt;

/// Error de parseo de un archivo `.model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoldeError {
    /// Línea (1-based) donde se detectó el problema; 0 si no aplica.
    pub line: usize,
    /// Columna (1-based) del cursor `^`, si se pudo localizar.
    pub column: Option<usize>,
    pub message: String,
    /// Nombre del archivo de origen, si se conoce (lo aporta `parse_project`).
    pub file: Option<String>,
    /// Texto de la línea ofensora (sin blancos finales), si se adjuntó la fuente.
    pub snippet: Option<String>,
}

impl MoldeError {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column: None,
            message: message.into(),
            file: None,
            snippet: None,
        }
    }

    /// Adjunta la fuente para enriquecer el error con la línea ofensora y, en lo
    /// posible, la columna (deducida del primer fragmento entre comillas simples
    /// del mensaje, p. ej. `faceta desconocida: 'uniq'`). Idempotente: si ya hay
    /// snippet, no lo pisa. Llamar en la frontera pública del parser.
    pub fn with_source(mut self, src: &str) -> Self {
        if self.line == 0 || self.snippet.is_some() {
            return self;
        }
        if let Some(raw) = src.lines().nth(self.line - 1) {
            let text = raw.trim_end();
            self.column = quoted_fragment(&self.message)
                .and_then(|frag| char_column(text, &frag))
                .or_else(|| first_nonspace_column(text));
            self.snippet = Some(text.to_string());
        }
        self
    }

    /// Etiqueta el error con el archivo de origen (contexto de `parse_project`).
    pub fn in_file(mut self, name: impl Into<String>) -> Self {
        self.file = Some(name.into());
        self
    }

    /// Cabecera de una línea: `[archivo:]línea L[, columna C]`.
    fn location(&self) -> String {
        let mut loc = String::new();
        if let Some(f) = &self.file {
            loc.push_str(f);
            if self.line > 0 {
                loc.push(':');
            }
        }
        if self.line > 0 {
            loc.push_str(&format!("línea {}", self.line));
            if let Some(c) = self.column {
                loc.push_str(&format!(", columna {c}"));
            }
        }
        loc
    }
}

impl fmt::Display for MoldeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = self.location();
        if loc.is_empty() {
            return write!(f, "{}", self.message);
        }
        write!(f, "{}: {}", loc, self.message)?;
        // Diagnóstico tipo compilador: la línea ofensora + un cursor `^`.
        if let Some(snippet) = &self.snippet {
            let num = self.line.to_string();
            let gutter = " ".repeat(num.len());
            write!(f, "\n{num} | {snippet}")?;
            if let Some(col) = self.column {
                let pad = " ".repeat(col.saturating_sub(1));
                write!(f, "\n{gutter} | {pad}^")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for MoldeError {}

pub type Result<T> = std::result::Result<T, MoldeError>;

/// Primer fragmento entre comillas simples del mensaje (`'…'`), si existe.
fn quoted_fragment(msg: &str) -> Option<String> {
    let start = msg.find('\'')? + 1;
    let end = msg[start..].find('\'')? + start;
    Some(msg[start..end].to_string())
}

/// Columna (1-based, en caracteres) donde aparece `needle` dentro de `line`.
fn char_column(line: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let byte = line.find(needle)?;
    Some(line[..byte].chars().count() + 1)
}

/// Columna (1-based) del primer carácter no blanco de `line`.
fn first_nonspace_column(line: &str) -> Option<usize> {
    line.char_indices()
        .position(|(_, c)| !c.is_whitespace())
        .map(|p| p + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_fuente_muestra_solo_linea_y_mensaje() {
        let e = MoldeError::new(3, "algo salió mal");
        assert_eq!(e.to_string(), "línea 3: algo salió mal");
    }

    #[test]
    fn sin_linea_muestra_solo_mensaje() {
        let e = MoldeError::new(0, "archivo vacío");
        assert_eq!(e.to_string(), "archivo vacío");
    }

    #[test]
    fn with_source_localiza_columna_del_token_citado() {
        let src = "Customer:\n  fields:\n    Email: string uniq\n";
        let e = MoldeError::new(3, "faceta desconocida: 'uniq'").with_source(src);
        assert_eq!(e.column, Some(19));
        assert_eq!(e.snippet.as_deref(), Some("    Email: string uniq"));
        let shown = e.to_string();
        assert!(shown.contains("línea 3, columna 19: faceta desconocida: 'uniq'"));
        assert!(shown.contains("3 |     Email: string uniq"));
        // El cursor cae justo bajo la 'u' de uniq (columna 19 → 18 espacios).
        assert!(shown.contains(&format!("\n  | {}^", " ".repeat(18))));
    }

    #[test]
    fn with_source_sin_token_apunta_al_primer_no_blanco() {
        let src = "  Customer\n";
        let e = MoldeError::new(1, "sección desconocida").with_source(src);
        assert_eq!(e.column, Some(3));
    }

    #[test]
    fn in_file_prefija_el_nombre() {
        let e = MoldeError::new(2, "x").in_file("Customer.model");
        assert_eq!(e.to_string(), "Customer.model:línea 2: x");
    }

    #[test]
    fn with_source_es_idempotente() {
        let src = "uno\ndos\n";
        let e = MoldeError::new(1, "err").with_source(src);
        let snippet = e.snippet.clone();
        let e2 = e.with_source("otra\nfuente\n");
        assert_eq!(e2.snippet, snippet);
    }
}
