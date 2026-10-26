use crate::ast::span::Span;

/// Errors that can occur during parsing.
#[derive(Debug, Clone, thiserror::Error, serde::Serialize)]
pub enum ParseError {
    #[error("syntax error at {span:?}: {message}")]
    SyntaxError { message: String, span: Span },

    #[error("unexpected node type '{kind}' at {span:?}")]
    UnexpectedNode { kind: String, span: Span },

    #[error("tree-sitter error: {0}")]
    TreeSitter(String),
}
