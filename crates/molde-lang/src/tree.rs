//! Builds a generic tree from the indentation of the `.model` text.
//! Each node is `key: inline` (or a list item `- …`) with more deeply
//! indented children; block scalars `|` capture the subsequent raw text.

use crate::error::{MoldeError, Result};

#[derive(Debug, Clone)]
pub struct Node {
    pub line: usize,
    /// Key (`key` in `key: value`) or `-` for list items.
    pub key: String,
    /// Text after `key:` or after `- ` (may be `""`, `|`, `{..}`, a scalar, or a
    /// nested `k: v` in list items).
    pub inline: String,
    /// Text of the block scalar if `inline == "|"`.
    pub block: Option<String>,
    pub children: Vec<Node>,
}

impl Node {
    /// Finds the first child with the given key.
    pub fn child(&self, key: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.key == key)
    }

    /// Interprets the node as a `(key, value)` pair. For a list item
    /// (`- key: value`) it re-splits `inline`; otherwise it returns `(key, inline)`.
    pub fn as_kv(&self) -> (String, String) {
        if self.key == "-" {
            split_line(&self.inline)
        } else {
            (self.key.clone(), self.inline.clone())
        }
    }
}

struct Line {
    indent: usize,
    text: String,
    no: usize,
}

/// Significant lines (no blanks or comments), with their indentation.
fn scan(src: &str) -> Vec<Line> {
    src.lines()
        .enumerate()
        .filter_map(|(i, raw)| {
            let indent = raw.len() - raw.trim_start().len();
            let trimmed = raw.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                Some(Line {
                    indent,
                    text: trimmed.trim_end().to_string(),
                    no: i + 1,
                })
            }
        })
        .collect()
}

/// Parses the full text into a list of top-level nodes.
pub fn parse_tree(src: &str) -> Result<Vec<Node>> {
    let raw_all: Vec<&str> = src.lines().collect();
    let lines = scan(src);
    let mut idx = 0;
    let nodes = build(&lines, &mut idx, &raw_all)?;
    Ok(nodes)
}

fn build(lines: &[Line], idx: &mut usize, raw_all: &[&str]) -> Result<Vec<Node>> {
    let mut nodes = Vec::new();
    if *idx >= lines.len() {
        return Ok(nodes);
    }
    let level = lines[*idx].indent;
    while *idx < lines.len() {
        let cur = &lines[*idx];
        if cur.indent < level {
            break;
        }
        if cur.indent > level {
            return Err(MoldeError::new(cur.no, "unexpected indentation"));
        }
        let (key, inline) = split_line(&cur.text);
        let line_no = cur.no;
        let cur_indent = cur.indent;
        *idx += 1;

        let mut node = Node {
            line: line_no,
            key,
            inline,
            block: None,
            children: Vec::new(),
        };

        // Is the effective value a block scalar `|`? For list items we must
        // look at the value after re-splitting (e.g. `- normalize_body: |`).
        let is_block = if node.key == "-" {
            node.inline == "|" || split_line(&node.inline).1 == "|"
        } else {
            node.inline == "|"
        };

        if is_block {
            node.block = Some(capture_block(lines, idx, cur_indent, raw_all));
        } else if *idx < lines.len() && lines[*idx].indent > cur_indent {
            // Children: the following lines indented more than the current one.
            node.children = build(lines, idx, raw_all)?;
        }
        nodes.push(node);
    }
    Ok(nodes)
}

/// Captures the raw text of a block scalar: lines indented more than
/// `key_indent`, trimming the common indent (that of the block's first line).
fn capture_block(lines: &[Line], idx: &mut usize, key_indent: usize, raw_all: &[&str]) -> String {
    let mut block_lines: Vec<String> = Vec::new();
    let mut base: Option<usize> = None;
    while *idx < lines.len() {
        let l = &lines[*idx];
        if l.indent <= key_indent {
            break;
        }
        // We take the original raw text to preserve the content as-is.
        let raw = raw_all.get(l.no - 1).copied().unwrap_or(&l.text);
        let this_indent = raw.len() - raw.trim_start().len();
        let b = *base.get_or_insert(this_indent);
        let dedented = if this_indent >= b {
            raw[b..].to_string()
        } else {
            raw.trim_start().to_string()
        };
        block_lines.push(dedented);
        *idx += 1;
    }
    block_lines.join("\n")
}

/// Splits a line into `(key, inline)`. Supports `key: value`, `key:` (no value)
/// and list items `- …`.
fn split_line(text: &str) -> (String, String) {
    if let Some(rest) = text.strip_prefix("- ") {
        return ("-".to_string(), rest.trim().to_string());
    }
    if text == "-" {
        return ("-".to_string(), String::new());
    }
    // Find the first ':' at the top level (outside quotes/brackets).
    let mut depth = 0i32;
    let mut in_str = false;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => in_str = !in_str,
            '[' | '{' | '(' if !in_str => depth += 1,
            ']' | '}' | ')' if !in_str => depth -= 1,
            ':' if !in_str && depth == 0 => {
                let key: String = chars[..i].iter().collect();
                let inline: String = chars[i + 1..].iter().collect();
                return (key.trim().to_string(), inline.trim().to_string());
            }
            _ => {}
        }
    }
    (text.trim().to_string(), String::new())
}
