use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Source location span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub start: Position,
    pub end: Position,
    /// File identifier for multi-file projects. 0 = single-file / unknown.
    /// Maps to a file path via `FileRegistry`.
    #[serde(skip_serializing_if = "is_zero")]
    pub file_id: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// A position in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub byte_offset: usize,
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self {
            start,
            end,
            file_id: 0,
        }
    }

    pub fn with_file_id(mut self, file_id: u32) -> Self {
        self.file_id = file_id;
        self
    }

    pub fn from_ts_node(node: &tree_sitter::Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            start: Position {
                line: start.row + 1,
                column: start.column + 1,
                byte_offset: node.start_byte(),
            },
            end: Position {
                line: end.row + 1,
                column: end.column + 1,
                byte_offset: node.end_byte(),
            },
            file_id: 0,
        }
    }

    pub fn from_ts_node_with_file(node: &tree_sitter::Node, file_id: u32) -> Self {
        Self::from_ts_node(node).with_file_id(file_id)
    }
}

/// Registry mapping file IDs to file paths for multi-file projects.
#[derive(Debug, Clone, Default)]
pub struct FileRegistry {
    paths: HashMap<u32, PathBuf>,
    next_id: u32,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
            next_id: 1,
        }
    }

    /// Register a file path and return its ID.
    pub fn register(&mut self, path: impl Into<PathBuf>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.paths.insert(id, path.into());
        id
    }

    /// Look up the file path for a given ID.
    pub fn get_path(&self, file_id: u32) -> Option<&Path> {
        self.paths.get(&file_id).map(|p| p.as_path())
    }

    /// Get the display name for a file ID (filename only, or full path if no filename).
    pub fn display_name(&self, file_id: u32) -> Option<String> {
        self.get_path(file_id).map(|p| {
            p.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        })
    }
}

/// Thread-safe global file registry for use during compilation.
static GLOBAL_REGISTRY: Mutex<Option<FileRegistry>> = Mutex::new(None);

/// Initialize the global file registry. Call before multi-file parsing.
pub fn init_global_registry() -> FileRegistry {
    FileRegistry::new()
}

/// Get a file path from the global registry by ID.
pub fn global_file_path(file_id: u32) -> Option<PathBuf> {
    GLOBAL_REGISTRY.lock().ok().and_then(|r| {
        r.as_ref()
            .and_then(|reg| reg.get_path(file_id).map(|p| p.to_owned()))
    })
}
