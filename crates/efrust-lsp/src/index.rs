//! Índice del workspace: mapea cada nombre de tabla a su archivo `.model` y sus
//! columnas, para resolver navegación (go-to-definition) y dependencias
//! (find-references) entre entidades.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::Url;

/// Información de una entidad indexada.
#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub table: String,
    pub path: PathBuf,
    pub columns: Vec<String>,
}

/// Índice de todo el directorio de modelos.
#[derive(Debug, Default)]
pub struct Index {
    /// Clave: nombre de tabla y, como alias, el stem del archivo.
    by_table: HashMap<String, EntityInfo>,
}

impl Index {
    /// Reconstruye el índice escaneando `root` en busca de archivos `.model`.
    pub fn build(root: &Path) -> Self {
        let mut idx = Index::default();
        for path in model_paths(root) {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if name == efrust_lang::DATABASE_FILE {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(table) = efrust_lang::parse_entity(&src) {
                let info = EntityInfo {
                    table: table.name.clone(),
                    path: path.clone(),
                    columns: table.columns.iter().map(|c| c.name.clone()).collect(),
                };
                // Indexar por nombre de tabla y por stem del archivo (alias).
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    idx.by_table.entry(stem.to_string()).or_insert(info.clone());
                }
                idx.by_table.insert(info.table.clone(), info);
            }
        }
        idx
    }

    pub fn get(&self, table: &str) -> Option<&EntityInfo> {
        self.by_table.get(table)
    }

    /// Nombres de tabla conocidos (sin duplicar alias de stem que coincidan).
    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .by_table
            .values()
            .map(|e| e.table.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// `Url` de archivo a partir de una ruta del filesystem.
pub fn path_to_uri(path: &Path) -> Option<Url> {
    Url::from_file_path(path).ok()
}

/// Todos los archivos `.model` bajo `root` (recursivo, profundidad acotada).
pub fn model_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(root, &mut out, 0);
    out
}

/// Recolecta archivos `.model` bajo `root` recursivamente (profundidad acotada),
/// saltando directorios de build/control de versiones.
fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("target" | ".git" | "node_modules" | ".devcontainer")
            );
            if !skip {
                collect(&path, out, depth + 1);
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("model") {
            out.push(path);
        }
    }
}
