//! Canonicalization of Postgres expression text.
//!
//! `pg_get_constraintdef`/`pg_get_expr` deparse a check or index filter like
//! `… = ANY ((ARRAY[a, b])::text[])`, but once that text is re-applied the
//! parser distributes the cast over the array elements and it deparses as
//! `… = ANY (ARRAY[(a)::text, (b)::text])`. The two are identical; rewriting
//! the whole-array-cast form to the distributed form (what PostgreSQL stores)
//! makes reads from different databases converge, so a freshly applied check
//! or partial index does not read back as drift.
//!
//! > A twin of this function lives in `molde-sync` (a deliberately standalone
//! > crate); keep both in sync if the rewrite rules evolve.

/// Rewrites whole-array casts to the distributed form PostgreSQL stores.
/// Expressions without the pattern pass through unchanged.
pub fn normalize_pg_expression(def: &str) -> String {
    let mut s = def.to_string();
    let mut from = 0;
    while let Some(rel) = s[from..].find("(ARRAY[") {
        let start = from + rel;
        let lbracket = start + "(ARRAY".len(); // index of '['
        let Some(rbracket) = matching_bracket(&s, lbracket) else {
            from = start + 7;
            continue;
        };
        // Right after the inner `]` we need `)::<base>[]` for this to be a
        // whole-array cast; otherwise it is just an ARRAY[...] in some context.
        let tail = &s[rbracket + 1..];
        let Some(after) = tail.strip_prefix(")::") else {
            from = start + 7;
            continue;
        };
        let Some(arr_pos) = after.find("[]") else {
            from = start + 7;
            continue;
        };
        let base = after[..arr_pos].trim();
        if base.is_empty() {
            from = start + 7;
            continue;
        }
        let inner = &s[lbracket + 1..rbracket];
        let distributed: Vec<String> = split_top_commas(inner)
            .iter()
            .map(|e| format!("({})::{base}", e.trim()))
            .collect();
        let replacement = format!("ARRAY[{}]", distributed.join(", "));
        let end = rbracket + 1 + 3 + arr_pos + 2; // ")::" + base + "[]"
        s.replace_range(start..end, &replacement);
        from = start + replacement.len();
    }
    s
}

/// Index of the `]` matching the `[` at `open`, respecting nesting and single
/// quotes. Returns `None` if unbalanced.
fn matching_bracket(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        let c = b as char;
        if in_str {
            if c == '\'' {
                in_str = false;
            }
        } else {
            match c {
                '\'' => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Split on top-level commas, ignoring those inside parens/brackets or quotes.
fn split_top_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut cur = String::new();
    for c in s.chars() {
        if in_str {
            cur.push(c);
            if c == '\'' {
                in_str = false;
            }
            continue;
        }
        match c {
            '\'' => {
                in_str = true;
                cur.push(c);
            }
            '(' | '[' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize_pg_expression;

    #[test]
    fn whole_array_cast_is_distributed() {
        let factored = "CHECK (((status)::text = ANY ((ARRAY['a'::character varying, 'b'::character varying])::text[])))";
        let distributed = "CHECK (((status)::text = ANY (ARRAY[('a'::character varying)::text, ('b'::character varying)::text])))";
        assert_eq!(normalize_pg_expression(factored), distributed);
        // Idempotent: the distributed form passes through unchanged.
        assert_eq!(normalize_pg_expression(distributed), distributed);
    }

    #[test]
    fn expressions_without_the_pattern_pass_through() {
        let simple = "CHECK ((amount > 0))";
        assert_eq!(normalize_pg_expression(simple), simple);
        let filter = "(\"deletedAt\" IS NULL)";
        assert_eq!(normalize_pg_expression(filter), filter);
    }
}
