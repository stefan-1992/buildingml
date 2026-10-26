use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::completion;
use crate::diagnostics as diag;
use crate::document::{DocumentState, DocumentStore};
use crate::formatting;
use crate::goto_definition;
use crate::hover;
use crate::semantic_tokens;
use crate::symbols;

/// The BuildingML language server.
pub struct BmlLanguageServer {
    client: Client,
    store: Arc<DocumentStore>,
}

impl BmlLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            store: Arc::new(DocumentStore::new()),
        }
    }

    async fn publish_diagnostics(&self, uri: Url, doc: &DocumentState) {
        let mut lsp_diagnostics = Vec::new();

        // Parse errors
        lsp_diagnostics.extend(diag::parse_errors_to_diagnostics(&doc.parse_errors));

        // Semantic diagnostics
        lsp_diagnostics.extend(diag::semantic_diagnostics_to_lsp(&doc.diagnostics));

        self.client
            .publish_diagnostics(uri, lsp_diagnostics, None)
            .await;
    }

    fn update_document(&self, uri: &Url, text: String) -> DocumentState {
        let path = uri.to_file_path().ok();
        let doc = DocumentState::from_text_with_path(path.as_deref(), text);
        let uri_str = uri.to_string();
        self.store.documents.lock().unwrap().insert(
            uri_str,
            DocumentState::from_text_with_path(path.as_deref(), doc.text.clone()),
        );
        doc
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for BmlLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["#".into(), ":".into(), " ".into()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: semantic_tokens::TOKEN_TYPES.to_vec(),
                                token_modifiers: semantic_tokens::TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            ..Default::default()
                        },
                    ),
                ),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "BuildingML language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let doc = self.update_document(&uri, params.text_document.text);
        self.publish_diagnostics(uri, &doc).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            let doc = self.update_document(&uri, change.text);
            self.publish_diagnostics(uri, &doc).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri_str = params.text_document.uri.to_string();
        self.store.documents.lock().unwrap().remove(&uri_str);
        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri_str = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            let items = completion::provide_completions(doc, position);
            Ok(Some(CompletionResponse::Array(items)))
        } else {
            Ok(None)
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri_str = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            Ok(hover::provide_hover(doc, position))
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let uri_str = uri.to_string();
        let position = params.text_document_position_params.position;

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            if let Some(mut location) = goto_definition::goto_definition(doc, position) {
                location.uri = uri;
                Ok(Some(GotoDefinitionResponse::Scalar(location)))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let uri_str = uri.to_string();
        let position = params.text_document_position.position;

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            let mut locations = goto_definition::find_references(doc, position);
            for loc in &mut locations {
                loc.uri = uri.clone();
            }
            if locations.is_empty() {
                Ok(None)
            } else {
                Ok(Some(locations))
            }
        } else {
            Ok(None)
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri_str = params.text_document.uri.to_string();

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            if let Some(ast) = &doc.ast {
                let syms = symbols::document_symbols(ast);
                Ok(Some(DocumentSymbolResponse::Nested(syms)))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri_str = params.text_document.uri.to_string();

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            let edits = formatting::format_document(&doc.text);
            if edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(edits))
            }
        } else {
            Ok(None)
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri_str = params.text_document.uri.to_string();

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            if let Some(ast) = &doc.ast {
                let tokens = semantic_tokens::semantic_tokens(ast, &doc.text);
                Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: None,
                    data: tokens,
                })))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let uri_str = uri.to_string();

        let store = self.store.documents.lock().unwrap();
        if let Some(doc) = store.get(&uri_str) {
            let actions = code_actions(doc, &uri, &params);
            if actions.is_empty() {
                Ok(None)
            } else {
                Ok(Some(actions))
            }
        } else {
            Ok(None)
        }
    }
}

/// Generate code actions for the current diagnostics range.
fn code_actions(
    doc: &DocumentState,
    uri: &Url,
    params: &CodeActionParams,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diagnostic in &params.context.diagnostics {
        let code = diagnostic.code.as_ref().and_then(|c| match c {
            NumberOrString::String(s) => Some(s.as_str()),
            _ => None,
        });

        match code {
            Some("E101") => {
                // Missing required property — offer to add it
                if let Some(action) = missing_property_action(doc, uri, diagnostic) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            Some("E301") => {
                // Unresolved material reference — offer to create the material
                if let Some(action) = create_material_action(doc, uri, diagnostic) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            _ => {}
        }
    }

    actions
}

fn missing_property_action(
    _doc: &DocumentState,
    uri: &Url,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    // Extract property name from message: "Wall is missing required property `from`"
    let message = &diagnostic.message;
    let prop_name = message.split('`').nth(1)?;

    // Insert the property on the line after the diagnostic
    let insert_line = diagnostic.range.end.line + 1;
    let indent = "    "; // 4 spaces (element property level)

    let placeholder = match prop_name {
        "from" => format!("{}{}: (0, 0) to: (0, 0)\n", indent, prop_name),
        "height" | "thickness" | "offset" | "width" | "sill_height" | "ridge_height"
        | "riser_height" | "tread_depth" => {
            format!("{}{}: 0.0 m\n", indent, prop_name)
        }
        "boundary" => format!("{}{}: [(0,0), (0,0), (0,0), (0,0)]\n", indent, prop_name),
        "position" => format!("{}{}: (0.0, 0.0)\n", indent, prop_name),
        "profile" => format!("{}{}: rectangle(0.3, 0.3)\n", indent, prop_name),
        "type" => format!("{}{}: straight\n", indent, prop_name),
        "riser_count" => format!("{}{}: 18\n", indent, prop_name),
        _ => format!("{}{}: \n", indent, prop_name),
    };

    let mut changes = std::collections::HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: insert_line,
                    character: 0,
                },
                end: Position {
                    line: insert_line,
                    character: 0,
                },
            },
            new_text: placeholder,
        }],
    );

    Some(CodeAction {
        title: format!("Add missing property `{}`", prop_name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        is_preferred: Some(true),
        ..Default::default()
    })
}

fn create_material_action(
    _doc: &DocumentState,
    uri: &Url,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    // Extract material name from message
    let message = &diagnostic.message;
    let mat_name = message.split('`').nth(1)?;

    // Insert material definition at the top of the file
    let material_def = format!(
        "Material: {}\n  type: homogeneous\n  category: concrete\n\n",
        mat_name
    );

    let mut changes = std::collections::HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: material_def,
        }],
    );

    Some(CodeAction {
        title: format!("Create material `{}`", mat_name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        is_preferred: Some(false),
        ..Default::default()
    })
}

/// Run the language server on stdin/stdout.
pub async fn run() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = tower_lsp::LspService::new(BmlLanguageServer::new);

    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentState;

    #[test]
    fn test_from_file_resolves_imports() {
        let path =
            std::path::Path::new("/mnt/d/buildingml/tests/fixtures/common_furniture_test.bml");
        if !path.exists() {
            return; // skip in CI if fixture not present
        }
        let text = std::fs::read_to_string(path).unwrap();
        let doc = DocumentState::from_text_with_path(Some(path), text);
        let e301_count = doc
            .diagnostics
            .iter()
            .filter(|d| {
                matches!(
                    d.code,
                    bml_core::semantic::diagnostics::DiagnosticCode::E301
                )
            })
            .count();
        assert_eq!(
            e301_count, 0,
            "E301 errors should be gone after import resolution"
        );
    }

    #[test]
    fn test_document_state_valid() {
        let source = "# Site: Test\n  location: 49.0, 12.0\n";
        let doc = DocumentState::from_text(source.to_string());
        assert!(doc.ast.is_some());
        assert!(doc.parse_errors.is_empty());
        assert!(doc.model.is_some());
    }

    #[test]
    fn test_document_state_with_errors() {
        let source = "# Site: Test\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W1\n    height: 3 m\n";
        let doc = DocumentState::from_text(source.to_string());
        assert!(doc.ast.is_some());
        // Should have missing property diagnostics for wall (from, thickness)
        assert!(!doc.diagnostics.is_empty());
    }

    #[test]
    fn test_diagnostics_conversion() {
        let source = "# Site: Test\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W1\n    height: 3 m\n";
        let doc = DocumentState::from_text(source.to_string());
        let lsp_diags = diag::semantic_diagnostics_to_lsp(&doc.diagnostics);
        assert!(!lsp_diags.is_empty());
        // Check that all diagnostics have the source set
        for d in &lsp_diags {
            assert_eq!(d.source.as_deref(), Some("buildingml"));
        }
    }
}
