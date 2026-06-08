//! Workspace index: maps each table name to its `.model` file and its columns,
//! to resolve navigation (go-to-definition) and dependencies (find-references)
//! between entities.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::Url;

/// Information about an indexed entity.
#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub table: String,
    pub path: PathBuf,
    pub columns: Vec<String>,
}

/// Index of the entire models directory.
#[derive(Debug, Default)]
pub struct Index {
    /// Key: table name and, as an alias, the file stem.
    by_table: HashMap<String, EntityInfo>,
}

impl Index {
    /// Rebuilds the index by scanning `root` for `.model` files.
    pub fn build(root: &Path) -> Self {
        let mut idx = Index::default();
        for path in model_paths(root) {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if name == molde_lang::DATABASE_FILE {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(table) = molde_lang::parse_entity(&src) {
                let info = EntityInfo {
                    table: table.name.clone(),
                    path: path.clone(),
                    columns: table.columns.iter().map(|c| c.name.clone()).collect(),
                };
                // Index by table name and by file stem (alias).
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

    /// Known table names (without duplicating matching stem aliases).
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

/// File `Url` from a filesystem path.
pub fn path_to_uri(path: &Path) -> Option<Url> {
    Url::from_file_path(path).ok()
}

/// All `.model` files under `root` (recursive, bounded depth).
pub fn model_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(root, &mut out, 0);
    out
}

/// Collects `.model` files under `root` recursively (bounded depth), skipping
/// build/version-control directories.
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
