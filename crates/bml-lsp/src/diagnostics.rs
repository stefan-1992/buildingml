use bml_core::ast::span::Span;
use bml_core::parser::error::ParseError;
use bml_core::semantic::diagnostics::{Diagnostic, Severity};
use tower_lsp::lsp_types;

/// Convert a BML span to an LSP range (0-indexed lines/columns).
pub fn span_to_range(span: &Span) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position {
            line: span.start.line.saturating_sub(1) as u32,
            character: span.start.column.saturating_sub(1) as u32,
        },
        end: lsp_types::Position {
            line: span.end.line.saturating_sub(1) as u32,
            character: span.end.column.saturating_sub(1) as u32,
        },
    }
}

/// Convert parse errors to LSP diagnostics.
pub fn parse_errors_to_diagnostics(errors: &[ParseError]) -> Vec<lsp_types::Diagnostic> {
    errors
        .iter()
        .map(|err| {
            let (message, range) = match err {
                ParseError::SyntaxError { message, span } => (message.clone(), span_to_range(span)),
                ParseError::UnexpectedNode { kind, span } => {
                    (format!("unexpected node: {}", kind), span_to_range(span))
                }
                ParseError::TreeSitter(msg) => (
                    msg.clone(),
                    lsp_types::Range {
                        start: lsp_types::Position {
                            line: 0,
                            character: 0,
                        },
                        end: lsp_types::Position {
                            line: 0,
                            character: 0,
                        },
                    },
                ),
            };

            lsp_types::Diagnostic {
                range,
                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                code: Some(lsp_types::NumberOrString::String("E001".into())),
                source: Some("buildingml".into()),
                message,
                related_information: None,
                tags: None,
                code_description: None,
                data: None,
            }
        })
        .collect()
}

/// Convert semantic diagnostics to LSP diagnostics.
pub fn semantic_diagnostics_to_lsp(diagnostics: &[Diagnostic]) -> Vec<lsp_types::Diagnostic> {
    diagnostics
        .iter()
        .map(|d| {
            let severity = match d.severity {
                Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                Severity::Info => lsp_types::DiagnosticSeverity::INFORMATION,
            };

            lsp_types::Diagnostic {
                range: span_to_range(&d.span),
                severity: Some(severity),
                code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
                source: Some("buildingml".into()),
                message: if let Some(help) = &d.help {
                    format!("{}\nhint: {}", d.message, help)
                } else {
                    d.message.clone()
                },
                related_information: if d.related.is_empty() {
                    None
                } else {
                    Some(
                        d.related
                            .iter()
                            .map(|r| lsp_types::DiagnosticRelatedInformation {
                                location: lsp_types::Location {
                                    uri: lsp_types::Url::parse("file:///").unwrap(),
                                    range: span_to_range(&r.span),
                                },
                                message: r.message.clone(),
                            })
                            .collect(),
                    )
                },
                tags: None,
                code_description: None,
                data: None,
            }
        })
        .collect()
}
