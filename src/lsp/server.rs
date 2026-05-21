/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use parking_lot::Mutex;
use rockr::compiler::Config;
use rockr::{Db, RockrDb};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct LspBackend {
    client: Client,
    db: Arc<Mutex<RockrDb>>,
}

impl LspBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            db: Arc::new(Mutex::new(RockrDb::new())),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LspBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), "::".into()]),
                    all_commit_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                    completion_item: None,
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                        SemanticTokensRegistrationOptions {
                            text_document_registration_options: TextDocumentRegistrationOptions {
                                document_selector: Some(vec![DocumentFilter {
                                    language: Some("l".to_string()),
                                    scheme: Some("file".to_string()),
                                    pattern: None,
                                }]),
                            },
                            semantic_tokens_options: SemanticTokensOptions {
                                work_done_progress_options: WorkDoneProgressOptions::default(),
                                legend: SemanticTokensLegend {
                                    token_types: vec![
                                        SemanticTokenType::FUNCTION,
                                        SemanticTokenType::VARIABLE,
                                        SemanticTokenType::PARAMETER,
                                        SemanticTokenType::STRUCT,
                                        SemanticTokenType::ENUM,
                                        SemanticTokenType::PROPERTY,
                                        SemanticTokenType::METHOD,
                                        SemanticTokenType::INTERFACE,
                                    ],
                                    token_modifiers: vec![],
                                },
                                range: Some(true),
                                full: Some(SemanticTokensFullOptions::Bool(true)),
                            },
                            static_registration_options: StaticRegistrationOptions::default(),
                        },
                    ),
                ),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "rock-lsp ready")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // let file = self.vfs.intern(&params.text_document.uri);
        // let snapshot = self.apply_change(file, params.text_document.text);
        // self.publish_diagnostics(file, snapshot).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // // FULL sync, so there is exactly one change with the whole text.
        // let Some(change) = params.content_changes.into_iter().next() else {
        //     return;
        // };
        // let file = self.vfs.intern(&params.text_document.uri);
        // let snapshot = self.apply_change(file, change.text);
        // self.publish_diagnostics(file, snapshot).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        todo!()
        // let uri = params.text_document_position_params.text_document.uri;
        // let pos = params.text_document_position_params.position;
        // let file = self.vfs.intern(&uri);
        // let snapshot = self.snapshot();

        // let result = tokio::task::spawn_blocking(move || {
        //     // Convert LSP Position → byte offset via a Salsa query on `snapshot`
        //     // let offset = snapshot.line_index(file).offset(pos);
        //     // let info = snapshot.hover_at(file, offset)?;
        //     // Some(Hover { contents: ..., range: ... })
        //     None::<Hover>
        // })
        // .await
        // .ok()
        // .flatten();

        // Ok(result)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

struct TextDocumentChange<'a> {
    uri: String,
    text: &'a str,
}

impl LspBackend {
    // pub fn snapshot(&self) -> salsa::Snapshot<RockrDb> {
    //     self.db.lock().snapshot()
    // }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| LspBackend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
