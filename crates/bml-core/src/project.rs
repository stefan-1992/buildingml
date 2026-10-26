use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::nodes::SourceFile;
use crate::config::ProjectConfig;
use crate::parser::cst_to_ast;
use crate::parser::error::ParseError;
use crate::semantic;
use crate::semantic::diagnostics::Diagnostic;
use crate::semantic::model::SemanticModel;

/// A multi-file BuildingML project.
pub struct Project {
    pub config: ProjectConfig,
    pub root: PathBuf,
    pub files: HashMap<PathBuf, SourceFile>,
    pub file_sources: HashMap<PathBuf, String>,
}

/// Errors during project loading.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("failed to read file `{0}`: {1}")]
    Io(String, String),

    #[error("parse error in `{file}`: {errors:?}")]
    Parse {
        file: String,
        errors: Vec<ParseError>,
    },

    #[error("circular import detected: {0}")]
    CircularImport(String),

    #[error("config error: {0}")]
    Config(#[from] crate::config::ConfigError),
}

impl Project {
    /// Load a single .bml file as a project entry point, resolving its imports
    /// relative to its parent directory. Use this when linting a standalone file
    /// that may contain `import` directives.
    pub fn load_entry_file(file: &Path) -> Result<Self, ProjectError> {
        let canonical = file
            .canonicalize()
            .map_err(|e| ProjectError::Io(file.display().to_string(), e.to_string()))?;
        let root = canonical.parent().unwrap_or(Path::new(".")).to_path_buf();
        let filename = canonical
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| canonical.clone());

        let mut project = Project {
            config: ProjectConfig::default(),
            root,
            files: HashMap::new(),
            file_sources: HashMap::new(),
        };

        project.load_file(&filename)?;
        project.resolve_imports()?;
        Ok(project)
    }

    /// Load a project from a directory containing `buildingml.toml`.
    pub fn load(root: &Path) -> Result<Self, ProjectError> {
        let config_path = root.join("buildingml.toml");
        let config = if config_path.exists() {
            ProjectConfig::load(&config_path).map_err(ProjectError::Config)?
        } else {
            ProjectConfig::default()
        };

        let mut project = Project {
            config,
            root: root.to_path_buf(),
            files: HashMap::new(),
            file_sources: HashMap::new(),
        };

        // Auto-load asset libraries declared in buildingml.toml [assets] before
        // the entry file, so assets are available without explicit `import`.
        let asset_paths: Vec<String> = project.config.assets.clone();
        for asset_path in &asset_paths {
            let full = root.join(asset_path);
            if full.exists() {
                project.load_file(Path::new(asset_path))?;
            }
        }

        // Load entry point file (pass relative path to load_file, which joins with root)
        let entry = project.config.entry.clone();
        let entry_path = root.join(&entry);
        if entry_path.exists() {
            project.load_file(Path::new(&entry))?;
        } else {
            // Try loading all .bml files in the directory
            project.load_all_bml_files(root)?;
        }

        Ok(project)
    }

    /// Load a project from in-memory sources (for WASM).
    pub fn load_from_sources(sources: HashMap<String, String>) -> Result<Self, ProjectError> {
        let config = if let Some(toml_src) = sources.get("buildingml.toml") {
            ProjectConfig::parse(toml_src).map_err(ProjectError::Config)?
        } else {
            ProjectConfig::default()
        };

        let mut project = Project {
            config,
            root: PathBuf::from("."),
            files: HashMap::new(),
            file_sources: HashMap::new(),
        };

        for (name, source) in &sources {
            if name.ends_with(".bml") {
                let path = PathBuf::from(name);
                let ast = cst_to_ast::parse(source).map_err(|errors| ProjectError::Parse {
                    file: name.clone(),
                    errors,
                })?;
                project.file_sources.insert(path.clone(), source.clone());
                project.files.insert(path, ast);
            }
        }

        // Resolve imports transitively
        project.resolve_imports()?;

        Ok(project)
    }

    /// Load a single .bml file and its imports.
    fn load_file(&mut self, path: &Path) -> Result<(), ProjectError> {
        let canonical = normalize_path(&if path.is_absolute() || path.starts_with(&self.root) {
            path.to_path_buf()
        } else {
            self.root.join(path)
        });

        if self.files.contains_key(&canonical) {
            return Ok(()); // Already loaded
        }

        let source = std::fs::read_to_string(&canonical)
            .map_err(|e| ProjectError::Io(canonical.display().to_string(), e.to_string()))?;

        let ast = cst_to_ast::parse(&source).map_err(|errors| ProjectError::Parse {
            file: canonical.display().to_string(),
            errors,
        })?;

        self.file_sources.insert(canonical.clone(), source);

        // Collect imports before inserting
        let import_paths: Vec<PathBuf> = ast
            .items
            .iter()
            .filter_map(|item| {
                if let crate::ast::nodes::Item::Import(import) = item {
                    let import_path = canonical
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(&import.path);
                    Some(import_path)
                } else {
                    None
                }
            })
            .collect();

        self.files.insert(canonical, ast);

        // Recursively load imports
        for import_path in import_paths {
            self.load_file(&import_path)?;
        }

        Ok(())
    }

    /// Load all .bml files in a directory.
    fn load_all_bml_files(&mut self, dir: &Path) -> Result<(), ProjectError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| ProjectError::Io(dir.display().to_string(), e.to_string()))?;

        let mut bml_files: Vec<PathBuf> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("bml") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        bml_files.sort();

        for path in bml_files {
            self.load_file(&path)?;
        }

        Ok(())
    }

    /// Resolve imports (check for circular dependencies).
    fn resolve_imports(&self) -> Result<(), ProjectError> {
        // Simple circular dependency check using DFS with owned PathBufs
        let mut visited: HashMap<PathBuf, bool> = HashMap::new();

        for path in self.files.keys() {
            self.check_circular(path, &mut visited)?;
        }

        Ok(())
    }

    fn check_circular(
        &self,
        path: &Path,
        visited: &mut HashMap<PathBuf, bool>,
    ) -> Result<(), ProjectError> {
        if let Some(&in_stack) = visited.get(path) {
            if in_stack {
                return Err(ProjectError::CircularImport(path.display().to_string()));
            }
            return Ok(());
        }

        visited.insert(path.to_path_buf(), true);

        if let Some(ast) = self.files.get(path) {
            let import_paths: Vec<PathBuf> = ast
                .items
                .iter()
                .filter_map(|item| {
                    if let crate::ast::nodes::Item::Import(import) = item {
                        Some(normalize_path(
                            &path.parent().unwrap_or(Path::new(".")).join(&import.path),
                        ))
                    } else {
                        None
                    }
                })
                .collect();

            for import_path in &import_paths {
                self.check_circular(import_path, visited)?;
            }
        }

        visited.insert(path.to_path_buf(), false);
        Ok(())
    }

    /// Loaded file paths in a stable order. File `i` gets `Span::file_id`
    /// `i + 1`; 0 means unknown.
    pub fn file_paths(&self) -> Vec<&PathBuf> {
        let mut paths: Vec<&PathBuf> = self.files.keys().collect();
        paths.sort();
        paths
    }

    /// File names and sources in [`Project::file_paths`] order, so entry
    /// `file_id - 1` is the file of a `Span::file_id`.
    pub fn sources(&self) -> Vec<(String, &str)> {
        self.file_paths()
            .into_iter()
            .map(|p| (p.display().to_string(), self.file_sources[p].as_str()))
            .collect()
    }

    /// Run semantic analysis across all files, merging into a single model.
    pub fn analyze(&self) -> (SemanticModel, Vec<Diagnostic>) {
        semantic::analyze_project(self)
    }
}

/// True when the source contains at least one `import` directive.
pub fn has_imports(source: &str) -> bool {
    source
        .lines()
        .any(|l| l.trim_start().starts_with("import "))
}

/// Parse and analyze a BML source buffer, resolving imports when the buffer
/// contains `import` directives and a filesystem path is provided.
///
/// When a path is supplied and the source has imports, the file is loaded as
/// a project entry point so sibling files (asset libraries, materials, etc.)
/// are merged into the semantic model. The live `source` overrides whatever
/// is on disk for the entry file, so unsaved edits are reflected.
///
/// Falls back to single-file analysis when no path is provided, no imports
/// are present, or project loading fails (e.g. a referenced file is missing).
pub fn analyze_source(
    path: Option<&Path>,
    source: &str,
) -> Result<(SemanticModel, Vec<Diagnostic>), Vec<ParseError>> {
    let ast = cst_to_ast::parse(source)?;

    if has_imports(source)
        && let Some(p) = path
        && let Ok(mut project) = Project::load_entry_file(p)
        && let Ok(canonical) = p.canonicalize()
    {
        project
            .file_sources
            .insert(canonical.clone(), source.to_string());
        project.files.insert(canonical, ast);
        return Ok(project.analyze());
    }

    Ok(semantic::analyze(&ast))
}

/// Resolve `.` and `..` components without touching the file system, so a
/// file imported as `./m.bml` and as `storeys/../m.bml` is loaded once.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
