// Cliente de VS Code para el language server de molde (molde-lsp).
// Lanza el binario `molde-lsp` por stdio y delega en él diagnostics,
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
  const config = workspace.getConfiguration("molde");
  if (!config.get<boolean>("server.enabled", true)) {
    return;
  }

  const command = config.get<string>("server.path", "molde-lsp");

  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "molde" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.model"),
    },
  };

  client = new LanguageClient(
    "molde",
    "molde Language Server",
    serverOptions,
    clientOptions
  );

  client.start().catch((err) => {
    window.showErrorMessage(
      `No se pudo iniciar molde-lsp ('${command}'). ¿Está compilado y en el PATH? ${err}`
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
