use std::path::PathBuf;

use bml_core::codegen;
use bml_core::parser::cst_to_ast;
use bml_core::project::Project;
use bml_core::semantic;
use bml_core::semantic::diagnostics::Severity;

use crate::output;

pub fn run(input: PathBuf, output_path: Option<PathBuf>) -> miette::Result<()> {
    // If input is a directory, use multi-file project mode
    if input.is_dir() {
        return run_project(input, output_path);
    }

    let source = std::fs::read_to_string(&input)
        .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;

    // If the file contains import directives, load it as a project so imports are resolved.
    let has_imports = source
        .lines()
        .any(|l| l.trim_start().starts_with("import "));
    if has_imports {
        return run_project_file(input, output_path);
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
                "failed to parse {}: {} syntax error(s)",
                filename,
                errors.len()
            ));
        }
    };

    // Semantic analysis
    let (model, diagnostics) = semantic::analyze(&ast);

    let error_count = output::print_diagnostics(&diagnostics, &source, &filename);
    let mut warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    if error_count > 0 {
        output::print_summary(error_count, warning_count);
        return Err(miette::miette!(
            "compilation failed with {} error(s)",
            error_count
        ));
    }

    // Generate IFC
    let codegen_output = codegen::generate_ifc_with_warnings(&model)
        .map_err(|e| miette::miette!("code generation failed: {}", e))?;

    // Print codegen warnings
    for w in &codegen_output.warnings {
        eprintln!("\x1b[33mwarning\x1b[0m[codegen]: {}", w);
    }
    warning_count += codegen_output.warnings.len();

    // Write output
    let output_file = output_path.unwrap_or_else(|| input.with_extension("ifc"));
    std::fs::write(&output_file, &codegen_output.ifc_string)
        .map_err(|e| miette::miette!("failed to write {}: {}", output_file.display(), e))?;

    if warning_count > 0 {
        output::print_summary(0, warning_count);
    }

    eprintln!(
        "\x1b[32mCompiled\x1b[0m {} -> {} ({} bytes)",
        filename,
        output_file.display(),
        codegen_output.ifc_string.len()
    );

    Ok(())
}

/// Compile a single .bml file that has imports by loading it as a project entry point.
fn run_project_file(input: PathBuf, output_path: Option<PathBuf>) -> miette::Result<()> {
    let project = Project::load_entry_file(&input)
        .map_err(|e| miette::miette!("failed to load {}: {}", input.display(), e))?;

    let (model, diagnostics) = project.analyze();

    let total_errors = output::print_diagnostics_multi_file(&diagnostics, &project.sources());
    let mut total_warnings: usize = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    if total_errors > 0 {
        output::print_summary(total_errors, total_warnings);
        return Err(miette::miette!(
            "compilation failed with {} error(s)",
            total_errors
        ));
    }

    let codegen_output = codegen::generate_ifc_with_warnings(&model)
        .map_err(|e| miette::miette!("code generation failed: {}", e))?;

    for w in &codegen_output.warnings {
        eprintln!("\x1b[33mwarning\x1b[0m[codegen]: {}", w);
    }
    total_warnings += codegen_output.warnings.len();

    let output_file = output_path.unwrap_or_else(|| input.with_extension("ifc"));
    std::fs::write(&output_file, &codegen_output.ifc_string)
        .map_err(|e| miette::miette!("failed to write {}: {}", output_file.display(), e))?;

    if total_warnings > 0 {
        output::print_summary(0, total_warnings);
    }

    eprintln!(
        "\x1b[32mCompiled\x1b[0m {} -> {} ({} bytes, {} files)",
        input.display(),
        output_file.display(),
        codegen_output.ifc_string.len(),
        project.files.len()
    );

    Ok(())
}

fn run_project(input: PathBuf, output_path: Option<PathBuf>) -> miette::Result<()> {
    let project =
        Project::load(&input).map_err(|e| miette::miette!("failed to load project: {}", e))?;

    let (model, diagnostics) = project.analyze();

    let total_errors = output::print_diagnostics_multi_file(&diagnostics, &project.sources());
    let mut total_warnings: usize = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    if total_errors > 0 {
        output::print_summary(total_errors, total_warnings);
        return Err(miette::miette!(
            "compilation failed with {} error(s)",
            total_errors
        ));
    }

    // Generate IFC
    let codegen_output = codegen::generate_ifc_with_warnings(&model)
        .map_err(|e| miette::miette!("code generation failed: {}", e))?;

    // Print codegen warnings
    for w in &codegen_output.warnings {
        eprintln!("\x1b[33mwarning\x1b[0m[codegen]: {}", w);
    }
    total_warnings += codegen_output.warnings.len();

    // Write output
    let output_file = output_path.unwrap_or_else(|| input.join("output.ifc"));
    std::fs::write(&output_file, &codegen_output.ifc_string)
        .map_err(|e| miette::miette!("failed to write {}: {}", output_file.display(), e))?;

    if total_warnings > 0 {
        output::print_summary(0, total_warnings);
    }

    eprintln!(
        "\x1b[32mCompiled\x1b[0m {} -> {} ({} bytes, {} files)",
        input.display(),
        output_file.display(),
        codegen_output.ifc_string.len(),
        project.files.len()
    );

    Ok(())
}
