//! Construye un árbol genérico a partir de la indentación del texto `.model`.
//! Cada nodo es `key: inline` (o un ítem de lista `- …`) con hijos más
//! indentados; los block scalars `|` capturan texto crudo subsiguiente.

use crate::error::{MoldeError, Result};

#[derive(Debug, Clone)]
pub struct Node {
    pub line: usize,
    /// Clave (`key` en `key: valor`) o `-` para ítems de lista.
    pub key: String,
    /// Texto tras `key:` o tras `- ` (puede ser `""`, `|`, `{..}`, escalar, o un
    /// `k: v` anidado en ítems de lista).
    pub inline: String,
    /// Texto del block scalar si `inline == "|"`.
    pub block: Option<String>,
    pub children: Vec<Node>,
}

impl Node {
    /// Busca el primer hijo con la clave dada.
    pub fn child(&self, key: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.key == key)
    }

    /// Interpreta el nodo como par `(clave, valor)`. Para un ítem de lista
    /// (`- clave: valor`) re-divide `inline`; en otro caso devuelve `(key, inline)`.
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

/// Líneas significativas (sin blancos ni comentarios), con su indentación.
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

/// Parsea el texto completo a una lista de nodos de nivel superior.
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
            return Err(MoldeError::new(cur.no, "indentación inesperada"));
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

        // ¿El valor efectivo es un block scalar `|`? Para ítems de lista hay que
        // mirar el valor tras re-dividir (p. ej. `- normalize_body: |`).
        let is_block = if node.key == "-" {
            node.inline == "|" || split_line(&node.inline).1 == "|"
        } else {
            node.inline == "|"
        };

        if is_block {
            node.block = Some(capture_block(lines, idx, cur_indent, raw_all));
        } else if *idx < lines.len() && lines[*idx].indent > cur_indent {
            // Hijos: las siguientes líneas más indentadas que la actual.
            node.children = build(lines, idx, raw_all)?;
        }
        nodes.push(node);
    }
    Ok(nodes)
}

/// Captura el texto crudo de un block scalar: líneas con indentación mayor que
/// `key_indent`, recortando el sangrado común (el del primer renglón del bloque).
fn capture_block(lines: &[Line], idx: &mut usize, key_indent: usize, raw_all: &[&str]) -> String {
    let mut block_lines: Vec<String> = Vec::new();
    let mut base: Option<usize> = None;
    while *idx < lines.len() {
        let l = &lines[*idx];
        if l.indent <= key_indent {
            break;
        }
        // Tomamos el texto crudo original para preservar el contenido tal cual.
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

/// Separa una línea en `(key, inline)`. Soporta `key: valor`, `key:` (sin valor)
/// e ítems de lista `- …`.
fn split_line(text: &str) -> (String, String) {
    if let Some(rest) = text.strip_prefix("- ") {
        return ("-".to_string(), rest.trim().to_string());
    }
    if text == "-" {
        return ("-".to_string(), String::new());
    }
    // Buscar el primer ':' a nivel superior (fuera de comillas/corchetes).
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
