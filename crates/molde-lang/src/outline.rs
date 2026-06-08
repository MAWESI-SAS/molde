//! Esquema estructural (outline) de un archivo `.model` con números de línea.
//! Lo consume el language server para los *document symbols* (vista de esquema).
//! Reutiliza el árbol por indentación de `tree.rs` (que ya lleva la línea de cada
//! nodo); no produce IR, solo la forma del archivo tal cual.

use crate::error::Result;
use crate::tree::{parse_tree, Node};

/// Un nodo del outline: etiqueta, detalle inline y línea (1-based), con hijos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub label: String,
    pub detail: String,
    /// Línea 1-based donde aparece.
    pub line: usize,
    pub children: Vec<OutlineItem>,
}

/// Construye el outline de un archivo `.model`.
pub fn outline(src: &str) -> Result<Vec<OutlineItem>> {
    let nodes = parse_tree(src)?;
    Ok(nodes.iter().map(to_item).collect())
}

fn to_item(node: &Node) -> OutlineItem {
    // Para ítems de lista (`- clave: valor`) usamos la clave re-dividida como
    // etiqueta; para el resto, la propia clave.
    let (label, detail) = node.as_kv();
    OutlineItem {
        label,
        detail,
        line: node.line,
        children: node.children.iter().map(to_item).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_de_entidad_con_secciones() {
        let src = "\
Customer
  fields:
    Id: int pk
    Email: string?
  indexes:
    - ix_email: {on: [Email]}
";
        let items = outline(src).unwrap();
        assert_eq!(items.len(), 1);
        let table = &items[0];
        assert_eq!(table.label, "Customer");
        assert_eq!(table.line, 1);
        // Secciones fields + indexes.
        let labels: Vec<&str> = table.children.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["fields", "indexes"]);
        // Campos.
        let fields = &table.children[0];
        assert_eq!(fields.children[0].label, "Id");
        assert_eq!(fields.children[0].line, 3);
        // Ítem de lista de índice: etiqueta = nombre del índice.
        let indexes = &table.children[1];
        assert_eq!(indexes.children[0].label, "ix_email");
        assert_eq!(indexes.children[0].line, 6);
    }
}
