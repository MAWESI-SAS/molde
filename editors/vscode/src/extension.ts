// VS Code client for the molde language server (molde-lsp).
// Launches the `molde-lsp` binary over stdio and delegates diagnostics,
// navigation, dependencies, and formatting to it. Highlighting is provided by
// the TextMate grammar.

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
      `Could not start molde-lsp ('${command}'). Is it compiled and on the PATH? ${err}`
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
