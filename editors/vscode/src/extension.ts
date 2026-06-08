// Cliente de VS Code para el language server de EFM (efrust-lsp).
// Lanza el binario `efrust-lsp` por stdio y delega en él diagnostics,
// navegación, dependencias y formateo. El resaltado lo da la gramática TextMate.

import { workspace, ExtensionContext, window } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("efm");
  if (!config.get<boolean>("server.enabled", true)) {
    return;
  }

  const command = config.get<string>("server.path", "efrust-lsp");

  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "efm" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.model"),
    },
  };

  client = new LanguageClient(
    "efm",
    "EFM Language Server",
    serverOptions,
    clientOptions
  );

  client.start().catch((err) => {
    window.showErrorMessage(
      `No se pudo iniciar efrust-lsp ('${command}'). ¿Está compilado y en el PATH? ${err}`
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
