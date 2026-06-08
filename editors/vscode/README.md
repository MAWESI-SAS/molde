# EFM — soporte de VS Code para archivos `.model`

Extensión de VS Code para el lenguaje de modelos **EFM** de efrust. Combina:

- **Resaltado de sintaxis** — gramática TextMate (`syntaxes/efm.tmLanguage.json`),
  sin dependencias.
- **Formateo** — format-on-save vía el language server (forma canónica del
  emitter de efrust). También disponible en CLI con `efrust fmt`.
- **Navegación y dependencias** — provisto por el language server `efrust-lsp`:
  - Diagnostics: errores de parseo inline (con línea/columna).
  - Outline (document symbols): estructura de cada entidad (Ctrl+Shift+O).
  - Go-to-definition: en `references: Tabla.col` salta a `Tabla.model`.
  - Find-references: sobre un nombre de tabla, lista todas las FKs que la
    apuntan (el grafo de dependencias).
  - Hover y autocompletado de nombres de tabla en `references:`.

## Requisitos

El binario del language server **`efrust-lsp`** debe estar compilado y accesible:

```bash
# Desde la raíz del repo
cargo build -p efrust-lsp --release
# El binario queda en target/release/efrust-lsp
```

Ponlo en el `PATH`, o indica su ruta en los ajustes de VS Code:

```json
// settings.json
"efm.server.path": "/ruta/al/repo/target/release/efrust-lsp"
```

## Compilar e instalar la extensión

```bash
cd editors/vscode
npm install
npm run compile        # genera out/extension.js

# Opción A — empaquetar e instalar:
npx @vscode/vsce package        # genera efm-language-0.0.1.vsix
code --install-extension efm-language-0.0.1.vsix

# Opción B — desarrollo: abre esta carpeta en VS Code y pulsa F5
# (Extension Development Host).
```

## Ajustes

| Ajuste | Por defecto | Descripción |
|---|---|---|
| `efm.server.path` | `efrust-lsp` | Ruta al binario del language server. |
| `efm.server.enabled` | `true` | Activa el language server. |

Con `editor.formatOnSave` activo, al guardar un `.model` se reformatea a la forma
canónica.

## Sin el language server

Si solo quieres resaltado (sin navegación), pon `efm.server.enabled` en `false`:
la gramática TextMate funciona sola. Como alternativa mínima sin extensión,
`"files.associations": { "*.model": "yaml" }` da coloreado aproximado.
