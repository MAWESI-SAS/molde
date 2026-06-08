//! molde language errors, with readable diagnostics: line, column and a
//! snippet of the offending line with a `^` cursor underneath.

use std::fmt;

/// Parse error of a `.model` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoldeError {
    /// Line (1-based) where the problem was detected; 0 if not applicable.
    pub line: usize,
    /// Column (1-based) of the `^` cursor, if it could be located.
    pub column: Option<usize>,
    pub message: String,
    /// Name of the source file, if known (provided by `parse_project`).
    pub file: Option<String>,
    /// Text of the offending line (without trailing whitespace), if the source was attached.
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

    /// Attaches the source to enrich the error with the offending line and, when
    /// possible, the column (deduced from the first single-quoted fragment of the
    /// message, e.g. `unknown facet: 'uniq'`). Idempotent: if a snippet
    /// already exists, it is not overwritten. Call at the parser's public boundary.
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

    /// Labels the error with the source file (context from `parse_project`).
    pub fn in_file(mut self, name: impl Into<String>) -> Self {
        self.file = Some(name.into());
        self
    }

    /// Header of a line: `[file:]line L[, column C]`.
    fn location(&self) -> String {
        let mut loc = String::new();
        if let Some(f) = &self.file {
            loc.push_str(f);
            if self.line > 0 {
                loc.push(':');
            }
        }
        if self.line > 0 {
            loc.push_str(&format!("line {}", self.line));
            if let Some(c) = self.column {
                loc.push_str(&format!(", column {c}"));
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
        // Compiler-style diagnostic: the offending line + a `^` cursor.
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

/// First single-quoted fragment of the message (`'…'`), if any.
fn quoted_fragment(msg: &str) -> Option<String> {
    let start = msg.find('\'')? + 1;
    let end = msg[start..].find('\'')? + start;
    Some(msg[start..end].to_string())
}

/// Column (1-based, in characters) where `needle` appears within `line`.
fn char_column(line: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let byte = line.find(needle)?;
    Some(line[..byte].chars().count() + 1)
}

/// Column (1-based) of the first non-whitespace character of `line`.
fn first_nonspace_column(line: &str) -> Option<usize> {
    line.char_indices()
        .position(|(_, c)| !c.is_whitespace())
        .map(|p| p + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_source_shows_only_line_and_message() {
        let e = MoldeError::new(3, "something went wrong");
        assert_eq!(e.to_string(), "line 3: something went wrong");
    }

    #[test]
    fn without_line_shows_only_message() {
        let e = MoldeError::new(0, "empty file");
        assert_eq!(e.to_string(), "empty file");
    }

    #[test]
    fn with_source_locates_column_of_quoted_token() {
        let src = "Customer:\n  fields:\n    Email: string uniq\n";
        let e = MoldeError::new(3, "unknown facet: 'uniq'").with_source(src);
        assert_eq!(e.column, Some(19));
        assert_eq!(e.snippet.as_deref(), Some("    Email: string uniq"));
        let shown = e.to_string();
        assert!(shown.contains("line 3, column 19: unknown facet: 'uniq'"));
        assert!(shown.contains("3 |     Email: string uniq"));
        // The cursor falls right under the 'u' of uniq (column 19 → 18 spaces).
        assert!(shown.contains(&format!("\n  | {}^", " ".repeat(18))));
    }

    #[test]
    fn with_source_without_token_points_to_first_nonspace() {
        let src = "  Customer\n";
        let e = MoldeError::new(1, "unknown section").with_source(src);
        assert_eq!(e.column, Some(3));
    }

    #[test]
    fn in_file_prefixes_the_name() {
        let e = MoldeError::new(2, "x").in_file("Customer.model");
        assert_eq!(e.to_string(), "Customer.model:line 2: x");
    }

    #[test]
    fn with_source_is_idempotent() {
        let src = "uno\ndos\n";
        let e = MoldeError::new(1, "err").with_source(src);
        let snippet = e.snippet.clone();
        let e2 = e.with_source("otra\nfuente\n");
        assert_eq!(e2.snippet, snippet);
    }
}
