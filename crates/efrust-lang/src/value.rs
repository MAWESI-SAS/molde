//! Léxico de bajo nivel: tokenización que respeta comillas y delimitadores, y
//! parseo de valores inline (`[..]`, `{..}`, escalares, `~`=null).

use serde_json::Value;

use crate::error::{EfmError, Result};

/// Divide `s` por el separador `sep` a NIVEL SUPERIOR, ignorando separadores que
/// aparezcan dentro de comillas dobles o de `[] {} ()`.
pub fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            cur.push(c);
            if c == '\\' {
                if let Some(&n) = chars.peek() {
                    cur.push(n);
                    chars.next();
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            '[' | '{' | '(' => {
                depth += 1;
                cur.push(c);
            }
            ']' | '}' | ')' => {
                depth -= 1;
                cur.push(c);
            }
            _ if c == sep && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Tokeniza por espacios en blanco respetando comillas y `[] {} ()`.
pub fn tokenize_ws(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            cur.push(c);
            if c == '\\' {
                if let Some(&n) = chars.peek() {
                    cur.push(n);
                    chars.next();
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            '[' | '{' | '(' => {
                depth += 1;
                cur.push(c);
            }
            ']' | '}' | ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Quita las comillas dobles externas de `s` y deshace los escapes `\"` y `\\`.
pub fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        let inner = &t[1..t.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        t.to_string()
    }
}

/// Indica si un escalar necesita comillas al emitirse.
pub fn needs_quote(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s == "~" || s == "true" || s == "false" {
        return true;
    }
    s.chars().any(|c| {
        c.is_whitespace() || matches!(c, ':' | '#' | '{' | '}' | '[' | ']' | ',' | '"' | '|')
    })
}

/// Emite un escalar, citándolo si hace falta.
pub fn quote_scalar(s: &str) -> String {
    if needs_quote(s) {
        let esc = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{esc}\"")
    } else {
        s.to_string()
    }
}

/// Parsea un valor inline a `serde_json::Value`: listas, objetos, `~`, bool,
/// número o cadena.
pub fn parse_value(s: &str, line: usize) -> Result<Value> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(Value::String(String::new()));
    }
    if t == "~" {
        return Ok(Value::Null);
    }
    if t == "true" {
        return Ok(Value::Bool(true));
    }
    if t == "false" {
        return Ok(Value::Bool(false));
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        let mut arr = Vec::new();
        for part in split_top_level(inner, ',') {
            if part.is_empty() {
                continue;
            }
            arr.push(parse_value(&part, line)?);
        }
        return Ok(Value::Array(arr));
    }
    if let Some(inner) = t.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
        let mut map = serde_json::Map::new();
        for part in split_top_level(inner, ',') {
            if part.is_empty() {
                continue;
            }
            let (k, v) = part.split_once(':').ok_or_else(|| {
                EfmError::new(line, format!("se esperaba 'clave: valor' en '{part}'"))
            })?;
            map.insert(k.trim().to_string(), parse_value(v, line)?);
        }
        return Ok(Value::Object(map));
    }
    if t.starts_with('"') {
        return Ok(Value::String(unquote(t)));
    }
    if let Ok(i) = t.parse::<i64>() {
        return Ok(Value::Number(i.into()));
    }
    if let Ok(fl) = t.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(fl) {
            return Ok(Value::Number(n));
        }
    }
    Ok(Value::String(t.to_string()))
}

/// Emite un `serde_json::Value` como literal inline EFM.
pub fn emit_value(v: &Value) -> String {
    match v {
        Value::Null => "~".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => quote_scalar(s),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(emit_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, val)| format!("{k}: {}", emit_value(val)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}
