use std::path::{Path, PathBuf};

use bml_core::checker;
use bml_core::checker::report;
use bml_core::parser::cst_to_ast;
use bml_core::project::{self, Project};
use bml_core::semantic;

pub fn run(
    input: PathBuf,
    _suite: Option<String>,
    format: Option<String>,
    output: Option<PathBuf>,
) -> miette::Result<()> {
    if input.is_dir() {
        let project =
            Project::load(&input).map_err(|e| miette::miette!("failed to load project: {}", e))?;
        return run_project(&project, &input, format, output);
    }

    let source = std::fs::read_to_string(&input)
        .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;

    // A file with `import` directives is checked as a project entry point.
    if project::has_imports(&source) {
        let project = Project::load_entry_file(&input)
            .map_err(|e| miette::miette!("failed to load {}: {}", input.display(), e))?;
        return run_project(&project, &input, format, output);
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

    let (model, diagnostics) = semantic::analyze(&ast);

    // Print any semantic errors
    let error_count = crate::output::print_diagnostics(&diagnostics, &source, &filename);
    if error_count > 0 {
        return Err(miette::miette!(
            "{} semantic error(s) — fix these before running checks",
            error_count
        ));
    }

    // Run checks
    let check_report = checker::run_checks(&ast, &model);

    // Output results
    output_report(
        &check_report,
        &format,
        &output,
        &[(filename.clone(), source.as_str())],
        &filename,
    )?;

    // Exit with error code if checks failed
    if !check_report.all_passed() {
        std::process::exit(1);
    }

    Ok(())
}

fn run_project(
    project: &Project,
    input: &Path,
    format: Option<String>,
    output: Option<PathBuf>,
) -> miette::Result<()> {
    let (model, diagnostics) = project.analyze();
    let sources = project.sources();

    // Print any semantic errors
    let error_count = crate::output::print_diagnostics_multi_file(&diagnostics, &sources);
    if error_count > 0 {
        return Err(miette::miette!(
            "{} semantic error(s) — fix these before running checks",
            error_count
        ));
    }

    // Run checks from all files
    let mut combined_report = checker::report::CheckReport {
        suites: Vec::new(),
        results: Vec::new(),
    };

    for (index, path) in project.file_paths().into_iter().enumerate() {
        let mut file_report = checker::run_checks(&project.files[path], &model);
        // Check-level results carry the span of the file's AST, which has no file id.
        for result in &mut file_report.results {
            if result.file_id == 0 {
                result.file_id = index as u32 + 1;
            }
        }
        combined_report.suites.extend(file_report.suites);
        combined_report.results.extend(file_report.results);
    }

    // Deduplicate suites
    combined_report.suites.sort();
    combined_report.suites.dedup();

    output_report(
        &combined_report,
        &format,
        &output,
        &sources,
        &input.display().to_string(),
    )?;

    if !combined_report.all_passed() {
        std::process::exit(1);
    }

    Ok(())
}

fn output_report(
    check_report: &report::CheckReport,
    format: &Option<String>,
    output: &Option<PathBuf>,
    sources: &[(String, &str)],
    filename: &str,
) -> miette::Result<()> {
    let format_str = format.as_deref().unwrap_or("text");

    match format_str {
        "json" => {
            let json = check_report
                .to_json()
                .map_err(|e| miette::miette!("failed to generate JSON: {}", e))?;
            if let Some(path) = output {
                std::fs::write(path, &json)
                    .map_err(|e| miette::miette!("failed to write {}: {}", path.display(), e))?;
                eprintln!("Report written to {}", path.display());
            } else {
                println!("{}", json);
            }
        }
        "html" => {
            let html = check_report.to_html(&format!("BuildingML Check Report — {}", filename));
            if let Some(path) = output {
                std::fs::write(path, &html)
                    .map_err(|e| miette::miette!("failed to write {}: {}", path.display(), e))?;
                eprintln!("Report written to {}", path.display());
            } else {
                println!("{}", html);
            }
        }
        "sarif" => {
            let sarif = check_report.to_sarif("buildingml", env!("CARGO_PKG_VERSION"));
            if let Some(path) = output {
                std::fs::write(path, &sarif)
                    .map_err(|e| miette::miette!("failed to write {}: {}", path.display(), e))?;
                eprintln!("Report written to {}", path.display());
            } else {
                println!("{}", sarif);
            }
        }
        _ => {
            report::print_check_results(check_report, sources);
            report::print_check_summary(check_report);
        }
    }

    Ok(())
}
