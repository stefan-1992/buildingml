use std::collections::HashMap;

use wasm_bindgen::prelude::*;

use bml_core::codegen;
use bml_core::decompile;
use bml_core::parser::cst_to_ast;
use bml_core::project::Project;
use bml_core::semantic;
use bml_core::semantic::diagnostics::Severity;

/// Lint a BuildingML source string and return diagnostics as JSON.
#[wasm_bindgen]
pub fn lint(source: &str) -> String {
    let ast = match cst_to_ast::parse(source) {
        Ok(ast) => ast,
        Err(errors) => {
            let parse_errors: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "severity": "error",
                        "message": e.to_string(),
                        "code": "E001",
                    })
                })
                .collect();
            return serde_json::to_string(&parse_errors).unwrap_or_else(|_| "[]".to_string());
        }
    };

    let (_, diagnostics) = semantic::analyze(&ast);

    let diag_json: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "code": d.code.as_str(),
                "severity": d.severity_str(),
                "message": d.message,
                "line": d.span.start.line,
                "column": d.span.start.column,
                "help": d.help,
            })
        })
        .collect();

    serde_json::to_string(&diag_json).unwrap_or_else(|_| "[]".to_string())
}

/// Compile a BuildingML source string and return IFC STEP content.
#[wasm_bindgen]
pub fn compile(source: &str) -> Result<String, JsError> {
    let ast = cst_to_ast::parse(source).map_err(|errors| {
        JsError::new(&format!(
            "parse error(s): {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;

    let (model, diagnostics) = semantic::analyze(&ast);

    // Check for errors
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    codegen::generate_ifc(&model).map_err(|e| JsError::new(&e.to_string()))
}

/// Compile a multi-file project from a JSON map of filename → source.
/// Returns IFC STEP content.
#[wasm_bindgen]
pub fn compile_project(files_json: &str) -> Result<String, JsError> {
    let sources: HashMap<String, String> = serde_json::from_str(files_json)
        .map_err(|e| JsError::new(&format!("invalid JSON input: {}", e)))?;

    let project = Project::load_from_sources(sources)
        .map_err(|e| JsError::new(&format!("project load error: {}", e)))?;

    let (model, diagnostics) = project.analyze();

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    codegen::generate_ifc(&model).map_err(|e| JsError::new(&e.to_string()))
}

/// Lint a multi-file project from a JSON map of filename → source.
/// Returns diagnostics as JSON.
#[wasm_bindgen]
pub fn lint_project(files_json: &str) -> String {
    let sources: HashMap<String, String> = match serde_json::from_str(files_json) {
        Ok(s) => s,
        Err(e) => {
            return serde_json::to_string(&[serde_json::json!({
                "severity": "error",
                "message": format!("invalid JSON input: {}", e),
                "code": "E001",
            })])
            .unwrap_or_else(|_| "[]".to_string());
        }
    };

    let project = match Project::load_from_sources(sources) {
        Ok(p) => p,
        Err(e) => {
            return serde_json::to_string(&[serde_json::json!({
                "severity": "error",
                "message": format!("project load error: {}", e),
                "code": "E001",
            })])
            .unwrap_or_else(|_| "[]".to_string());
        }
    };

    let (_, diagnostics) = project.analyze();

    let diag_json: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "code": d.code.as_str(),
                "severity": d.severity_str(),
                "message": d.message,
                "line": d.span.start.line,
                "column": d.span.start.column,
                "help": d.help,
            })
        })
        .collect();

    serde_json::to_string(&diag_json).unwrap_or_else(|_| "[]".to_string())
}

/// Decompile an IFC STEP string to BuildingML source.
#[wasm_bindgen]
pub fn decompile_ifc(ifc_content: &str) -> Result<String, JsError> {
    decompile::decompile_to_bml(ifc_content).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a 3D scene graph from BuildingML source as JSON.
/// Returns a JSON object with `nodes` array containing mesh data for WebGL rendering.
#[wasm_bindgen]
pub fn generate_scene(source: &str) -> Result<String, JsError> {
    let ast = cst_to_ast::parse(source).map_err(|errors| {
        JsError::new(&format!(
            "parse error(s): {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;

    let (model, diagnostics) = semantic::analyze(&ast);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    let scene = bml_core::scene::generate_scene(&model);
    serde_json::to_string(&scene).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a semantic model from BuildingML source as JSON.
/// Returns the full SemanticModel IR for use by the floorplan editor.
#[wasm_bindgen]
pub fn generate_model(source: &str) -> Result<String, JsError> {
    let ast = cst_to_ast::parse(source).map_err(|errors| {
        JsError::new(&format!(
            "parse error(s): {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;

    let (model, diagnostics) = semantic::analyze(&ast);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    serde_json::to_string(&model).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a semantic model from a multi-file project (JSON map of filename → source).
/// Returns the full SemanticModel IR for use by the floorplan editor.
#[wasm_bindgen]
pub fn generate_model_project(files_json: &str) -> Result<String, JsError> {
    let sources: HashMap<String, String> = serde_json::from_str(files_json)
        .map_err(|e| JsError::new(&format!("invalid JSON input: {}", e)))?;

    let project = Project::load_from_sources(sources)
        .map_err(|e| JsError::new(&format!("project load error: {}", e)))?;

    let (model, diagnostics) = project.analyze();

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    serde_json::to_string(&model).map_err(|e| JsError::new(&e.to_string()))
}

/// Generate a 3D scene graph from a multi-file project (JSON map of filename → source).
/// Returns a JSON object with `nodes` array containing mesh data for WebGL rendering.
#[wasm_bindgen]
pub fn generate_scene_project(files_json: &str) -> Result<String, JsError> {
    let sources: HashMap<String, String> = serde_json::from_str(files_json)
        .map_err(|e| JsError::new(&format!("invalid JSON input: {}", e)))?;

    let project = Project::load_from_sources(sources)
        .map_err(|e| JsError::new(&format!("project load error: {}", e)))?;

    let (model, diagnostics) = project.analyze();

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(JsError::new(&format!(
            "{} semantic error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    let scene = bml_core::scene::generate_scene(&model);
    serde_json::to_string(&scene).map_err(|e| JsError::new(&e.to_string()))
}
