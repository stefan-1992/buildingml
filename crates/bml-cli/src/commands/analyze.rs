use std::path::PathBuf;

use bml_core::decompile::ifc_analyzer;

pub fn run(
    inputs: Vec<PathBuf>,
    format: Option<String>,
    output: Option<PathBuf>,
) -> miette::Result<()> {
    if inputs.is_empty() {
        return Err(miette::miette!("no input files specified"));
    }

    let mut analyses = Vec::new();

    for input in &inputs {
        let ifc_content = std::fs::read_to_string(input)
            .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;

        let file_name = input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        eprintln!(
            "\x1b[36mAnalyzing\x1b[0m {} ({} bytes)...",
            input.display(),
            ifc_content.len()
        );

        let analysis = ifc_analyzer::analyze_ifc(&ifc_content, &file_name);
        analyses.push(analysis);
    }

    let aggregate = ifc_analyzer::analyze_multiple(analyses);

    let output_text = match format.as_deref() {
        Some("json") => ifc_analyzer::format_json(&aggregate),
        _ => ifc_analyzer::format_report(&aggregate),
    };

    if let Some(output_path) = output {
        std::fs::write(&output_path, &output_text)
            .map_err(|e| miette::miette!("failed to write {}: {}", output_path.display(), e))?;
        eprintln!(
            "\x1b[32mReport written\x1b[0m to {} ({} bytes)",
            output_path.display(),
            output_text.len()
        );
    } else {
        println!("{}", output_text);
    }

    Ok(())
}
