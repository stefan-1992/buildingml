use bml_core::ast::nodes::SourceFile;
use bml_core::parser::cst_to_ast;
use bml_core::parser::error::ParseError;
use bml_core::project::Project;
use bml_core::semantic::diagnostics::Diagnostic;
use bml_core::semantic::model::SemanticModel;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// State for a single open document.
#[allow(dead_code)]
pub struct DocumentState {
    pub text: String,
    pub ast: Option<SourceFile>,
    pub parse_errors: Vec<ParseError>,
    pub model: Option<SemanticModel>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentState {
    /// Parse and analyze the document text without import resolution.
    #[allow(dead_code)]
    pub fn from_text(text: String) -> Self {
        Self::from_text_with_path(None, text)
    }

    /// Parse and analyze the document text. When `path` is provided and the
    /// buffer contains `import` directives, imports are resolved relative to
    /// the file's parent directory and the live buffer overrides the entry
    /// file's on-disk content so unsaved edits are reflected in diagnostics.
    pub fn from_text_with_path(path: Option<&Path>, text: String) -> Self {
        let ast = match cst_to_ast::parse(&text) {
            Ok(ast) => ast,
            Err(errors) => {
                return Self {
                    text,
                    ast: None,
                    parse_errors: errors,
                    model: None,
                    diagnostics: Vec::new(),
                };
            }
        };

        let has_imports = text.lines().any(|l| l.trim_start().starts_with("import "));

        if has_imports
            && let Some(p) = path
            && let Some(state) = analyze_with_imports(p, &text, &ast)
        {
            return state;
        }

        let (model, diagnostics) = bml_core::semantic::analyze(&ast);
        Self {
            text,
            ast: Some(ast),
            parse_errors: Vec::new(),
            model: Some(model),
            diagnostics,
        }
    }
}

/// Analyze a buffer as a project entry point, overriding the entry file's
/// source with the live buffer text. Returns `None` if the project cannot be
/// loaded (e.g. missing imported files on disk); the caller falls back to
/// single-file analysis.
fn analyze_with_imports(path: &Path, text: &str, ast: &SourceFile) -> Option<DocumentState> {
    let mut project = Project::load_entry_file(path).ok()?;
    let canonical = path.canonicalize().ok()?;

    project
        .file_sources
        .insert(canonical.clone(), text.to_string());
    project.files.insert(canonical, ast.clone());

    let (model, diagnostics) = project.analyze();
    Some(DocumentState {
        text: text.to_string(),
        ast: Some(ast.clone()),
        parse_errors: Vec::new(),
        model: Some(model),
        diagnostics,
    })
}

/// Manages all open documents.
pub struct DocumentStore {
    pub documents: Mutex<HashMap<String, DocumentState>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: Mutex::new(HashMap::new()),
        }
    }
}
