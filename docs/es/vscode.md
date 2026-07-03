# molde — Soporte de VS Code para archivos `.model`

Extensión de VS Code para el lenguaje de modelo **molde** de molde. Combina:

- **Resaltado de sintaxis** — gramática TextMate
  (`syntaxes/molde.tmLanguage.json`), sin dependencias.
- **Icono de archivo** — los `.model` tienen su propio icono de molde
  (`icons/molde.svg`) en el explorador y las pestañas.
- **Formato** — format-on-save mediante el language server (la forma
  canónica del emitter de molde). También disponible en la CLI con `molde fmt`.
- **Navegación y dependencias** — provistas por el language server `molde-lsp`:
  - Diagnósticos: errores de parseo en línea (con línea/columna).
  - Outline (símbolos del documento): estructura de cada entidad (Ctrl+Shift+O).
  - Go-to-definition: en `references: Table.col` salta a `Table.model`.
  - Find-references: en un nombre de tabla, lista todas las FKs que apuntan a
    ella (el grafo de dependencias).
  - Hover y autocompletado de nombres de tabla en `references:`.

## Requisitos

El binario del language server **`molde-lsp`** debe estar compilado y accesible:

```bash
# From the repo root
cargo build -p molde-lsp --release
# The binary ends up in target/release/molde-lsp
```

Ponlo en el `PATH`, o configura su ruta en la configuración de VS Code:

```json
// settings.json
"molde.server.path": "/path/to/repo/target/release/molde-lsp"
```

## Compilar e instalar la extensión

```bash
cd editors/vscode
npm install
npm run compile        # generates out/extension.js

# Option A — package and install:
npx @vscode/vsce package        # generates molde-language-<version>.vsix
code --install-extension molde-language-<version>.vsix

# Option B — development: open this folder in VS Code and press F5
# (Extension Development Host).
```

> **Mantén `molde-lsp` sincronizado con el lenguaje.** Cuando el lenguaje
> `.model` gane sintaxis (secciones o facetas nuevas), recompila y reemplaza el
> binario `molde-lsp` de tu `PATH` — un server viejo marcará la sintaxis nueva
> como errores de parseo aunque la CLI la acepte.

## Configuración

| Setting | Default | Description |
|---|---|---|
| `molde.server.path` | `molde-lsp` | Ruta al binario del language server. |
| `molde.server.enabled` | `true` | Habilita el language server. |

Con `editor.formatOnSave` habilitado, guardar un `.model` lo reformatea a la
forma canónica.

## Sin el language server

Si solo quieres el resaltado (sin navegación), configura
`molde.server.enabled` en `false`: la gramática TextMate funciona por sí
sola. Como alternativa mínima sin la extensión,
`"files.associations": { "*.model": "yaml" }` da un coloreado aproximado.
