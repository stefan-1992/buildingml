use std::path::{Path, PathBuf};

use bml_core::parser::cst_to_ast;
use bml_core::project::Project;
use bml_core::semantic;
use bml_core::semantic::diagnostics::Severity;

use crate::output;

pub fn run(input: PathBuf) -> miette::Result<()> {
    if input.is_dir() {
        return run_project(input);
    }

    let source = std::fs::read_to_string(&input)
        .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;

    // If the file contains import directives, load it as a project so imports are resolved.
    let has_imports = source
        .lines()
        .any(|l| l.trim_start().starts_with("import "));
    if has_imports {
        return run_project_file(input);
    }

    let filename = input.display().to_string();

    // Parse
    let ast = match cst_to_ast::parse(&source) {
        Ok(ast) => ast,
        Err(errors) => {
            for err in &errors {
                eprintln!("\x1b[31merror\x1b[0m: {}", err);
            }
            return Err(miette::miette!(
                "{} syntax error(s) in {}",
                errors.len(),
                filename
            ));
        }
    };

    // Semantic analysis
    let (_, diagnostics) = semantic::analyze(&ast);

    let error_count = output::print_diagnostics(&diagnostics, &source, &filename);
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    output::print_summary(error_count, warning_count);

    if error_count > 0 {
        std::process::exit(1);
    }

    if diagnostics.is_empty() {
        eprintln!("\x1b[32mAll checks passed\x1b[0m for {}", filename);
    }

    Ok(())
}

/// Lint a single .bml file that has imports by loading it as a project entry point.
fn run_project_file(input: PathBuf) -> miette::Result<()> {
    let project = Project::load_entry_file(&input)
        .map_err(|e| miette::miette!("failed to load {}: {}", input.display(), e))?;
    lint_project(&project, &input)
}

fn run_project(input: PathBuf) -> miette::Result<()> {
    let project =
        Project::load(&input).map_err(|e| miette::miette!("failed to load project: {}", e))?;
    lint_project(&project, &input)
}

fn lint_project(project: &Project, input: &Path) -> miette::Result<()> {
    let (_, diagnostics) = project.analyze();

    let error_count = output::print_diagnostics_multi_file(&diagnostics, &project.sources());
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    output::print_summary(error_count, warning_count);

    if error_count > 0 {
        std::process::exit(1);
    }

    if diagnostics.is_empty() {
        eprintln!(
            "\x1b[32mAll checks passed\x1b[0m for {} ({} files)",
            input.display(),
            project.files.len()
        );
    }

    Ok(())
}
