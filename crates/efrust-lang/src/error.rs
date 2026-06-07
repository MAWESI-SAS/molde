//! Errores del lenguaje EFM, con número de línea para diagnósticos legibles.

use std::fmt;

/// Error de parseo de un archivo `.model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EfmError {
    /// Línea (1-based) donde se detectó el problema; 0 si no aplica.
    pub line: usize,
    pub message: String,
}

impl EfmError {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for EfmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "línea {}: {}", self.line, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for EfmError {}

pub type Result<T> = std::result::Result<T, EfmError>;
