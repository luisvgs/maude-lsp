mod grammar;
use grammar::{KEYWORDS, TYPES};
use std::collections::HashMap;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    document: Mutex<HashMap<Url, String>>,
}

fn check_document(text: &str, uri: Url) -> Vec<Diagnostic> {
    let mut diagnostics = vec![];
    let skip = ["endfm", "endm", "endom", "is", ""];

    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if skip
            .iter()
            .any(|s| trimmed == *s || trimmed.starts_with(*s))
        {
            continue;
        }
        if !trimmed.ends_with('.') {
            let len = line.len() as u32;
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: i as u32,
                        character: 0,
                    },
                    end: Position {
                        line: i as u32,
                        character: len,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                message: "Missing '.' at end of declaration".to_string(),
                source: Some("maude-lsp".to_string()),
                ..Default::default()
            });
        }
    }
    diagnostics
}

fn hover_for_word(word: &str) -> Option<String> {
    match word {
        "Nat" | "NAT" => Some("Built-in sort for natural numbers.".to_string()),
        "Bool" | "BOOL" => Some("Built-in sort for booleans. Values: true, false.".to_string()),
        "Int" | "INT" => Some("Built-in sort for integers.".to_string()),
        "String" | "STRING" => Some("Built-in sort for string.".to_string()),
        "Float" | "FLOAT" => Some("Built-in sort for floating point.".to_string()),
        "op" => Some("Operator declaration.".to_string()),
        "eq" => Some("Equation declaration.".to_string()),
        "sort" | "sorts" => Some("Sort declaration.".to_string()),
        "var" | "vars" => Some("Variable declaration.".to_string()),
        "fmod" => Some("Functional module declaration.".to_string()),
        "mod" => Some("System module declaration.".to_string()),
        _ => None,
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                completion_provider: Some(CompletionOptions::default()),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Maude server initialized.")
            .await;
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        let items = KEYWORDS
            .iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .chain(TYPES.iter().map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                ..Default::default()
            }))
            .collect();

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }


    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let docs = self.document.lock().await;

        let Some(text) = docs.get(&uri) else { return Ok(None) };
        let Some(line) = text.lines().nth(pos.line as usize) else { return Ok(None) };

        let col = pos.character as usize;
        let start = line[..col].rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = line[col..].find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + col)
            .unwrap_or(line.len());

        let word = &line[start..end];

        Ok(hover_for_word(word).map(|desc| Hover {
            contents: HoverContents::Scalar(MarkedString::String(desc)),
            range: None,
        }))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
        .log_message(MessageType::WARNING, "did_open called!")
        .await;
        let uri = &params.text_document.uri;
        let text = &params.text_document.text.clone();
        let diagnostics =
            check_document(&params.text_document.text, uri.clone());
        self.client.log_message(MessageType::INFO, format!("diagnostics: {:?}", diagnostics)).await;
        self.document.lock().await.insert(uri.clone(), text.clone());
        
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;

    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let text = &params.content_changes[0].text;
        let uri = &params.text_document.uri;
        let diagnostics = check_document(text, uri.clone());
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;

        self.document.lock().await.insert(uri.clone(), text.to_string());
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.document.lock().await.remove(&params.text_document.uri);
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        document: Mutex::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
