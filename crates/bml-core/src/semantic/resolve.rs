use std::collections::{HashMap, HashSet};

use crate::ast::span::Span;

use super::containment::layer_list;
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::*;

/// Compute edit distance between two strings (case-insensitive Levenshtein distance).
fn edit_distance_ci(a: &str, b: &str) -> usize {
    let a_lower: Vec<char> = a.to_lowercase().chars().collect();
    let b_lower: Vec<char> = b.to_lowercase().chars().collect();
    let a_len = a_lower.len();
    let b_len = b_lower.len();
    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(a_len + 1) {
        row[0] = i;
    }
    for (j, val) in matrix[0].iter_mut().enumerate().take(b_len + 1) {
        *val = j;
    }

    for (i, ca) in a_lower.iter().enumerate() {
        for (j, cb) in b_lower.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}

/// Suggest a similar storey name from known storeys using case-insensitive Levenshtein.
fn suggest_storey<'a>(input: &str, known_storeys: &[&'a str]) -> Option<&'a str> {
    let max_distance = match input.len() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };

    known_storeys
        .iter()
        .filter_map(|&candidate| {
            let dist = edit_distance_ci(input, candidate);
            if dist <= max_distance && dist > 0 {
                Some((candidate, dist))
            } else {
                None
            }
        })
        .min_by_key(|(_, dist)| *dist)
        .map(|(name, _)| name)
}

/// Check for duplicate element IDs and duplicate material IDs.
/// Also validates material, asset, and storey references.
pub fn resolve_references(model: &SemanticModel) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect all element IDs
    let mut element_ids: HashMap<&str, Vec<Span>> = HashMap::new();
    collect_element_ids(model, &mut element_ids);

    // Check for duplicates (with multi-location related info)
    for (id, spans) in &element_ids {
        if spans.len() > 1 {
            for span in &spans[1..] {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E300,
                        format!("duplicate element ID `{}`", id),
                        *span,
                    )
                    .with_help(format!("first defined at line {}", spans[0].start.line))
                    .with_related(spans[0], format!("first definition of `{}`", id)),
                );
            }
        }
    }

    // Collect material IDs
    let mut material_ids: HashMap<&str, Vec<Span>> = HashMap::new();
    for mat in &model.materials {
        material_ids
            .entry(mat.id.as_str())
            .or_default()
            .push(mat.span);
    }

    // Check for duplicate material IDs
    for (id, spans) in &material_ids {
        if spans.len() > 1 {
            for span in &spans[1..] {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E302,
                        format!("duplicate material ID `{}`", id),
                        *span,
                    )
                    .with_help(format!("first defined at line {}", spans[0].start.line))
                    .with_related(spans[0], format!("first definition of `{}`", id)),
                );
            }
        }
    }

    // Build set of known material IDs for reference checking
    let known_materials: HashSet<&str> = model.materials.iter().map(|m| m.id.as_str()).collect();

    // Check all material references
    check_material_refs(model, &known_materials, &mut diagnostics);
    check_layered_materials(model, &known_materials, &mut diagnostics);

    // Collect all asset IDs and check for duplicates
    let mut asset_ids: HashMap<&str, Vec<Span>> = HashMap::new();
    for lib in &model.asset_libraries {
        for asset in &lib.assets {
            asset_ids
                .entry(asset.id.as_str())
                .or_default()
                .push(asset.span);
        }
    }
    for (id, spans) in &asset_ids {
        if spans.len() > 1 {
            for span in &spans[1..] {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E300,
                        format!("duplicate asset ID `{}`", id),
                        *span,
                    )
                    .with_help(format!("first defined at line {}", spans[0].start.line))
                    .with_related(spans[0], format!("first definition of `{}`", id)),
                );
            }
        }
    }

    // Check all asset references from Furniture elements
    let known_assets: HashSet<&str> = asset_ids.keys().copied().collect();
    check_asset_refs(model, &known_assets, &mut diagnostics);

    // Collect all storey names for cross-storey reference validation
    let known_storeys: Vec<(&str, Span)> = model
        .sites
        .iter()
        .flat_map(|s| s.buildings.iter())
        .flat_map(|b| b.storeys.iter())
        .map(|s| (s.name.as_str(), s.span))
        .collect();

    check_storey_refs(model, &known_storeys, &mut diagnostics);
    check_system_refs(model, &mut diagnostics);

    diagnostics
}

/// Every `connects:` id names an element. Run after routes, curtain walls and
/// stairs are expanded, so their generated parts can be named.
pub(super) fn check_connects(model: &SemanticModel) -> Vec<Diagnostic> {
    let all = || {
        model
            .elements()
            .flat_map(|e| std::iter::once(e).chain(&e.children))
    };
    let ids: HashSet<&str> = all().map(|e| e.id.as_str()).collect();
    let mut diagnostics = Vec::new();
    for e in all() {
        for t in refs(&e.properties, "connects") {
            if !ids.contains(t) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E301,
                        format!("`connects:` on `{}` names no element `{t}`", e.id),
                        e.span,
                    )
                    .with_help("name an element of this file or one it imports"),
                );
            }
        }
    }
    diagnostics
}

/// Every `system:` names a `System:` element.
fn check_system_refs(model: &SemanticModel, diagnostics: &mut Vec<Diagnostic>) {
    let systems: HashSet<&str> = model
        .elements()
        .filter(|e| e.element_type == ElementType::System)
        .map(|e| e.id.as_str())
        .collect();
    for e in model.elements() {
        if let Some(s) = string_or_ident(&e.properties, "system")
            && !systems.contains(s)
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E307,
                    format!("`system: {s}` on `{}` names no `System:` element", e.id),
                    e.span,
                )
                .with_help("define it with `System: <id>` or import the file that does"),
            );
        }
    }
}

fn collect_element_ids<'a>(model: &'a SemanticModel, ids: &mut HashMap<&'a str, Vec<Span>>) {
    for elem in model.elements() {
        collect_element_ids_recursive(elem, ids);
    }
    // Source order: `elements()` lists storeys before building and site contents.
    for spans in ids.values_mut() {
        spans.sort_by_key(|s| (s.file_id, s.start.line, s.start.column));
    }
}

fn collect_element_ids_recursive<'a>(
    elem: &'a ResolvedElement,
    ids: &mut HashMap<&'a str, Vec<Span>>,
) {
    ids.entry(elem.id.as_str()).or_default().push(elem.span);
    for child in &elem.children {
        collect_element_ids_recursive(child, ids);
    }
}

fn check_material_refs(
    model: &SemanticModel,
    known_materials: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for elem in model.elements() {
        check_element_material_refs(elem, known_materials, diagnostics);
    }
}

/// Validate layered materials: each layer names a defined, homogeneous material
/// with a non-negative thickness (IFC membranes have none), and elements using
/// one have the summed thickness.
fn check_layered_materials(
    model: &SemanticModel,
    known_materials: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let by_id: HashMap<&str, &ResolvedMaterial> =
        model.materials.iter().map(|m| (m.id.as_str(), m)).collect();
    for mat in &model.materials {
        if !matches!(mat.material_type, MaterialType::Layered { .. }) {
            continue;
        }
        let items = layer_list(&mat.properties).unwrap_or_default();
        if items.is_empty() {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::E306,
                    format!(
                        "layered material `{}` needs a `layers:` list of `- material: thickness` items",
                        mat.id
                    ),
                    mat.span,
                )
                .with_help("e.g. `layers:` then `  - gypsum_board: 0.0125 m` per layer"),
            );
            continue;
        }
        for item in items {
            let warn =
                |message: String| Diagnostic::warning(DiagnosticCode::E306, message, item.span);
            let ResolvedValue::Number(thickness) = item.value else {
                diagnostics.push(warn(format!(
                    "layer `{}` of `{}` needs a thickness, e.g. `- {}: 0.2 m`",
                    item.key, mat.id, item.key
                )));
                continue;
            };
            if !known_materials.contains(item.key.as_str()) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E301,
                        format!(
                            "unresolved material reference `{}` in layers of `{}`",
                            item.key, mat.id
                        ),
                        item.span,
                    )
                    .with_help("define the material with `Material: <id>` or import it"),
                );
            } else if by_id[item.key.as_str()].layers().is_some() {
                diagnostics.push(warn(format!(
                    "layer `{}` of `{}` is itself layered; layers must be homogeneous materials",
                    item.key, mat.id
                )));
            }
            if thickness < 0.0 {
                diagnostics.push(warn(format!(
                    "layer `{}` of `{}` must not have a negative thickness",
                    item.key, mat.id
                )));
            }
        }
    }

    for elem in model.elements() {
        check_layered_thickness(elem, &by_id, diagnostics);
    }
}

fn check_layered_thickness(
    elem: &ResolvedElement,
    materials: &HashMap<&str, &ResolvedMaterial>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let prop = |key: &str| elem.properties.iter().find(|p| p.key == key);
    if let Some(ResolvedValue::Identifier(id)) = prop("material").map(|p| &p.value)
        && let Some(layers) = materials.get(id.as_str()).and_then(|m| m.layers())
        && let Some(thickness) = prop("thickness")
        && let ResolvedValue::Number(t) = thickness.value
    {
        let sum: f64 = layers.iter().map(|l| l.thickness).sum();
        if (sum - t).abs() > 0.0005 {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::E306,
                    format!(
                        "{} `{}` is {} m thick, but the layers of `{}` add up to {} m",
                        elem.element_type.as_str(),
                        elem.id,
                        (t * 10000.0).round() / 10000.0,
                        id,
                        (sum * 10000.0).round() / 10000.0
                    ),
                    thickness.span,
                )
                .with_help("set `thickness:` to the layer sum or adjust the layers"),
            );
        }
    }
    for child in &elem.children {
        check_layered_thickness(child, materials, diagnostics);
    }
}

fn check_asset_refs(
    model: &SemanticModel,
    known_assets: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for elem in model.elements() {
        check_element_asset_ref(elem, known_assets, diagnostics);
    }
}

/// Validate from_storey / to_storey references on Stair and Elevator elements.
fn check_storey_refs(
    model: &SemanticModel,
    known_storeys: &[(&str, Span)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let storey_names: Vec<&str> = known_storeys.iter().map(|(n, _)| *n).collect();
    let storey_set: HashSet<&str> = storey_names.iter().copied().collect();

    for elem in model.elements() {
        check_element_storey_refs(elem, &storey_set, &storey_names, known_storeys, diagnostics);
    }
}

/// Check if a storey reference matches a storey name.
/// Supports exact match, case-insensitive match, and word-prefix match
/// (since multi-word storey names like "Ground Floor" may parse as just "Ground").
fn storey_name_matches(reference: &str, storey_name: &str) -> bool {
    // Exact match
    if reference == storey_name {
        return true;
    }
    // Case-insensitive exact match
    if reference.eq_ignore_ascii_case(storey_name) {
        return true;
    }
    // Word-prefix match: "Ground" matches "Ground Floor"
    let ref_lower = reference.to_lowercase();
    let name_lower = storey_name.to_lowercase();
    if name_lower.starts_with(&ref_lower)
        && name_lower.len() > ref_lower.len()
        && name_lower.as_bytes()[ref_lower.len()] == b' '
    {
        return true;
    }
    false
}

fn check_element_storey_refs(
    elem: &ResolvedElement,
    _storey_set: &HashSet<&str>,
    storey_names: &[&str],
    storey_spans: &[(&str, Span)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Only Stair and Elevator have storey references
    if elem.element_type != ElementType::Stair && elem.element_type != ElementType::Elevator {
        return;
    }

    for prop in &elem.properties {
        if (prop.key == "from_storey" || prop.key == "to_storey")
            && let ResolvedValue::Identifier(ref storey_name) = prop.value
        {
            // Check if any known storey matches (exact, case-insensitive, or prefix)
            let matched = storey_names
                .iter()
                .find(|&&s| storey_name_matches(storey_name, s));

            if matched.is_some() {
                continue; // Valid reference
            }

            // Try fuzzy matching
            let suggestion = suggest_storey(storey_name, storey_names);

            let mut diag = Diagnostic::error(
                DiagnosticCode::E303,
                format!(
                    "unresolved storey reference `{}` on {} `{}`",
                    storey_name,
                    elem.element_type.as_str(),
                    elem.id
                ),
                prop.span,
            );

            if let Some(suggested) = suggestion {
                diag = diag.with_help(format!("did you mean `{}`?", suggested));
                // Add related info pointing to the suggested storey definition
                if let Some((_, span)) = storey_spans.iter().find(|(n, _)| *n == suggested) {
                    diag = diag.with_related(*span, format!("storey `{}` defined here", suggested));
                }
            } else {
                let names: Vec<&str> = storey_names.to_vec();
                diag = diag.with_help(format!("known storeys: {}", names.join(", ")));
            }

            diagnostics.push(diag);
        }
    }
}

fn check_element_asset_ref(
    elem: &ResolvedElement,
    known_assets: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if elem.element_type != ElementType::Furniture {
        return;
    }
    for prop in &elem.properties {
        if prop.key == "asset"
            && let ResolvedValue::Identifier(ref id) = prop.value
            && !known_assets.contains(id.as_str())
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E301,
                    format!(
                        "unresolved asset reference `{}` on Furniture `{}`",
                        id, elem.id
                    ),
                    prop.span,
                )
                .with_help("define the asset with `## Asset: <id>` in an asset library"),
            );
        }
    }
}

fn check_element_material_refs(
    elem: &ResolvedElement,
    known_materials: &HashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for prop in &elem.properties {
        if matches!(prop.key.as_str(), "material" | "panel_material")
            && let ResolvedValue::Identifier(ref id) = prop.value
            && !known_materials.is_empty()
            && !known_materials.contains(id.as_str())
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E301,
                    format!(
                        "unresolved material reference `{}` on {} `{}`",
                        id,
                        elem.element_type.as_str(),
                        elem.id
                    ),
                    prop.span,
                )
                .with_help("define the material with `Material: <id>` or import it"),
            );
        }
    }

    for child in &elem.children {
        check_element_material_refs(child, known_materials, diagnostics);
    }
}
