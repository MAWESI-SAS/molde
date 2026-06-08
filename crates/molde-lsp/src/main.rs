//! molde-lsp — language server for the molde model language (`.model`).
//!
//! Reuses `molde-lang` to parse/format/inspect. Provides:
//! diagnostics (inline parse errors), document symbols (outline),
//! go-to-definition (FK `references:` → entity), find-references (dependencies),
//! hover, table-name completion, and canonical formatting.

use std::path::PathBuf;
use std::sync::RwLock;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod index;
mod text;

use index::Index;

struct Backend {
    client: Client,
    /// Open documents (FULL sync): uri → content.
    docs: DashMap<Url, String>,
    /// Workspace root.
    root: RwLock<Option<PathBuf>>,
    /// Workspace entity index.
    index: RwLock<Index>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            docs: DashMap::new(),
            root: RwLock::new(None),
            index: RwLock::new(Index::default()),
        }
    }

    fn rebuild_index(&self) {
        let root = self.root.read().unwrap().clone();
        if let Some(root) = root {
            let idx = Index::build(&root);
            *self.index.write().unwrap() = idx;
        }
    }

    /// Re-parses a document and publishes diagnostics (empty if it parses fine).
    async fn validate(&self, uri: Url) {
        let Some(text) = self.docs.get(&uri).map(|t| t.clone()) else {
            return;
        };
        let name = text::uri_file_name(&uri);
        let diags = match molde_lang::format_model(&name, &text) {
            Ok(_) => Vec::new(),
            Err(e) => vec![text::diagnostic_from(&e, &text)],
        };
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Root: root_uri (deprecated) or the first workspace folder.
        let root = params
            .root_uri
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|f| f.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            });
        *self.root.write().unwrap() = root;

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "molde-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![" ".into(), ".".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.rebuild_index();
        let n = self.index.read().unwrap().table_names().len();
        self.client
            .log_message(
                MessageType::INFO,
                format!("molde-lsp: {n} entities indexed"),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.docs.insert(doc.uri.clone(), doc.text);
        self.validate(doc.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the last change carries the complete document.
        if let Some(change) = params.content_changes.into_iter().last() {
            self.docs
                .insert(params.text_document.uri.clone(), change.text);
        }
        self.validate(params.text_document.uri).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // On save, reindex the workspace (tables/columns may have changed).
        self.rebuild_index();
        self.validate(params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let Ok(items) = molde_lang::outline(&content) else {
            return Ok(None);
        };
        let lines: Vec<&str> = content.lines().collect();
        let symbols = items
            .iter()
            .map(|it| text::to_symbol(it, &lines, 0))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let line_text = text::nth_line(&content, pos.line as usize);
        // We only navigate on FK lines (`... references: Table.col ...`).
        let Some(target) = text::references_target(line_text) else {
            return Ok(None);
        };
        let idx = self.index.read().unwrap();
        let Some(info) = idx.get(&target) else {
            return Ok(None);
        };
        let Some(target_uri) = index::path_to_uri(&info.path) else {
            return Ok(None);
        };
        // We jump to the entity header (line 0).
        let range = Range::new(Position::new(0, 0), Position::new(0, 0));
        Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
            target_uri, range,
        ))))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let pos = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let line_text = text::nth_line(&content, pos.line as usize);
        let char_idx = text::utf16_to_char_idx(line_text, pos.character as usize);
        let Some(target) = text::word_at(line_text, char_idx) else {
            return Ok(None);
        };

        // Walk the entire workspace looking for FKs that point to `target`.
        let root = self.root.read().unwrap().clone();
        let Some(root) = root else {
            return Ok(None);
        };
        let mut locations = Vec::new();
        for path in index::model_paths(&root) {
            let Ok(file_text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(file_uri) = index::path_to_uri(&path) else {
                continue;
            };
            for (i, line) in file_text.lines().enumerate() {
                if text::references_target(line).as_deref() == Some(target.as_str()) {
                    let end = text::char_to_utf16(line, line.chars().count());
                    let range =
                        Range::new(Position::new(i as u32, 0), Position::new(i as u32, end));
                    locations.push(Location::new(file_uri.clone(), range));
                }
            }
        }
        Ok(Some(locations))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let line_text = text::nth_line(&content, pos.line as usize);
        let char_idx = text::utf16_to_char_idx(line_text, pos.character as usize);
        let Some(word) = text::word_at(line_text, char_idx) else {
            return Ok(None);
        };
        let idx = self.index.read().unwrap();
        let markdown = if let Some(info) = idx.get(&word) {
            format!(
                "**entity** `{}` — {} column(s)\n\n_{}_",
                info.table,
                info.columns.len(),
                info.path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
            )
        } else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let pos = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let line_text = text::nth_line(&content, pos.line as usize);
        // Complete table names inside a `references:`.
        if !line_text.contains("references:") {
            return Ok(None);
        }
        let idx = self.index.read().unwrap();
        let items = idx
            .table_names()
            .into_iter()
            .map(|name| CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::CLASS),
                ..Default::default()
            })
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(content) = self.docs.get(&uri).map(|t| t.clone()) else {
            return Ok(None);
        };
        let name = text::uri_file_name(&uri);
        let Ok(formatted) = molde_lang::format_model(&name, &content) else {
            return Ok(None); // document with a parse error: do not format
        };
        if formatted == content {
            return Ok(None);
        }
        Ok(Some(vec![TextEdit {
            range: text::whole_range(&content),
            new_text: formatted,
        }]))
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
