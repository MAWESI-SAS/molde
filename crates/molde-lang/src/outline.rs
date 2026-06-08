//! Structural outline of a `.model` file with line numbers.
//! Consumed by the language server for *document symbols* (outline view).
//! Reuses the indentation tree from `tree.rs` (which already carries each node's
//! line); it does not produce IR, only the shape of the file as-is.

use crate::error::Result;
use crate::tree::{parse_tree, Node};

/// An outline node: label, inline detail and line (1-based), with children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub label: String,
    pub detail: String,
    /// 1-based line where it appears.
    pub line: usize,
    pub children: Vec<OutlineItem>,
}

/// Builds the outline of a `.model` file.
pub fn outline(src: &str) -> Result<Vec<OutlineItem>> {
    let nodes = parse_tree(src)?;
    Ok(nodes.iter().map(to_item).collect())
}

fn to_item(node: &Node) -> OutlineItem {
    // For list items (`- key: value`) we use the re-split key as the label;
    // for the rest, the key itself.
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
    fn outline_of_entity_with_sections() {
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
        // Sections fields + indexes.
        let labels: Vec<&str> = table.children.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["fields", "indexes"]);
        // Fields.
        let fields = &table.children[0];
        assert_eq!(fields.children[0].label, "Id");
        assert_eq!(fields.children[0].line, 3);
        // Index list item: label = index name.
        let indexes = &table.children[1];
        assert_eq!(indexes.children[0].label, "ix_email");
        assert_eq!(indexes.children[0].line, 6);
    }
}
