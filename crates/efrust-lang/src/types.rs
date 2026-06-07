//! Mapeo entre los tipos lógicos del lenguaje y el `clr_type` del IR.

/// Tabla canónica `lógico ↔ clr_type`. El primer elemento es el nombre lógico.
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

/// `clr_type` correspondiente a un tipo lógico conocido.
pub fn logical_to_clr(logical: &str) -> Option<&'static str> {
    // `json` es azúcar de entrada: se canoniza a `string` + dbtype.
    if logical == "json" {
        return Some("System.String");
    }
    MAP.iter().find(|(l, _)| *l == logical).map(|(_, c)| *c)
}

/// Tipo lógico para un `clr_type` conocido (dirección de emisión).
pub fn clr_to_logical(clr: &str) -> Option<&'static str> {
    MAP.iter().find(|(_, c)| *c == clr).map(|(l, _)| *l)
}

/// ¿Es `s` un nombre de tipo lógico conocido (incluida el azúcar `json`)?
pub fn is_logical(s: &str) -> bool {
    s == "json" || MAP.iter().any(|(l, _)| *l == s)
}
