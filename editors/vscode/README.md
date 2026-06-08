# molde — VS Code support for `.model` files

VS Code extension for molde's **molde** model language. It combines:

- **Syntax highlighting** — TextMate grammar (`syntaxes/molde.tmLanguage.json`),
  no dependencies.
- **Formatting** — format-on-save via the language server (molde emitter's
  canonical form). Also available on the CLI with `molde fmt`.
- **Navigation and dependencies** — provided by the `molde-lsp` language server:
  - Diagnostics: inline parse errors (with line/column).
  - Outline (document symbols): structure of each entity (Ctrl+Shift+O).
  - Go-to-definition: on `references: Table.col` jumps to `Table.model`.
  - Find-references: on a table name, lists all the FKs that point to it
    (the dependency graph).
  - Hover and autocompletion of table names in `references:`.

## Requirements

The **`molde-lsp`** language server binary must be compiled and accessible:

```bash
# From the repo root
cargo build -p molde-lsp --release
# The binary ends up in target/release/molde-lsp
```

Put it on the `PATH`, or set its path in the VS Code settings:

```json
// settings.json
"molde.server.path": "/path/to/repo/target/release/molde-lsp"
```

## Build and install the extension

```bash
cd editors/vscode
npm install
npm run compile        # generates out/extension.js

# Option A — package and install:
npx @vscode/vsce package        # generates molde-language-0.0.1.vsix
code --install-extension molde-language-0.0.1.vsix

# Option B — development: open this folder in VS Code and press F5
# (Extension Development Host).
```

## Settings

| Setting | Default | Description |
|---|---|---|
| `molde.server.path` | `molde-lsp` | Path to the language server binary. |
| `molde.server.enabled` | `true` | Enables the language server. |

With `editor.formatOnSave` enabled, saving a `.model` reformats it to the
canonical form.

## Without the language server

If you only want highlighting (no navigation), set `molde.server.enabled` to
`false`: the TextMate grammar works on its own. As a minimal alternative without
the extension, `"files.associations": { "*.model": "yaml" }` gives approximate
coloring.
