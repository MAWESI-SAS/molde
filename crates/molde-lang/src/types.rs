//! Mapping between the language's logical types and the IR's `clr_type`.

/// Canonical `logical ↔ clr_type` table. The first element is the logical name.
const MAP: &[(&str, &str)] = &[
    ("int", "System.Int32"),
    ("long", "System.Int64"),
    ("short", "System.Int16"),
    ("byte", "System.Byte"),
    ("bool", "System.Boolean"),
    ("string", "System.String"),
    ("decimal", "System.Decimal"),
    ("double", "System.Double"),
    ("float", "System.Single"),
    ("datetime", "System.DateTime"),
    ("datetimeoffset", "System.DateTimeOffset"),
    ("date", "System.DateOnly"),
    ("time", "System.TimeOnly"),
    ("guid", "System.Guid"),
    ("bytes", "System.Byte[]"),
];

/// `clr_type` corresponding to a known logical type.
pub fn logical_to_clr(logical: &str) -> Option<&'static str> {
    // `json` is input sugar: it is canonicalized to `string` + dbtype.
    if logical == "json" {
        return Some("System.String");
    }
    MAP.iter().find(|(l, _)| *l == logical).map(|(_, c)| *c)
}

/// Logical type for a known `clr_type` (emission direction).
pub fn clr_to_logical(clr: &str) -> Option<&'static str> {
    MAP.iter().find(|(_, c)| *c == clr).map(|(l, _)| *l)
}

/// Is `s` a known logical type name (including the `json` sugar)?
pub fn is_logical(s: &str) -> bool {
    s == "json" || MAP.iter().any(|(l, _)| *l == s)
}
