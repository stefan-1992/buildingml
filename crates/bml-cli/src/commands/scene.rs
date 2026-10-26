use std::path::{Path, PathBuf};

use bml_core::project::{Project, analyze_source};
use bml_core::scene;
use bml_core::semantic::diagnostics::Severity;

pub fn run(input: PathBuf) -> miette::Result<()> {
    let scene_json = if input.is_dir() {
        let project =
            Project::load(&input).map_err(|e| miette::miette!("failed to load project: {}", e))?;
        let (model, _diagnostics) = project.analyze();
        let scene_graph = scene::generate_scene(&model);
        serde_json::to_string(&scene_graph)
            .map_err(|e| miette::miette!("serialization error: {}", e))?
    } else {
        let source = std::fs::read_to_string(&input)
            .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;
        generate_scene_json(&input, &source)?
    };

    println!("{}", scene_json);
    Ok(())
}

fn generate_scene_json(path: &Path, source: &str) -> miette::Result<String> {
    let (model, diagnostics) = analyze_source(Some(path), source).map_err(|errors| {
        miette::miette!(
            "Parse error(s): {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(miette::miette!(
            "{} error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    let scene_graph = scene::generate_scene(&model);
    serde_json::to_string(&scene_graph).map_err(|e| miette::miette!("serialization error: {}", e))
}
