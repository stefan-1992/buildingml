pub mod containment;
mod covering;
mod curtain_wall;
mod curves;
pub mod diagnostics;
mod footing;
pub mod lint;
mod mep;
pub mod model;
mod parts;
mod penetration;
pub mod registry;
pub mod resolve;
mod stair;
pub mod type_check;
pub mod unit_normalize;

use crate::ast::nodes::SourceFile;
use crate::config::ProjectConfig;
use crate::project::Project;
use diagnostics::{Diagnostic, DiagnosticCode};
use model::SemanticModel;

/// Run the full semantic analysis pipeline on a parsed AST.
///
/// Returns the resolved semantic model and any diagnostics (errors + warnings).
pub fn analyze(source_file: &SourceFile) -> (SemanticModel, Vec<Diagnostic>) {
    analyze_with_config(source_file, None)
}

/// Run the full semantic analysis pipeline with optional project config for lint rules.
pub fn analyze_with_config(
    source_file: &SourceFile,
    config: Option<&ProjectConfig>,
) -> (SemanticModel, Vec<Diagnostic>) {
    let curve_tolerance = config.map_or_else(crate::config::default_curve_tolerance, |c| {
        c.curve_tolerance
    });
    analyze_file(source_file, config, curve_tolerance, true)
}

/// The pipeline of [`analyze_with_config`]; a project cuts `penetrations` once
/// on its merged model, where runs and hosts of all files meet.
fn analyze_file(
    source_file: &SourceFile,
    config: Option<&ProjectConfig>,
    curve_tolerance: f64,
    penetrations: bool,
) -> (SemanticModel, Vec<Diagnostic>) {
    // Phase 0: Expand floor templates (apply_template: directives)
    let mut expanded = source_file.clone();
    crate::template::expand_templates(&mut expanded);

    // Phase 1: Build spatial hierarchy + type check properties
    let (mut model, mut diagnostics) = containment::build_model(&expanded, curve_tolerance);

    // Phase 2: Resolve references (ID uniqueness, material refs, storey refs)
    diagnostics.extend(resolve::resolve_references(&model));

    // Phase 3a: Infer space boundaries from wall topology + resolve auto slab boundaries
    resolve_geometry(&mut model);
    diagnostics.extend(curtain_wall::resolve(&mut model));
    diagnostics.extend(stair::resolve(&mut model));
    // Coverings take their outline before runs cut them.
    covering::resolve(&mut model);
    diagnostics.extend(covering::missing_geometry(&model));
    diagnostics.extend(mep::resolve(&mut model, curve_tolerance));
    if penetrations {
        diagnostics.extend(penetration::resolve(&mut model));
    }
    diagnostics.extend(footing::resolve(&mut model));
    diagnostics.extend(resolve::check_connects(&model));

    // Phase 3b: Unit normalization checks
    for item in &expanded.items {
        match item {
            crate::ast::nodes::Item::Heading(heading) => {
                check_units_in_body(&heading.body, &mut diagnostics)
            }
            crate::ast::nodes::Item::Material(mat) => {
                check_units_in_body(&mat.body, &mut diagnostics)
            }
            _ => {}
        }
    }

    // Phase 4: Opening overflow validation
    validate_opening_overflow(&model, curve_tolerance, &mut diagnostics);

    // Phase 5: Coordinate bounds checking
    validate_coordinate_bounds(&model, &mut diagnostics);

    // Phase 6: Linting rules
    let lint_config = config
        .and_then(|c| c.lint.as_ref())
        .cloned()
        .unwrap_or_default();
    diagnostics.extend(lint::run_lint_checks(&model, &lint_config));

    (model, diagnostics)
}

/// Run semantic analysis across multiple files in a project.
///
/// Merges all file ASTs into a single model and runs cross-file validation.
pub fn analyze_project(project: &Project) -> (SemanticModel, Vec<Diagnostic>) {
    let mut merged_model = SemanticModel {
        sites: Vec::new(),
        materials: Vec::new(),
        asset_libraries: Vec::new(),
    };
    let mut all_diagnostics = Vec::new();

    // Analyze each file individually and merge results
    let paths = project.file_paths();
    for (index, filename) in paths.iter().enumerate() {
        let file_id = index as u32 + 1;
        let (mut file_model, file_diagnostics) = analyze_file(
            &project.files[*filename],
            None,
            project.config.curve_tolerance,
            false,
        );
        file_model.set_file_id(file_id);

        let earlier = merged_model.sites.len();
        let mut merge_diagnostics = Vec::new();
        for site in file_model.sites {
            merge_site(
                &mut merged_model.sites,
                earlier,
                site,
                &mut merge_diagnostics,
            );
        }
        merged_model.materials.extend(file_model.materials);
        merged_model
            .asset_libraries
            .extend(file_model.asset_libraries);

        // Tag each diagnostic with its source file.
        // Filter out reference errors (E300-E307): they are re-checked by the
        // global resolve_references() call below, which has the full merged
        // model (including assets/materials from imported files).
        for mut diag in file_diagnostics.into_iter().chain(merge_diagnostics) {
            if matches!(
                diag.code,
                DiagnosticCode::E300
                    | DiagnosticCode::E301
                    | DiagnosticCode::E302
                    | DiagnosticCode::E303
                    | DiagnosticCode::E304
                    | DiagnosticCode::E305
                    | DiagnosticCode::E306
                    | DiagnosticCode::E307
            ) {
                continue;
            }
            diag.span.file_id = file_id;
            if diag.file.is_none() {
                diag.file = Some(filename.to_string_lossy().to_string());
            }
            all_diagnostics.push(diag);
        }
    }

    let generated = penetration::resolve(&mut merged_model);

    // Run cross-file reference resolution
    for mut diag in generated
        .into_iter()
        .chain(resolve::resolve_references(&merged_model))
        .chain(resolve::check_connects(&merged_model))
    {
        if diag.file.is_none() {
            diag.file = (diag.span.file_id as usize)
                .checked_sub(1)
                .and_then(|i| paths.get(i))
                .map(|p| p.to_string_lossy().to_string());
        }
        all_diagnostics.push(diag);
    }

    (merged_model, all_diagnostics)
}

/// Add a file's site to the project model. Each storey file redeclares
/// `# Site` / `## Building` so it can be analyzed alone; headings with the same
/// name in different files are one site or building, which IFC must contain once
/// (Archicad imports only the first `IfcBuilding`). Only the first `earlier`
/// sites come from other files; same-named headings within one file stay apart.
fn merge_site(
    sites: &mut Vec<model::ResolvedSite>,
    earlier: usize,
    site: model::ResolvedSite,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(target) = sites[..earlier]
        .iter_mut()
        .find(|s| s.name == site.name && s.implicit == site.implicit)
    else {
        sites.push(site);
        return;
    };
    merge_properties(&mut target.properties, site.properties);
    target.spaces.extend(site.spaces);
    target.elements.extend(site.elements);
    let earlier = target.buildings.len();
    for building in site.buildings {
        merge_building(&mut target.buildings, earlier, building, diagnostics);
    }
}

fn merge_building(
    buildings: &mut Vec<model::ResolvedBuilding>,
    earlier: usize,
    building: model::ResolvedBuilding,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(target) = buildings[..earlier]
        .iter_mut()
        .find(|b| b.name == building.name)
    else {
        buildings.push(building);
        return;
    };
    merge_properties(&mut target.properties, building.properties);
    let earlier = target.storeys.len();
    for storey in building.storeys {
        merge_storey(&mut target.storeys, earlier, storey, diagnostics);
    }
    // Files load in path order; list the storeys bottom-up.
    target.storeys.sort_by(|a, b| {
        let elevation =
            |s: &model::ResolvedStorey| model::number(&s.properties, "elevation").unwrap_or(0.0);
        elevation(a).total_cmp(&elevation(b))
    });
    target.spaces.extend(building.spaces);
    target.elements.extend(building.elements);
}

/// A storey redeclared in another file (e.g. a services file per discipline)
/// is the same storey. The first file's `elevation` and `height` win; a
/// different value in a later file is warned.
fn merge_storey(
    storeys: &mut Vec<model::ResolvedStorey>,
    earlier: usize,
    storey: model::ResolvedStorey,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(target) = storeys[..earlier]
        .iter_mut()
        .find(|s| s.name == storey.name)
    else {
        storeys.push(storey);
        return;
    };
    for key in ["elevation", "height"] {
        if let (Some(kept), Some(other)) = (
            model::number(&target.properties, key),
            model::number(&storey.properties, key),
        ) && (kept - other).abs() > 1e-6
        {
            diagnostics.push(Diagnostic::warning(
                DiagnosticCode::E201,
                format!(
                    "Storey `{}` is redeclared with {key} {other} m; the first declaration's {kept} m is used",
                    storey.name
                ),
                storey.span,
            ));
        }
    }
    merge_properties(&mut target.properties, storey.properties);
    target.spaces.extend(storey.spaces);
    target.elements.extend(storey.elements);
}

/// Keep existing properties and add the keys only the other heading sets.
fn merge_properties(into: &mut Vec<model::ResolvedProperty>, from: Vec<model::ResolvedProperty>) {
    for p in from {
        if !into.iter().any(|q| q.key == p.key) {
            into.push(p);
        }
    }
}

/// Resolve geometry-dependent features:
/// 1. Infer space boundaries from wall topology (when no explicit boundary: is set)
/// 2. Resolve `boundary: auto` on slabs to the enclosing space's boundary
fn resolve_geometry(model: &mut SemanticModel) {
    use crate::codegen::geometry;

    for (_, spaces, elements) in model.containers_mut() {
        // Step 1: Infer space boundaries from wall topology
        for space in spaces.iter_mut() {
            let has_boundary = space.properties.iter().any(|p| p.key == "boundary");
            if has_boundary {
                continue;
            }

            // Collect wall axes from this space's elements
            let walls: Vec<((f64, f64), (f64, f64))> = space
                .elements
                .iter()
                .filter(|e| e.element_type == model::ElementType::Wall)
                .filter_map(|e| {
                    e.properties
                        .iter()
                        .find(|p| p.key == "from")
                        .and_then(|p| match &p.value {
                            model::ResolvedValue::Axis { from, to, .. } => Some((*from, *to)),
                            _ => None,
                        })
                })
                .collect();

            if let Some(boundary) = geometry::infer_boundary_from_walls(&walls) {
                space.properties.push(model::ResolvedProperty {
                    key: "boundary".to_string(),
                    value: model::ResolvedValue::Polygon(boundary),
                    span: space.span,
                });
            }
        }

        // Step 2: Resolve boundary: auto on slabs
        // First, collect space boundaries for lookup
        let space_boundaries: Vec<Option<model::ResolvedValue>> = spaces
            .iter()
            .map(|space| {
                space.properties.iter().find_map(|p| {
                    let polygon = matches!(
                        p.value,
                        model::ResolvedValue::Polygon(_)
                            | model::ResolvedValue::PolygonWithHoles { .. }
                    );
                    (p.key == "boundary" && polygon).then(|| p.value.clone())
                })
            })
            .collect();

        // Resolve auto-boundaries on container-level slabs
        for elem in elements.iter_mut() {
            if elem.element_type == model::ElementType::Slab {
                resolve_auto_boundary(elem, &space_boundaries);
            }
        }

        // Resolve auto-boundaries on space-level slabs
        for (space_idx, space) in spaces.iter_mut().enumerate() {
            let own_boundary = space_boundaries.get(space_idx).cloned().flatten();
            for elem in &mut space.elements {
                if elem.element_type == model::ElementType::Slab
                    && let Some(ref boundary) = own_boundary
                {
                    resolve_auto_boundary_with(elem, boundary);
                }
            }
        }
    }
}

/// Replace `boundary: auto` on a slab with the first available space boundary.
fn resolve_auto_boundary(
    elem: &mut model::ResolvedElement,
    space_boundaries: &[Option<model::ResolvedValue>],
) {
    let is_auto = elem.properties.iter().any(|p| {
        p.key == "boundary"
            && matches!(&p.value, model::ResolvedValue::Identifier(id) if id == "auto")
    });

    if !is_auto {
        return;
    }

    // Find the first available space boundary
    if let Some(boundary) = space_boundaries.iter().find_map(|b| b.as_ref())
        && let Some(prop) = elem.properties.iter_mut().find(|p| p.key == "boundary")
    {
        prop.value = boundary.clone();
    }
}

/// Replace `boundary: auto` on a slab with a specific boundary polygon.
fn resolve_auto_boundary_with(elem: &mut model::ResolvedElement, boundary: &model::ResolvedValue) {
    let is_auto = elem.properties.iter().any(|p| {
        p.key == "boundary"
            && matches!(&p.value, model::ResolvedValue::Identifier(id) if id == "auto")
    });

    if !is_auto {
        return;
    }

    if let Some(prop) = elem.properties.iter_mut().find(|p| p.key == "boundary") {
        prop.value = boundary.clone();
    }
}

/// Validate that door/window openings don't overflow their parent wall's length,
/// and that curved walls do not bend tighter than half their thickness.
fn validate_opening_overflow(model: &SemanticModel, tol: f64, diagnostics: &mut Vec<Diagnostic>) {
    for elem in model.elements() {
        check_wall_opening_overflow(elem, tol, diagnostics);
        check_wall_radius(elem, tol, diagnostics);
    }
}

/// E311: the offset of a wall's inner face folds over where the axis bends to a
/// radius below half the thickness.
fn check_wall_radius(elem: &model::ResolvedElement, tol: f64, diagnostics: &mut Vec<Diagnostic>) {
    if elem.element_type != model::ElementType::Wall {
        return;
    }
    let (Some(axis), Some(thickness)) = (
        crate::geometry::axis::WallAxis::of(elem, tol),
        model::number(&elem.properties, "thickness"),
    ) else {
        return;
    };
    let radius = axis.min_radius();
    if radius < thickness / 2.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E311,
                format!(
                    "Wall `{}` bends to radius {radius:.3} m, less than half its thickness ({:.3} m)",
                    elem.id,
                    thickness / 2.0
                ),
                elem.span,
            )
            .with_help("widen the bend or make the wall thinner".to_string()),
        );
    }
}

fn check_wall_opening_overflow(
    elem: &model::ResolvedElement,
    tol: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if elem.element_type != model::ElementType::Wall || elem.children.is_empty() {
        return;
    }

    // Wall length along its axis
    let wall_length = crate::geometry::axis::WallAxis::of(elem, tol).map(|a| a.length());

    let wall_length = match wall_length {
        Some(l) if l > 0.0 => l,
        _ => return,
    };

    // Check each child (door/window) for overflow
    for child in &elem.children {
        if child.element_type != model::ElementType::Door
            && child.element_type != model::ElementType::Window
        {
            continue;
        }

        let offset = child.properties.iter().find_map(|p| {
            if p.key == "offset"
                && let model::ResolvedValue::Number(n) = &p.value
            {
                Some(*n)
            } else {
                None
            }
        });

        let width = child.properties.iter().find_map(|p| {
            if p.key == "width"
                && let model::ResolvedValue::Number(n) = &p.value
            {
                Some(*n)
            } else {
                None
            }
        });

        if let (Some(offset), Some(width)) = (offset, width) {
            let end = offset + width;
            if end > wall_length + 0.001 {
                // Allow tiny floating-point tolerance
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::E304,
                        format!(
                            "{} `{}` overflows Wall `{}`: offset ({:.2}m) + width ({:.2}m) = {:.2}m exceeds wall length {:.2}m",
                            child.element_type.as_str(),
                            child.id,
                            elem.id,
                            offset,
                            width,
                            end,
                            wall_length
                        ),
                        child.span,
                    )
                    .with_help(format!(
                        "reduce offset or width so that offset + width <= {:.2}m",
                        wall_length
                    ))
                    .with_related(elem.span, format!("Wall `{}` defined here", elem.id)),
                );
            }
        }
    }
}

/// Validate that element coordinates are not unreasonably far from the building footprint.
fn validate_coordinate_bounds(model: &SemanticModel, diagnostics: &mut Vec<Diagnostic>) {
    // Buildings and the site's own contents are separate coordinate frames.
    for site in &model.sites {
        for building in &site.buildings {
            let elements: Vec<_> = building
                .storeys
                .iter()
                .flat_map(|s| model::contents(&s.spaces, &s.elements))
                .chain(model::contents(&building.spaces, &building.elements))
                .collect();
            check_bounds_group(&elements, diagnostics);
        }
        let elements: Vec<_> = model::contents(&site.spaces, &site.elements).collect();
        check_bounds_group(&elements, diagnostics);
    }
}

fn check_bounds_group(elements: &[&model::ResolvedElement], diagnostics: &mut Vec<Diagnostic>) {
    // Collect all element coordinates to compute bounding box
    let mut all_coords: Vec<(f64, f64)> = Vec::new();
    collect_element_coords(elements, &mut all_coords);

    if all_coords.len() < 3 {
        return; // Not enough elements for outlier detection
    }

    // Use median-based bounding box (robust to outliers)
    let mut xs: Vec<f64> = all_coords.iter().map(|c| c.0).collect();
    let mut ys: Vec<f64> = all_coords.iter().map(|c| c.1).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Compute IQR (interquartile range) for robust outlier detection
    let q1_x = xs[xs.len() / 4];
    let q3_x = xs[3 * xs.len() / 4];
    let q1_y = ys[ys.len() / 4];
    let q3_y = ys[3 * ys.len() / 4];

    let iqr_x = (q3_x - q1_x).max(1.0);
    let iqr_y = (q3_y - q1_y).max(1.0);

    // Use 10x IQR as the outlier threshold (generous to avoid false positives)
    let threshold_x = iqr_x * 10.0;
    let threshold_y = iqr_y * 10.0;

    // Check each element against the expanded bounds
    check_coords_bounds(
        elements,
        q1_x - threshold_x,
        q3_x + threshold_x,
        q1_y - threshold_y,
        q3_y + threshold_y,
        diagnostics,
    );
}

fn collect_element_coords(elements: &[&model::ResolvedElement], coords: &mut Vec<(f64, f64)>) {
    for elem in elements {
        for prop in &elem.properties {
            match (&prop.key[..], &prop.value) {
                ("position", model::ResolvedValue::Point(x, y)) => {
                    coords.push((*x, *y));
                }
                ("from", model::ResolvedValue::Axis { from, to, .. }) => {
                    coords.push(*from);
                    coords.push(*to);
                }
                ("boundary", model::ResolvedValue::Polygon(pts)) => {
                    coords.extend(pts);
                }
                _ => {}
            }
        }
    }
}

fn check_coords_bounds(
    elements: &[&model::ResolvedElement],
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for elem in elements {
        for prop in &elem.properties {
            let outlier_point = match (&prop.key[..], &prop.value) {
                ("position", model::ResolvedValue::Point(x, y)) => {
                    if *x < min_x || *x > max_x || *y < min_y || *y > max_y {
                        Some((*x, *y))
                    } else {
                        None
                    }
                }
                ("from", model::ResolvedValue::Axis { from, to, .. }) => {
                    if from.0 < min_x || from.0 > max_x || from.1 < min_y || from.1 > max_y {
                        Some(*from)
                    } else if to.0 < min_x || to.0 > max_x || to.1 < min_y || to.1 > max_y {
                        Some(*to)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some((x, y)) = outlier_point {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::E305,
                        format!(
                            "{} `{}` has coordinates ({:.1}, {:.1}) far outside the building footprint",
                            elem.element_type.as_str(),
                            elem.id,
                            x,
                            y
                        ),
                        prop.span,
                    )
                    .with_help("check for misplaced decimal points or unit confusion"),
                );
            }
        }
    }
}

fn check_units_in_body(body: &[crate::ast::nodes::BodyItem], diagnostics: &mut Vec<Diagnostic>) {
    for item in body {
        match item {
            crate::ast::nodes::BodyItem::Property(prop) => {
                if let crate::ast::nodes::Value::NumberWithUnit(_, _)
                | crate::ast::nodes::Value::NumberList(_)
                | crate::ast::nodes::Value::List(_) = &prop.value
                {
                    let unit_diags =
                        unit_normalize::normalize_properties(std::slice::from_ref(prop));
                    diagnostics.extend(unit_diags);
                }
            }
            crate::ast::nodes::BodyItem::Element(elem) => {
                check_units_in_body(&elem.body, diagnostics);
            }
            crate::ast::nodes::BodyItem::CheckDecl(_) | crate::ast::nodes::BodyItem::Comment(_) => {
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;

    #[test]
    fn test_analyze_minimal() {
        let source = "# Site: MySite\n  location: 49.4521, 12.1803\n";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        assert_eq!(model.sites.len(), 1);
        assert_eq!(model.sites[0].name, "MySite");
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_analyze_spatial_hierarchy() {
        let source = std::fs::read_to_string("../../tests/fixtures/spatial_hierarchy.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        // Should have at least one site
        assert!(!model.sites.is_empty());

        // Print diagnostics for debugging (non-fatal)
        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }
    }

    #[test]
    fn test_analyze_full_example() {
        let source = std::fs::read_to_string("../../tests/fixtures/full_example.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        assert!(!model.sites.is_empty());

        // Check that materials were collected
        // (full_example.bml has Material definitions)

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }
    }

    #[test]
    fn test_duplicate_element_id() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    axis: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
  Wall: W1
    axis: (5, 0) to: (5, 5)
    height: 3 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let dup = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E300);
        assert!(dup.is_some(), "expected duplicate ID diagnostic");
    }

    #[test]
    fn test_missing_required_property() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    height: 3 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let missing: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == diagnostics::DiagnosticCode::E101)
            .collect();
        assert!(
            !missing.is_empty(),
            "expected missing required property diagnostics"
        );
        // Wall requires axis, height, thickness — height is present, so axis and thickness should be missing
        assert!(
            missing.len() >= 2,
            "expected at least 2 missing properties, got {}",
            missing.len()
        );
    }

    #[test]
    fn test_wrong_property_type() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    axis: (0, 0) to: (5, 0)
    height: true
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let type_err = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E102);
        assert!(
            type_err.is_some(),
            "expected type mismatch diagnostic for height: true"
        );
    }

    #[test]
    fn test_invalid_nesting() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: S1
    boundary: [(0,0), (5,0), (5,5), (0,5)]
    thickness: 0.2 m
    Window: Win1
      offset: 1 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let nesting = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E202);
        assert!(
            nesting.is_some(),
            "expected invalid nesting diagnostic (Window inside Slab)"
        );
    }

    #[test]
    fn test_heading_level_mismatch() {
        let source = "\
## Site: S
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let mismatch = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E200);
        assert!(
            mismatch.is_some(),
            "expected heading level mismatch diagnostic"
        );
    }

    #[test]
    fn test_unknown_element_type_is_parse_error() {
        // Unknown element types like "Escalator" are rejected at the grammar level,
        // not the semantic level. The tree-sitter grammar only recognizes specific
        // element types (Wall, Slab, Column, Elevator, etc.).
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Escalator: E1
    height: 3 m
";
        let result = cst_to_ast::parse(source);
        assert!(
            result.is_err(),
            "expected parse error for unknown element type 'Escalator'"
        );
    }

    #[test]
    fn test_elevator_element() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Elevator: EL-01
    position: (19.5, 8.5)
    shaft_width: 1.80 m
    shaft_depth: 2.00 m
    type: passenger
";
        let ast = cst_to_ast::parse(source).expect("Elevator should parse");
        let (model, diagnostics) = analyze(&ast);

        // Should have no errors
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity_str() == "error")
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        // Elevator should be a storey-level element
        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1);
        assert_eq!(
            storey.elements[0].element_type,
            model::ElementType::Elevator
        );
        assert_eq!(storey.elements[0].id, "EL-01");
    }

    #[test]
    fn test_material_homogeneous() {
        let source = "\
Material: concrete_c30
  category: concrete
  grade: C30
  density: 2400 kg/m3

# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _diagnostics) = analyze(&ast);

        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.materials[0].id, "concrete_c30");
        match &model.materials[0].material_type {
            model::MaterialType::Homogeneous {
                category,
                grade,
                density,
            } => {
                assert_eq!(category.as_deref(), Some("concrete"));
                assert_eq!(grade.as_deref(), Some("C30"));
                assert!(density.is_some());
            }
            other => panic!("expected Homogeneous material, got {:?}", other),
        }
    }

    #[test]
    fn test_material_layered_fixture() {
        let source = std::fs::read_to_string("../../tests/fixtures/layered_materials.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let wall = model.materials.iter().find(|m| m.id == "ext_wall").unwrap();
        let layers: Vec<(&str, f64)> = wall
            .layers()
            .unwrap()
            .iter()
            .map(|l| (l.name.as_str(), l.thickness))
            .collect();
        // `10 mm` is converted to metres.
        assert_eq!(
            layers,
            [
                ("mineral_wool", 0.14),
                ("concrete", 0.15),
                ("plaster", 0.01)
            ]
        );
    }

    #[test]
    fn test_material_layered_lint() {
        let source = "\
Material: concrete
Material: wall
  layers:
    - concrete: 0.2 m
    - foam: 0.1 m
Material: nested
  layers:
    - wall: 0.3 m
Material: no_thickness
  layers:
    - concrete: thick
Material: bare
  type: layered
Material: odd_unit
  layers:
    - concrete: 10 km

# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.25 m
    material: wall
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);
        let has = |code: &str, text: &str| {
            diagnostics
                .iter()
                .any(|d| d.code.as_str() == code && d.message.contains(text))
        };
        assert!(has("E301", "foam"), "{diagnostics:?}");
        assert!(has("E306", "nested"), "{diagnostics:?}");
        assert!(has("E306", "0.25"), "{diagnostics:?}");
        assert!(
            has("E306", "of `no_thickness` needs a thickness"),
            "{diagnostics:?}"
        );
        assert!(
            has("E306", "`bare` needs a `layers:` list"),
            "{diagnostics:?}"
        );
        assert!(has("E400", "unknown unit `km`"), "{diagnostics:?}");
    }

    #[test]
    fn test_analyze_all_elements_fixture() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        // Should have at least one site with one building
        assert!(!model.sites.is_empty());
        assert!(!model.sites[0].buildings.is_empty());
        assert!(!model.sites[0].buildings[0].storeys.is_empty());

        let gf = &model.sites[0].buildings[0].storeys[0];

        // Count element types
        let walls: Vec<_> = gf
            .elements
            .iter()
            .filter(|e| e.element_type == model::ElementType::Wall)
            .collect();
        assert!(
            walls.len() >= 4,
            "expected at least 4 walls, got {}",
            walls.len()
        );

        let columns: Vec<_> = gf
            .elements
            .iter()
            .filter(|e| e.element_type == model::ElementType::Column)
            .collect();
        assert!(!columns.is_empty(), "expected at least 1 column");

        let beams: Vec<_> = gf
            .elements
            .iter()
            .filter(|e| e.element_type == model::ElementType::Beam)
            .collect();
        assert!(!beams.is_empty(), "expected at least 1 beam");

        // Check wall has window child
        let wall_with_window = walls.iter().find(|w| !w.children.is_empty());
        assert!(
            wall_with_window.is_some(),
            "expected at least one wall with window/door children"
        );

        // Materials should be collected
        assert!(model.materials.len() >= 2, "expected at least 2 materials");

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }
    }

    #[test]
    fn test_analyze_two_storey_house() {
        let source = std::fs::read_to_string("../../tests/fixtures/two_storey_house.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        // Should have no semantic errors
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity_str() == "error")
            .collect();
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

        // Spatial hierarchy: 1 site, 1 building, 3 storeys
        assert_eq!(model.sites.len(), 1);
        assert_eq!(model.sites[0].buildings.len(), 1);
        assert_eq!(
            model.sites[0].buildings[0].storeys.len(),
            3,
            "expected 3 storeys (Ground, First, Roof)"
        );

        // 3 materials
        assert_eq!(model.materials.len(), 3, "expected 3 materials");

        let gf = &model.sites[0].buildings[0].storeys[0];
        let ff = &model.sites[0].buildings[0].storeys[1];

        // Ground floor should have 5 spaces
        assert!(
            gf.spaces.len() >= 5,
            "expected at least 5 ground floor spaces, got {}",
            gf.spaces.len()
        );

        // Count walls across all ground floor spaces
        let gf_walls: usize = gf
            .spaces
            .iter()
            .flat_map(|s| s.elements.iter())
            .filter(|e| e.element_type == model::ElementType::Wall)
            .count();
        assert!(
            gf_walls >= 10,
            "expected at least 10 ground floor walls, got {}",
            gf_walls
        );

        // First floor should have spaces with walls and railing
        let ff_walls: usize = ff
            .spaces
            .iter()
            .flat_map(|s| s.elements.iter())
            .filter(|e| e.element_type == model::ElementType::Wall)
            .count();
        assert!(
            ff_walls >= 10,
            "expected at least 10 first floor walls, got {}",
            ff_walls
        );
    }

    // =========================================================================
    // Phase 7a' tests: Language Improvements for Scale
    // =========================================================================

    #[test]
    fn test_elevator_full_pipeline() {
        let source = "\
# Site: TestSite
## Building: TestBuilding
### Storey: Ground Floor
  elevation: 0 m
  height: 3.2 m
  Elevator: EL-01
    position: (19.5, 8.5)
    shaft_width: 1.80 m
    shaft_depth: 2.00 m
    type: passenger
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1);
        let el = &storey.elements[0];
        assert_eq!(el.element_type, model::ElementType::Elevator);
        assert_eq!(el.id, "EL-01");

        // Verify IFC codegen
        let ifc = crate::codegen::generate_ifc(&model).unwrap();
        assert!(
            ifc.contains("IFCTRANSPORTELEMENT"),
            "should contain IFC transport element"
        );

        // Verify scene graph
        let scene = crate::scene::generate_scene(&model);
        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].element_type, "Elevator");
        assert_eq!(scene.nodes[0].mesh.vertices.len(), 24); // box
    }

    #[test]
    fn test_railing_with_axis() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Railing: RL-01
    from: (0, 0) to: (10, 0)
    height: 1.10 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1);
        let rl = &storey.elements[0];
        assert_eq!(rl.element_type, model::ElementType::Railing);

        // Verify scene graph positions the railing along the axis
        let scene = crate::scene::generate_scene(&model);
        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].element_type, "Railing");
    }

    #[test]
    fn test_ramp_with_axis() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Ramp: RM-01
    from: (16, 18) to: (19, 12)
    width: 5.50 m
    slope: 0.05
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1);
        assert_eq!(storey.elements[0].element_type, model::ElementType::Ramp);
    }

    #[test]
    fn test_extended_space_usage_enum() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
#### Space: Lobby
  usage: lobby
#### Space: Laundry
  usage: laundry
#### Space: Terrace
  usage: terrace
#### Space: Parking
  usage: parking
#### Space: Mechanical Room
  usage: mechanical
#### Space: Dining
  usage: dining
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.spaces.len(), 6);
    }

    #[test]
    fn test_space_zone_property() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
#### Space: Living Room 101
  usage: living
  area: 25.0 m2
  zone: unit_101
#### Space: Kitchen 101
  usage: kitchen
  area: 12.0 m2
  zone: unit_101
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.spaces.len(), 2);

        // Both spaces should have zone property
        for space in &storey.spaces {
            let zone = space.properties.iter().find(|p| p.key == "zone");
            assert!(
                zone.is_some(),
                "space {} should have zone property",
                space.name
            );
        }
    }

    #[test]
    fn test_auto_id_generation() {
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  Wall:
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall:
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m
  Column:
    position: (3, 2.5)
    profile: rectangle(0.3, 0.3)
    height: 2.8 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 3);

        // All elements should have auto-generated IDs
        assert!(
            !storey.elements[0].id.is_empty(),
            "wall 1 should have an ID"
        );
        assert!(
            !storey.elements[1].id.is_empty(),
            "wall 2 should have an ID"
        );
        assert!(
            !storey.elements[2].id.is_empty(),
            "column should have an ID"
        );

        // IDs should be unique
        assert_ne!(storey.elements[0].id, storey.elements[1].id);
        assert_ne!(storey.elements[0].id, storey.elements[2].id);

        // Check auto-ID format
        assert!(
            storey.elements[0].id.starts_with("W-GF"),
            "wall auto-ID should start with W-GF, got {}",
            storey.elements[0].id
        );
        assert!(
            storey.elements[2].id.starts_with("COL-GF"),
            "column auto-ID should start with COL-GF, got {}",
            storey.elements[2].id
        );
    }

    #[test]
    fn test_auto_id_mixed_with_explicit() {
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  Wall: W-EXPLICIT-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall:
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements[0].id, "W-EXPLICIT-01");
        assert!(storey.elements[1].id.starts_with("W-GF"));
    }

    #[test]
    fn test_template_basic() {
        let source = "\
Template: SimpleFloor
  Wall: W-{f}-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W-{f}-02
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m

# Site: S
## Building: B
### Storey: Floor 2
  elevation: 3.5 m
  apply_template: SimpleFloor
  f: F2
### Storey: Floor 3
  elevation: 7.0 m
  apply_template: SimpleFloor
  f: F3
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let b = &model.sites[0].buildings[0];
        assert_eq!(b.storeys.len(), 2);

        // Floor 2 should have expanded walls with F2 substitution
        let f2 = &b.storeys[0];
        assert_eq!(f2.elements.len(), 2, "floor 2 should have 2 walls");
        assert_eq!(f2.elements[0].id, "W-F2-01");
        assert_eq!(f2.elements[1].id, "W-F2-02");

        // Floor 3 should have expanded walls with F3 substitution
        let f3 = &b.storeys[1];
        assert_eq!(f3.elements.len(), 2, "floor 3 should have 2 walls");
        assert_eq!(f3.elements[0].id, "W-F3-01");
        assert_eq!(f3.elements[1].id, "W-F3-02");
    }

    #[test]
    fn test_template_with_additional_elements() {
        let source = "\
Template: BaseFloor
  Wall: W-{f}-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m

# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  apply_template: BaseFloor
  f: GF
  Slab: SLAB-GF
    boundary: [(0,0), (6,0), (6,5), (0,5)]
    thickness: 0.22 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let gf = &model.sites[0].buildings[0].storeys[0];
        // Should have 1 wall from template + 1 slab added directly
        assert_eq!(gf.elements.len(), 2, "should have template wall + slab");
        assert_eq!(gf.elements[0].id, "W-GF-01");
        assert_eq!(gf.elements[1].id, "SLAB-GF");
    }

    #[test]
    fn test_storey_level_elements() {
        // Verify that elements can be placed directly under a storey
        // without requiring a #### Space: wrapper
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.2 m

  Slab: SLAB-GF
    boundary: [(0,0), (40,0), (40,18), (0,18)]
    thickness: 0.25 m

  Column: COL-GF-01
    position: (8.0, 9.0)
    profile: rectangle(0.3, 0.3)
    height: 2.80 m

  #### Space: Unit 101
    usage: living
    area: 95.0 m2
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        // 2 elements at storey level (slab + column)
        assert_eq!(
            storey.elements.len(),
            2,
            "expected 2 storey-level elements, got {}",
            storey.elements.len()
        );
        // 1 space
        assert_eq!(storey.spaces.len(), 1);
        assert_eq!(storey.spaces[0].name, "Unit 101");
    }

    #[test]
    fn test_asset_library_resolve() {
        let source = concat!(
            "# AssetLibrary: Test Furniture\n",
            "## Asset: SimpleDesk\n",
            "  width: 1.2 m\n",
            "  depth: 0.6 m\n",
            "  height: 0.75 m\n",
            "### Part: Top\n",
            "  geometry: box(1.2, 0.6, 0.03)\n",
            "  position: (0, 0, 0.72)\n",
            "### Part: Leg\n",
            "  geometry: cylinder(0.02, 0.72)\n",
            "  position: (0.1, 0.1, 0)\n",
            "\n",
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "#### Space: Office\n",
            "  area: 20 m2\n",
            "  Furniture: DESK-01\n",
            "    asset: SimpleDesk\n",
            "    position: (2.0, 1.5)\n",
            "    rotation: 90\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        // No errors expected (asset reference should resolve)
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity_str() == "error")
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        // Verify asset library was resolved
        assert_eq!(model.asset_libraries.len(), 1);
        assert_eq!(model.asset_libraries[0].name, "Test Furniture");
        assert_eq!(model.asset_libraries[0].assets.len(), 1);

        let desk = &model.asset_libraries[0].assets[0];
        assert_eq!(desk.id, "SimpleDesk");
        assert_eq!(desk.parts.len(), 2);

        // Verify parts have resolved geometry
        let top = &desk.parts[0];
        assert_eq!(top.name, "Top");
        assert!(top.geometry.is_some());
        assert!(
            matches!(top.geometry, Some(model::GeometryPrimitive::Box(w, d, h))
            if (w - 1.2).abs() < 0.01 && (d - 0.6).abs() < 0.01 && (h - 0.03).abs() < 0.01)
        );
        assert_eq!(top.position, (0.0, 0.0, 0.72));

        let leg = &desk.parts[1];
        assert_eq!(leg.name, "Leg");
        assert!(
            matches!(leg.geometry, Some(model::GeometryPrimitive::Cylinder(r, h))
            if (r - 0.02).abs() < 0.01 && (h - 0.72).abs() < 0.01)
        );

        // Verify furniture element has asset property
        let space = &model.sites[0].buildings[0].storeys[0].spaces[0];
        assert_eq!(space.elements.len(), 1);
        assert_eq!(space.elements[0].id, "DESK-01");
        let asset_prop = space.elements[0]
            .properties
            .iter()
            .find(|p| p.key == "asset")
            .expect("should have asset property");
        assert!(
            matches!(&asset_prop.value, model::ResolvedValue::Identifier(id) if id == "SimpleDesk")
        );
    }

    #[test]
    fn test_unresolved_asset_reference() {
        let source = concat!(
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "  Furniture: F1\n",
            "    asset: NonExistentAsset\n",
            "    position: (1.0, 1.0)\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let asset_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unresolved asset reference"))
            .collect();
        assert_eq!(
            asset_errors.len(),
            1,
            "should report 1 unresolved asset reference"
        );
    }

    #[test]
    fn test_furniture_assets_fixture() {
        let source = std::fs::read_to_string(format!(
            "{}/tests/fixtures/furniture_assets.bml",
            env!("CARGO_MANIFEST_DIR").replace("/crates/bml-core", "")
        ))
        .unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = analyze(&ast);

        // Should have no errors
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity_str() == "error")
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        // 1 asset library with 2 assets
        assert_eq!(model.asset_libraries.len(), 1);
        assert_eq!(model.asset_libraries[0].assets.len(), 2);
        assert_eq!(model.asset_libraries[0].assets[0].id, "OfficeDesk");
        assert_eq!(model.asset_libraries[0].assets[1].id, "OfficeChair");

        // OfficeDesk has 4 parts, OfficeChair has 3 parts
        assert_eq!(model.asset_libraries[0].assets[0].parts.len(), 4);
        assert_eq!(model.asset_libraries[0].assets[1].parts.len(), 3);
    }

    // ====================================================================
    // "Did you mean?" suggestion tests
    // ====================================================================

    #[test]
    fn test_unknown_property_suggests_similar() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thicknes: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let unknown = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E103);
        assert!(
            unknown.is_some(),
            "expected unknown property diagnostic for 'thicknes'"
        );
        let help = unknown.unwrap().help.as_deref().unwrap_or("");
        assert!(
            help.contains("did you mean `thickness`"),
            "expected 'did you mean thickness' in help, got: {}",
            help
        );
    }

    #[test]
    fn test_unknown_property_lists_known_when_no_match() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
    zzz_unknown: 42
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = analyze(&ast);

        let unknown = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E103);
        assert!(
            unknown.is_some(),
            "expected unknown property diagnostic for 'zzz_unknown'"
        );
        let help = unknown.unwrap().help.as_deref().unwrap_or("");
        assert!(
            help.contains("known properties for Wall"),
            "expected known properties list in help, got: {}",
            help
        );
    }

    #[test]
    fn test_edit_distance_suggestions_various() {
        // Test the edit distance function directly through the type_check module
        use type_check::{known_property_names, suggest_element_type};

        // Property suggestions
        let wall_props = known_property_names(model::ElementType::Wall);
        let wall_refs: Vec<&str> = wall_props.iter().copied().collect();

        // "heigh" -> "height" (1 deletion)
        assert_eq!(
            type_check::suggest_element_type("heigh", &wall_refs),
            Some("height")
        );
        // "materail" -> "material" (transposition-like, dist 2)
        assert_eq!(
            type_check::suggest_element_type("materail", &wall_refs),
            Some("material")
        );

        // Element type suggestions
        let types = model::ElementType::all_names();
        assert_eq!(suggest_element_type("Wal", &types), Some("Wall"));
        assert_eq!(suggest_element_type("Winow", &types), Some("Window"));
        assert_eq!(suggest_element_type("Colum", &types), Some("Column"));
    }

    // =========================================================================
    // Phase 8a tests: Compiler Quality
    // =========================================================================

    #[test]
    fn test_opening_element_in_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 3 m
    thickness: 0.24 m
    Opening: OP-01
      offset: 2 m
      width: 0.6 m
      height: 1.0 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1); // wall only
        let wall = &storey.elements[0];
        assert_eq!(wall.element_type, model::ElementType::Wall);

        // Opening should be a child of the wall
        let opening = wall
            .children
            .iter()
            .find(|c| c.element_type == model::ElementType::Opening);
        assert!(opening.is_some(), "expected Opening as child of Wall");
        assert_eq!(opening.unwrap().id, "OP-01");
    }

    #[test]
    fn test_grid_element() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Grid: GR-01
    position: (0, 0)
    x_spacing: 6 m
    y_spacing: 5 m
    x_count: 4
    y_count: 3
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        let grid = storey
            .elements
            .iter()
            .find(|e| e.element_type == model::ElementType::Grid);
        assert!(grid.is_some(), "expected Grid element");
        assert_eq!(grid.unwrap().id, "GR-01");
    }

    #[test]
    fn test_opening_invalid_nesting() {
        // Openings may void any host; windows only belong in walls
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: S1
    boundary: [(0,0), (5,0), (5,5), (0,5)]
    thickness: 0.2 m
    Opening: OP-01
      position: (1, 1)
      width: 0.6 m
      height: 1.0 m
    Window: WIN-01
      offset: 1 m
      width: 0.6 m
      height: 1.0 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        let nesting: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == diagnostics::DiagnosticCode::E202)
            .collect();
        assert_eq!(
            nesting.len(),
            1,
            "expected only the Window in the Slab to be rejected: {:?}",
            nesting
        );
        assert!(nesting[0].message.contains("WIN-01") || nesting[0].message.contains("Window"));
    }

    #[test]
    fn test_opening_overflow_validation() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
    Window: WIN-01
      offset: 4.5 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        // offset (4.5) + width (1.2) = 5.7 > wall length (5.0)
        let overflow = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E304);
        assert!(
            overflow.is_some(),
            "expected opening overflow diagnostic (offset + width > wall length)"
        );
    }

    #[test]
    fn test_storey_reference_validation() {
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  height: 3.2 m
### Storey: First Floor
  elevation: 3.2 m
  height: 3.2 m
  Stair: ST-01
    position: (5, 2)
    from_storey: Groond
    to_storey: First
    width: 1.2 m
    riser_count: 18
    riser_height: 0.178 m
    tread_depth: 0.27 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        // "Groond" should be flagged as unresolved (fuzzy match to "Ground Floor")
        let unresolved = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E303);
        assert!(
            unresolved.is_some(),
            "expected unresolved storey reference diagnostic for 'Groond'"
        );
    }

    #[test]
    fn test_storey_reference_valid_prefix() {
        // "Ground" should match "Ground Floor" via word-prefix matching
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  height: 3.2 m
### Storey: First Floor
  elevation: 3.2 m
  height: 3.2 m
  Stair: ST-01
    position: (5, 2)
    from_storey: Ground
    to_storey: First
    width: 1.2 m
    riser_count: 18
    riser_height: 0.178 m
    tread_depth: 0.27 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        let unresolved = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E303);
        assert!(
            unresolved.is_none(),
            "word-prefix 'Ground' should match 'Ground Floor': {:?}",
            unresolved
        );
    }

    #[test]
    fn test_multi_location_diagnostics() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
  Wall: W1
    from: (5, 0) to: (5, 5)
    height: 3 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        let dup = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E300);
        assert!(dup.is_some(), "expected duplicate ID diagnostic");

        // Should have related info pointing to the first definition
        let dup = dup.unwrap();
        assert!(
            !dup.related.is_empty(),
            "expected related info on duplicate diagnostic"
        );
        assert!(
            dup.related[0].message.contains("first definition"),
            "related message should reference first definition"
        );
    }

    #[test]
    fn test_grid_ifc_codegen() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Grid: GR-01
    position: (0, 0)
    x_spacing: 6 m
    y_spacing: 5 m
    x_count: 3
    y_count: 2
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, _) = analyze(&ast);
        let ifc = crate::codegen::generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCGRID"), "should contain IFCGRID entity");
        assert!(
            ifc.contains("IFCGRIDAXIS"),
            "should contain IFCGRIDAXIS entities"
        );
    }

    #[test]
    fn test_grid_scene_graph() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Grid: GR-01
    position: (0, 0)
    x_spacing: 6 m
    y_spacing: 5 m
    x_count: 3
    y_count: 2
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, _) = analyze(&ast);
        let scene = crate::scene::generate_scene(&model);
        let grid_node = scene.nodes.iter().find(|n| n.element_type == "Grid");
        assert!(grid_node.is_some(), "expected Grid in scene graph");
        assert!(
            !grid_node.unwrap().mesh.vertices.is_empty(),
            "Grid should have mesh vertices"
        );
    }

    #[test]
    fn test_coordinate_bounds_building_and_site_level() {
        let walls = "  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 3 m
    thickness: 0.24 m
  Wall: W2
    from: (10, 0) to: (10, 8)
    height: 3 m
    thickness: 0.24 m
";
        let column = "  Column: C-out
    position: (5000, 5000)
    height: 3 m
    profile: rectangle(0.3, 0.3)
";
        let outliers = |source: &str| {
            let ast = cst_to_ast::parse(source).expect("should parse");
            let (_, diagnostics) = analyze(&ast);
            diagnostics
                .into_iter()
                .filter(|d| d.code == diagnostics::DiagnosticCode::E305)
                .map(|d| d.message)
                .collect::<Vec<_>>()
        };
        // A building-level column is checked against the storey walls
        let source = format!("# Site: S\n## Building: B\n{column}### Storey: GF\n{walls}");
        let found = outliers(&source);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("C-out"));
        // Site-level contents form their own frame, apart from the building
        let source = format!("# Site: S\n{column}{walls}## Building: B\n### Storey: GF\n{walls}");
        let found = outliers(&source);
        assert_eq!(found.len(), 1, "{found:?}");
        let source = format!("# Site: S\n{walls}## Building: B\n{column}{walls}");
        assert_eq!(outliers(&source).len(), 1);
    }

    #[test]
    fn test_coordinate_bounds_validation() {
        // Need enough clustered elements so the outlier exceeds 10x the bounding box
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 3 m
    thickness: 0.24 m
  Wall: W2
    from: (10, 0) to: (10, 8)
    height: 3 m
    thickness: 0.24 m
  Wall: W3
    from: (10, 8) to: (0, 8)
    height: 3 m
    thickness: 0.24 m
  Wall: W4
    from: (0, 8) to: (0, 0)
    height: 3 m
    thickness: 0.24 m
  Column: C-out
    position: (5000, 5000)
    height: 3 m
    profile: rectangle(0.3, 0.3)
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = analyze(&ast);

        // Column at (5000, 5000) is far outside the building bounding box (0-10, 0-8)
        // Threshold = max(10, 8) * 10 = 100. Column at 5000 >> expanded max of 110.
        let bounds = diagnostics
            .iter()
            .find(|d| d.code == diagnostics::DiagnosticCode::E305);
        assert!(
            bounds.is_some(),
            "expected coordinate bounds diagnostic for outlier column. diags: {:?}",
            diagnostics
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect::<Vec<_>>()
        );
    }

    // =========================================================================
    // Template edge case tests
    // =========================================================================

    #[test]
    fn test_template_nested_variables() {
        // Template using two variables ({f} and {n}) in IDs and space names
        let source = "\
Template: NestedVarFloor
  Wall: W-{f}-{n}-EXT
    from: (0, 0) to: (10, 0)
    height: 2.8 m
    thickness: 0.3 m
  Wall: W-{f}-{n}-INT
    from: (5, 0) to: (5, 5)
    height: 2.8 m
    thickness: 0.15 m

#### Space: Unit {n}A on Floor {f}
  usage: living
  area: 50.0 m2

  Wall: W-{f}-{n}A-01
    from: (0, 0) to: (5, 0)
    height: 2.8 m
    thickness: 0.15 m

# Site: S
## Building: B
### Storey: Floor 1
  elevation: 0 m
  height: 3.0 m
  apply_template: NestedVarFloor
  f: F1
  n: 1
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];

        // Should have 2 storey-level walls from template
        assert_eq!(storey.elements.len(), 2, "expected 2 storey-level walls");
        assert_eq!(storey.elements[0].id, "W-F1-1-EXT");
        assert_eq!(storey.elements[1].id, "W-F1-1-INT");

        // Should have 1 space with substituted name
        assert_eq!(storey.spaces.len(), 1);
        assert_eq!(storey.spaces[0].name, "Unit 1A on Floor F1");

        // Space should have 1 wall with nested variable substitution
        assert_eq!(storey.spaces[0].elements.len(), 1);
        assert_eq!(storey.spaces[0].elements[0].id, "W-F1-1A-01");
    }

    #[test]
    fn test_template_applied_to_multiple_storeys() {
        // Same template applied to 3 different storeys
        let source = "\
Template: SimpleWalls
  Wall: W-{f}-01
    from: (0, 0) to: (8, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W-{f}-02
    from: (8, 0) to: (8, 6)
    height: 2.8 m
    thickness: 0.24 m

# Site: S
## Building: B
### Storey: Floor 1
  elevation: 0 m
  apply_template: SimpleWalls
  f: F1
### Storey: Floor 2
  elevation: 3.0 m
  apply_template: SimpleWalls
  f: F2
### Storey: Floor 3
  elevation: 6.0 m
  apply_template: SimpleWalls
  f: F3
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let b = &model.sites[0].buildings[0];
        assert_eq!(b.storeys.len(), 3);

        // Each storey should have 2 walls with unique IDs
        for (i, storey) in b.storeys.iter().enumerate() {
            let floor_num = i + 1;
            assert_eq!(
                storey.elements.len(),
                2,
                "storey {} should have 2 walls",
                floor_num
            );
            assert_eq!(storey.elements[0].id, format!("W-F{}-01", floor_num));
            assert_eq!(storey.elements[1].id, format!("W-F{}-02", floor_num));
        }

        // Verify no duplicate IDs across storeys
        let all_ids: Vec<&str> = b
            .storeys
            .iter()
            .flat_map(|s| s.elements.iter().map(|e| e.id.as_str()))
            .collect();
        let unique_count = {
            let mut set = std::collections::HashSet::new();
            all_ids.iter().filter(|id| set.insert(**id)).count()
        };
        assert_eq!(all_ids.len(), unique_count, "all IDs should be unique");
    }

    #[test]
    fn test_template_static_no_variables() {
        // Template with no {variable} placeholders — content is copied verbatim
        let source = "\
Template: StaticLayout
  Slab: SLAB-STATIC
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.25 m
  Column: COL-STATIC-01
    position: (0, 0)
    height: 3 m
    profile: rectangle(0.3, 0.3)

# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  apply_template: StaticLayout
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 2, "expected slab + column");
        assert_eq!(storey.elements[0].id, "SLAB-STATIC");
        assert_eq!(storey.elements[1].id, "COL-STATIC-01");
    }

    #[test]
    fn test_template_with_unused_variable_binding() {
        // Variable binding provided but not used in the template — should not cause errors
        let source = "\
Template: MinimalFloor
  Wall: W-01
    from: (0, 0) to: (5, 0)
    height: 2.8 m
    thickness: 0.24 m

# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  apply_template: MinimalFloor
  f: GF
  n: 1
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(storey.elements.len(), 1);
        // ID should remain W-01 since {f} is not in the template
        assert_eq!(storey.elements[0].id, "W-01");
    }

    #[test]
    fn test_template_auto_id_within_template() {
        // Template with bare Type: (no ID) — auto-IDs should be generated per storey
        let source = "\
Template: AutoIdFloor
  Wall:
    from: (0, 0) to: (8, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall:
    from: (8, 0) to: (8, 6)
    height: 2.8 m
    thickness: 0.24 m

# Site: S
## Building: B
### Storey: Floor 1
  elevation: 0 m
  apply_template: AutoIdFloor
### Storey: Floor 2
  elevation: 3.0 m
  apply_template: AutoIdFloor
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let b = &model.sites[0].buildings[0];
        assert_eq!(b.storeys.len(), 2);

        // Floor 1 auto-IDs
        let f1 = &b.storeys[0];
        assert_eq!(f1.elements.len(), 2);
        assert!(
            f1.elements[0].id.starts_with("W-"),
            "auto-ID should start with W-: {}",
            f1.elements[0].id
        );
        assert!(
            f1.elements[1].id.starts_with("W-"),
            "auto-ID should start with W-: {}",
            f1.elements[1].id
        );
        // IDs should be different
        assert_ne!(f1.elements[0].id, f1.elements[1].id);

        // Floor 2 auto-IDs should be different from Floor 1
        let f2 = &b.storeys[1];
        assert_eq!(f2.elements.len(), 2);

        // Collect all IDs and verify uniqueness
        let all_ids: Vec<&str> = b
            .storeys
            .iter()
            .flat_map(|s| s.elements.iter().map(|e| e.id.as_str()))
            .collect();
        let unique: std::collections::HashSet<&&str> = all_ids.iter().collect();
        assert_eq!(
            all_ids.len(),
            unique.len(),
            "auto-generated IDs should be unique across storeys: {:?}",
            all_ids
        );
    }

    #[test]
    fn test_template_nonexistent_reference() {
        // Referencing a template that does not exist — should be handled gracefully
        // (template not found means no expansion, storey remains empty)
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  apply_template: NonExistentTemplate
  f: GF
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        // The storey should still exist but have no expanded elements
        let storey = &model.sites[0].buildings[0].storeys[0];
        assert_eq!(
            storey.elements.len(),
            0,
            "nonexistent template should not add elements"
        );

        // Check that there are no crash or panic — graceful handling
        // apply_template property may be flagged as unknown, which is acceptable
        let _ = diagnostics;
    }

    #[test]
    fn test_template_edge_cases_fixture() {
        // Parse the template_edge_cases.bml fixture file
        let source = std::fs::read_to_string(format!(
            "{}/tests/fixtures/template_edge_cases.bml",
            env!("CARGO_MANIFEST_DIR").replace("/crates/bml-core", "")
        ))
        .expect("should read fixture file");

        let ast = cst_to_ast::parse(&source).expect("should parse fixture");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let b = &model.sites[0].buildings[0];
        assert_eq!(b.storeys.len(), 4, "expected 4 storeys");

        // Floor 1: NestedVarFloor with f=F1, n=1
        let f1 = &b.storeys[0];
        assert_eq!(f1.elements[0].id, "W-F1-1-EXT");
        assert_eq!(f1.elements[1].id, "W-F1-1-INT");
        assert_eq!(f1.spaces.len(), 1);
        assert_eq!(f1.spaces[0].name, "Unit 1A on Floor F1");

        // Floor 2: NestedVarFloor with f=F2, n=2
        let f2 = &b.storeys[1];
        assert_eq!(f2.elements[0].id, "W-F2-2-EXT");
        assert_eq!(f2.elements[1].id, "W-F2-2-INT");
        assert_eq!(f2.spaces.len(), 1);
        assert_eq!(f2.spaces[0].name, "Unit 2A on Floor F2");

        // Floor 3: StaticLayout (no variables)
        let f3 = &b.storeys[2];
        assert_eq!(f3.elements.len(), 3, "static template: slab + 2 columns");
        assert_eq!(f3.elements[0].id, "SLAB-STATIC");
        assert_eq!(f3.elements[1].id, "COL-STATIC-01");
        assert_eq!(f3.elements[2].id, "COL-STATIC-02");

        // Floor 4: AutoIdFloor (auto-generated IDs)
        let f4 = &b.storeys[3];
        assert_eq!(
            f4.elements.len(),
            2,
            "auto-ID template should expand 2 walls"
        );
        // Walls should have auto-generated IDs starting with W-
        for elem in &f4.elements {
            assert!(
                elem.id.starts_with("W-"),
                "auto-ID should start with W-: {}",
                elem.id
            );
        }
    }

    #[test]
    fn test_template_with_spaces_and_storey_elements() {
        // Template containing both storey-level elements and space headings
        let source = "\
Template: MixedFloor
  Slab: SLAB-{f}
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.25 m
  Column: COL-{f}-01
    position: (5, 4)
    height: 3 m
    profile: rectangle(0.3, 0.3)

#### Space: Office {f}
  usage: office
  area: 40.0 m2

  Wall: W-{f}-OFFICE-01
    from: (0, 0) to: (5, 0)
    height: 2.8 m
    thickness: 0.12 m

#### Space: Corridor {f}
  usage: corridor
  area: 20.0 m2

# Site: S
## Building: B
### Storey: Floor 1
  elevation: 0 m
  apply_template: MixedFloor
  f: F1
### Storey: Floor 2
  elevation: 3 m
  apply_template: MixedFloor
  f: F2
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (model, diagnostics) = analyze(&ast);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let b = &model.sites[0].buildings[0];
        assert_eq!(b.storeys.len(), 2);

        for (i, storey) in b.storeys.iter().enumerate() {
            let f = format!("F{}", i + 1);
            // Storey-level elements: slab + column
            assert_eq!(
                storey.elements.len(),
                2,
                "storey {} should have 2 storey-level elements",
                f
            );
            assert_eq!(storey.elements[0].id, format!("SLAB-{}", f));
            assert_eq!(storey.elements[1].id, format!("COL-{}-01", f));

            // 2 spaces: Office and Corridor
            assert_eq!(storey.spaces.len(), 2, "storey {} should have 2 spaces", f);
            assert_eq!(storey.spaces[0].name, format!("Office {}", f));
            assert_eq!(storey.spaces[1].name, format!("Corridor {}", f));

            // Office space has 1 wall
            assert_eq!(storey.spaces[0].elements.len(), 1);
            assert_eq!(
                storey.spaces[0].elements[0].id,
                format!("W-{}-OFFICE-01", f)
            );
        }
    }

    // =========================================================================
    // Multi-file project tests
    // =========================================================================

    #[test]
    fn test_multi_file_project_merge() {
        use crate::project::Project;
        use std::collections::HashMap;

        // File 1: Materials + Site/Building structure
        let main_bml = "\
Material: concrete
  category: concrete
  grade: C30

# Site: TestSite
## Building: MainBuilding
### Storey: Ground Floor
  elevation: 0 m
  height: 3.2 m

  Wall: W-GF-01
    from: (0, 0) to: (10, 0)
    height: 3 m
    thickness: 0.24 m
    material: concrete
";

        // File 2: Another building in the same site
        let annex_bml = "\
# Site: TestSite
## Building: Annex
### Storey: Ground Floor
  elevation: 0 m
  height: 2.8 m

  Wall: W-ANNEX-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.2 m
";

        let mut sources = HashMap::new();
        sources.insert("main.bml".to_string(), main_bml.to_string());
        sources.insert("annex.bml".to_string(), annex_bml.to_string());

        let project = Project::load_from_sources(sources).expect("should load multi-file project");
        let (model, diagnostics) = project.analyze();

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        // Model should contain merged content from both files
        // Both files declare `# Site: TestSite`, which merges into one site
        assert_eq!(model.sites.len(), 1);

        // Should have the material from file 1
        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.materials[0].id, "concrete");

        // Find all buildings across all sites
        let all_buildings: Vec<&str> = model
            .sites
            .iter()
            .flat_map(|s| s.buildings.iter().map(|b| b.name.as_str()))
            .collect();
        assert!(
            all_buildings.contains(&"MainBuilding"),
            "should contain MainBuilding"
        );
        assert!(all_buildings.contains(&"Annex"), "should contain Annex");

        // Verify elements from both files exist
        let all_wall_ids: Vec<&str> = model
            .sites
            .iter()
            .flat_map(|s| {
                s.buildings.iter().flat_map(|b| {
                    b.storeys.iter().flat_map(|st| {
                        st.elements
                            .iter()
                            .chain(st.spaces.iter().flat_map(|sp| sp.elements.iter()))
                            .map(|e| e.id.as_str())
                    })
                })
            })
            .collect();
        assert!(
            all_wall_ids.contains(&"W-GF-01"),
            "should contain wall from main.bml"
        );
        assert!(
            all_wall_ids.contains(&"W-ANNEX-01"),
            "should contain wall from annex.bml"
        );
    }

    #[test]
    fn test_multi_file_project_with_import() {
        use crate::project::Project;
        use std::collections::HashMap;

        // File 1: Materials library
        let materials_bml = "\
Material: steel
  category: steel
  grade: S355
  density: 7850 kg/m3

Material: glass
  category: glass
  density: 2500 kg/m3
";

        // File 2: Building that imports materials
        let building_bml = "\
import materials from \"materials.bml\"

# Site: OfficeSite
## Building: OfficeBlock
### Storey: Ground Floor
  elevation: 0 m
  height: 4.0 m

  Wall: W-01
    from: (0, 0) to: (12, 0)
    height: 3.5 m
    thickness: 0.3 m
    material: steel
  Column: C-01
    position: (6, 0)
    height: 3.5 m
    profile: rectangle(0.4, 0.4)
    material: steel
";

        let mut sources = HashMap::new();
        sources.insert("materials.bml".to_string(), materials_bml.to_string());
        sources.insert("building.bml".to_string(), building_bml.to_string());

        let project =
            Project::load_from_sources(sources).expect("should load project with imports");
        let (model, diagnostics) = project.analyze();

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        // Should have materials from the imported file
        assert!(
            model.materials.len() >= 2,
            "expected at least 2 materials, got {}",
            model.materials.len()
        );

        let material_ids: Vec<&str> = model.materials.iter().map(|m| m.id.as_str()).collect();
        assert!(
            material_ids.contains(&"steel"),
            "should contain steel material"
        );
        assert!(
            material_ids.contains(&"glass"),
            "should contain glass material"
        );

        // Should have the building with elements
        let all_buildings: Vec<&str> = model
            .sites
            .iter()
            .flat_map(|s| s.buildings.iter().map(|b| b.name.as_str()))
            .collect();
        assert!(
            all_buildings.contains(&"OfficeBlock"),
            "should contain OfficeBlock building"
        );

        // Verify both elements from the building file
        let all_element_ids: Vec<&str> = model
            .sites
            .iter()
            .flat_map(|s| {
                s.buildings.iter().flat_map(|b| {
                    b.storeys
                        .iter()
                        .flat_map(|st| st.elements.iter().map(|e| e.id.as_str()))
                })
            })
            .collect();
        assert!(
            all_element_ids.contains(&"W-01"),
            "should contain wall W-01"
        );
        assert!(
            all_element_ids.contains(&"C-01"),
            "should contain column C-01"
        );
    }

    #[test]
    fn test_multi_file_diagnostics_tagged_with_file() {
        use crate::project::Project;
        use std::collections::HashMap;

        // File with a deliberate issue (unknown property)
        let file_a = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
    zzz_bad_prop: 42
";

        let mut sources = HashMap::new();
        sources.insert("file_a.bml".to_string(), file_a.to_string());

        let project = Project::load_from_sources(sources).expect("should load project");
        let (_, diagnostics) = project.analyze();

        // Diagnostics should be tagged with their source file
        let tagged: Vec<_> = diagnostics.iter().filter(|d| d.file.is_some()).collect();
        assert!(
            !tagged.is_empty(),
            "diagnostics should be tagged with source file"
        );

        // At least one diagnostic should reference file_a.bml
        let has_file_a = tagged
            .iter()
            .any(|d| d.file.as_ref().unwrap().contains("file_a.bml"));
        assert!(
            has_file_a,
            "at least one diagnostic should reference file_a.bml, got: {:?}",
            tagged.iter().map(|d| d.file.as_ref()).collect::<Vec<_>>()
        );
    }

    /// A reference error found after merging names the file of the element,
    /// and its span carries that file's id.
    #[test]
    fn test_project_shared_import_loaded_once() {
        use crate::project::Project;

        let dir = std::env::temp_dir().join("bml_test_shared_import_loaded_once");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("storeys")).unwrap();
        std::fs::write(
            dir.join("materials.bml"),
            "Material: concrete\n  category: concrete\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("project.bml"),
            "import m from \"./materials.bml\"\nimport gf from \"./storeys/gf.bml\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("storeys/gf.bml"),
            "import m from \"../materials.bml\"\n\n# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n",
        )
        .unwrap();

        let project = Project::load_entry_file(&dir.join("project.bml")).unwrap();
        let (_, diagnostics) = project.analyze();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(project.files.len(), 3, "{:?}", project.files.keys());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn test_project_merges_redeclared_site_and_building() {
        use crate::project::Project;
        use std::collections::HashMap;

        let storey = |name: &str, z: u32| {
            format!("# Site: S\n## Building: B\n### Storey: {name}\n  elevation: {z} m\n")
        };
        let sources = HashMap::from([
            (
                "project.bml".to_string(),
                "# Site: S\n  location: 48.2, 11.6\n## Building: B\n  type: educational\n"
                    .to_string(),
            ),
            ("gf.bml".to_string(), storey("GF", 0)),
            ("ff.bml".to_string(), storey("FF", 3)),
            (
                // A storey redeclared in another file is the same storey
                "services.bml".to_string(),
                format!(
                    "{}  Pipe: P1\n    from: (0, 0, 0.1) to: (0, 0, 2)\n",
                    storey("GF", 0)
                ),
            ),
            (
                // Two headings of one file are two buildings, even with one name
                "other.bml".to_string(),
                "# Site: S\n## Building: Annex\n## Building: Annex\n".to_string(),
            ),
        ]);
        let project = Project::load_from_sources(sources).expect("should load project");
        let (model, diagnostics) = project.analyze();

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(model.sites.len(), 1);
        let site = &model.sites[0];
        assert!(site.properties.iter().any(|p| p.key == "location"));
        let mut names: Vec<_> = site.buildings.iter().map(|b| b.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["Annex", "Annex", "B"]);
        let b = site.buildings.iter().find(|b| b.name == "B").unwrap();
        assert!(b.properties.iter().any(|p| p.key == "type"));
        let storeys: Vec<_> = b.storeys.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(storeys, ["GF", "FF"]);
        assert_eq!(b.storeys[0].elements.len(), 1);
    }

    #[test]
    fn test_project_warns_on_redeclared_storey_elevation() {
        use crate::project::Project;
        use std::collections::HashMap;

        let storey =
            |z: f64| format!("# Site: S\n## Building: B\n### Storey: GF\n  elevation: {z} m\n");
        let sources = HashMap::from([
            ("a.bml".to_string(), storey(0.0)),
            ("b.bml".to_string(), storey(0.5)),
        ]);
        let project = Project::load_from_sources(sources).expect("should load project");
        let (model, diagnostics) = project.analyze();

        assert_eq!(model.sites[0].buildings[0].storeys.len(), 1);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("elevation 0.5 m"));
        assert_eq!(diagnostics[0].file.as_deref(), Some("b.bml"));
    }

    #[test]
    fn test_auto_slab_takes_space_boundary_with_holes() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
#### Space: Hall
  boundary: { outer: [(0, 0), (10, 0), (10, 10), (0, 10)], holes: [[(4, 4), (6, 4), (6, 6), (4, 6)]] }
  Slab: SL1
    boundary: auto
    thickness: 0.2 m
";
        let (model, diagnostics) = analyze(&cst_to_ast::parse(source).unwrap());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let slab = model.elements().find(|e| e.id == "SL1").unwrap();
        let boundary = slab
            .properties
            .iter()
            .find(|p| p.key == "boundary")
            .unwrap();
        assert!(
            matches!(&boundary.value, model::ResolvedValue::PolygonWithHoles { holes, .. } if holes.len() == 1),
            "{:?}",
            boundary.value
        );
    }

    #[test]
    fn test_project_resolve_diagnostic_file() {
        use crate::project::Project;
        use std::collections::HashMap;

        let file_a = "\
Material: concrete
  category: concrete
";
        let file_b = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
    material: no_such_material
";
        let sources = HashMap::from([
            ("a.bml".to_string(), file_a.to_string()),
            ("b.bml".to_string(), file_b.to_string()),
        ]);
        let project = Project::load_from_sources(sources).expect("should load project");
        let (_, diagnostics) = project.analyze();

        let diag = diagnostics
            .iter()
            .find(|d| d.message.contains("no_such_material"))
            .unwrap_or_else(|| panic!("unresolved reference: {diagnostics:?}"));
        assert_eq!(diag.file.as_deref(), Some("b.bml"));
        assert_eq!(project.sources()[diag.span.file_id as usize - 1].0, "b.bml");
    }

    // =========================================================================
    // Multi-file reference fixture tests
    // =========================================================================

    #[test]
    fn test_multi_file_reference_fixture_loads() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        if !fixture_path.exists() {
            panic!("Multi-file fixture not found at {:?}", fixture_path);
        }

        let project = Project::load(fixture_path).expect("should load multi-file project");

        // Should have loaded multiple files
        assert!(
            project.files.len() >= 7,
            "expected at least 7 files, got {}",
            project.files.len()
        );
    }

    #[test]
    fn test_multi_file_reference_fixture_lint() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");
        let (_, diagnostics) = project.analyze();

        // Should have no errors (only warnings are acceptable)
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == crate::semantic::diagnostics::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "multi-file fixture should have no errors, got {} errors: {:?}",
            errors.len(),
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // Shared walls must be declared only once
        let overlaps: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E407)
            .map(|d| &d.message)
            .collect();
        assert!(
            overlaps.is_empty(),
            "unexpected wall overlaps: {overlaps:#?}"
        );
    }

    #[test]
    fn test_multi_file_reference_fixture_storey_count() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");
        let (model, _) = project.analyze();

        // Count total storeys across all sites/buildings
        let total_storeys: usize = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .count();

        assert_eq!(
            total_storeys, 7,
            "expected 7 storeys (Ground + F1-F4 + F5 + Roof), got {}",
            total_storeys
        );
    }

    #[test]
    fn test_multi_file_reference_fixture_materials() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");
        let (model, _) = project.analyze();

        assert_eq!(
            model.materials.len(),
            4,
            "expected 4 materials, got {}",
            model.materials.len()
        );

        let material_names: Vec<&str> = model.materials.iter().map(|m| m.id.as_str()).collect();
        assert!(material_names.contains(&"reinforced_concrete"));
        assert!(material_names.contains(&"exterior_insulated"));
        assert!(material_names.contains(&"interior_partition"));
        assert!(material_names.contains(&"shear_wall_concrete"));
    }

    /// Asset libraries declared in `buildingml.toml` [assets] are loaded automatically
    /// before the entry file — no explicit `import` directive needed.
    #[test]
    fn test_toml_assets_auto_include() {
        use crate::project::Project;
        use std::fs;

        let dir = std::env::temp_dir().join("bml_test_toml_assets_auto_include");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        let root = dir.as_path();

        // Write a minimal buildingml.toml referencing an asset file
        fs::write(
            root.join("buildingml.toml"),
            r#"buildingml_version = "0.1"
entry = "project.bml"
assets = ["my-assets.bml"]
"#,
        )
        .unwrap();

        // Write the asset library file
        fs::write(
            root.join("my-assets.bml"),
            r#"# AssetLibrary: Test Assets
## Asset: TestChair
  category: seating
  width: 0.5 m
  depth: 0.5 m
  height: 0.9 m
### Part: Seat
  geometry: box(0.5, 0.5, 0.05)
  position: (0, 0, 0.45)
  material: wood_light
"#,
        )
        .unwrap();

        // Write project.bml that uses the asset without any import
        fs::write(
            root.join("project.bml"),
            r#"# Site: Test
## Building: Test Building
### Storey: Ground Floor
  elevation: 0 m
  Furniture: CHAIR-01
    asset: TestChair
    position: (1.0, 1.0)
"#,
        )
        .unwrap();

        let project = Project::load(root).expect("project should load");
        let (model, diagnostics) = project.analyze();

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E301)
            .collect();
        assert!(
            errors.is_empty(),
            "auto-included asset should resolve without import: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let total_assets: usize = model.asset_libraries.iter().map(|l| l.assets.len()).sum();
        assert_eq!(total_assets, 1, "TestChair asset should be loaded");
    }

    #[test]
    fn test_multi_file_reference_fixture_asset_library() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");
        let (model, _) = project.analyze();

        assert!(
            !model.asset_libraries.is_empty(),
            "expected at least one asset library"
        );
        let lib = &model.asset_libraries[0];
        assert_eq!(lib.name, "Residential Furniture");
        assert_eq!(
            lib.assets.len(),
            2,
            "expected 2 assets (BasicDesk, BasicChair)"
        );
    }

    #[test]
    fn test_multi_file_reference_fixture_compile() {
        use crate::codegen::generate_ifc;
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");
        let (model, _) = project.analyze();

        let ifc = generate_ifc(&model);
        assert!(
            ifc.is_ok(),
            "multi-file fixture should compile to IFC: {:?}",
            ifc.err()
        );

        let ifc_content = ifc.unwrap();
        assert!(
            ifc_content.contains("FILE_DESCRIPTION"),
            "IFC output should contain FILE_DESCRIPTION header"
        );
        assert!(
            ifc_content.contains("IFCBUILDINGSTOREY"),
            "IFC output should contain storey entities"
        );
    }

    #[test]
    fn test_multi_file_matches_single_file_storey_count() {
        use crate::project::Project;
        use std::path::Path;

        // Load multi-file fixture
        let multi_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let multi_project = Project::load(multi_path).expect("should load multi-file project");
        let (multi_model, _) = multi_project.analyze();

        // Load single-file fixture
        let single_source =
            std::fs::read_to_string("../../tests/fixtures/six_storey_residential.bml")
                .expect("should read single-file fixture");
        let single_ast = crate::parser::cst_to_ast::parse(&single_source).unwrap();
        let (single_model, _) = analyze(&single_ast);

        // Compare storey counts
        let multi_storeys: usize = multi_model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .count();
        let single_storeys: usize = single_model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .count();

        assert_eq!(
            multi_storeys, single_storeys,
            "multi-file ({}) and single-file ({}) should have same number of storeys",
            multi_storeys, single_storeys
        );
    }

    #[test]
    fn test_multi_file_matches_single_file_element_count() {
        use crate::project::Project;
        use std::path::Path;

        // Load multi-file fixture
        let multi_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let multi_project = Project::load(multi_path).expect("should load multi-file project");
        let (multi_model, _) = multi_project.analyze();

        // Load single-file fixture
        let single_source =
            std::fs::read_to_string("../../tests/fixtures/six_storey_residential.bml")
                .expect("should read single-file fixture");
        let single_ast = crate::parser::cst_to_ast::parse(&single_source).unwrap();
        let (single_model, _) = analyze(&single_ast);

        // Count all elements across all storeys
        let count_elements = |model: &crate::semantic::model::SemanticModel| -> usize {
            model
                .sites
                .iter()
                .flat_map(|s| &s.buildings)
                .flat_map(|b| &b.storeys)
                .map(|st| {
                    st.elements.len() + st.spaces.iter().map(|sp| sp.elements.len()).sum::<usize>()
                })
                .sum()
        };

        let multi_elements = count_elements(&multi_model);
        let single_elements = count_elements(&single_model);

        assert_eq!(
            multi_elements, single_elements,
            "multi-file ({}) and single-file ({}) should have same total element count",
            multi_elements, single_elements
        );
    }

    #[test]
    fn test_multi_file_reference_fixture_checks() {
        use crate::project::Project;
        use std::path::Path;

        let fixture_path = Path::new("../../tests/fixtures/six_storey_residential_project");
        let project = Project::load(fixture_path).expect("should load multi-file project");

        // Checks are parsed at AST level
        let check_count: usize = project
            .files
            .values()
            .flat_map(|ast| &ast.items)
            .filter(|item| matches!(item, crate::ast::nodes::Item::CheckDecl(_)))
            .count();
        let suite_count: usize = project
            .files
            .values()
            .flat_map(|ast| &ast.items)
            .filter(|item| matches!(item, crate::ast::nodes::Item::CheckSuite(_)))
            .count();

        assert!(
            check_count >= 1,
            "expected at least 1 top-level check, got {}",
            check_count
        );
        assert!(
            suite_count >= 1,
            "expected at least 1 check suite, got {}",
            suite_count
        );
    }

    /// University fixture: template floors, curved glazing, roof slab with a void,
    /// raked Audimax tiers. Both the base and the LOD 300 version must analyze without
    /// errors or warnings.
    #[test]
    fn test_university_campus_project_clean() {
        use crate::project::Project;
        use std::path::Path;

        for fixture in [
            "university_campus_project",
            "university_campus_lod300_project",
            "university_campus_lod350_project",
            "university_campus_fluid_project",
        ] {
            let fixture_path = Path::new("../../tests/fixtures").join(fixture);
            let project = Project::load(&fixture_path)
                .unwrap_or_else(|e| panic!("{fixture}: should load: {e:?}"));
            let (model, diagnostics) = project.analyze();

            let problems: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.severity != crate::semantic::diagnostics::Severity::Info)
                .map(|d| &d.message)
                .collect();
            assert!(
                problems.is_empty(),
                "{fixture}: unexpected diagnostics: {problems:#?}"
            );

            // File merge order is not stable, so compare sorted
            let mut storeys: Vec<&str> = model
                .sites
                .iter()
                .flat_map(|s| &s.buildings)
                .flat_map(|b| &b.storeys)
                .map(|s| s.name.as_str())
                .collect();
            storeys.sort();
            assert_eq!(
                storeys,
                [
                    "Level 0", "Level 1", "Level 2", "Level 3", "Level 4", "Roof"
                ]
            );
        }
    }

    /// Regression test: when a .bml file imports an asset library from another file,
    /// Furniture elements referencing those assets must not produce E301 errors.
    /// Previously, per-file analysis generated E301s before the merged model had the
    /// imported assets, causing false positives.
    #[test]
    fn test_cross_file_asset_import_no_false_e301() {
        use crate::project::Project;
        use std::path::Path;

        let fixture = Path::new("../../tests/fixtures/common_furniture_test.bml");
        let project = Project::load_entry_file(fixture).expect("should load fixture with imports");
        let (model, diagnostics) = project.analyze();

        let e301_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E301 && d.message.contains("unresolved asset"))
            .collect();

        assert!(
            e301_errors.is_empty(),
            "expected no unresolved asset errors, got: {:?}",
            e301_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // All 25 assets from the common furniture library should be loaded
        let total_assets: usize = model.asset_libraries.iter().map(|l| l.assets.len()).sum();
        assert_eq!(
            total_assets, 25,
            "expected 25 assets in common furniture library"
        );
    }

    /// Verifies that all assets in the common furniture library have the expected categories.
    #[test]
    fn test_common_furniture_library_categories() {
        use crate::project::Project;
        use std::path::Path;

        let fixture = Path::new("../../tests/fixtures/common_furniture_test.bml");
        let project = Project::load_entry_file(fixture).expect("should load fixture with imports");
        let (model, _) = project.analyze();

        let lib = model
            .asset_libraries
            .iter()
            .find(|l| l.name == "Common Furniture")
            .expect("should find Common Furniture library");

        let asset_ids: Vec<&str> = lib.assets.iter().map(|a| a.id.as_str()).collect();

        // Residential
        assert!(asset_ids.contains(&"SingleBed"), "missing SingleBed");
        assert!(asset_ids.contains(&"DoubleBed"), "missing DoubleBed");
        assert!(asset_ids.contains(&"KingBed"), "missing KingBed");
        assert!(asset_ids.contains(&"Wardrobe"), "missing Wardrobe");
        assert!(asset_ids.contains(&"Nightstand"), "missing Nightstand");
        // Living/dining
        assert!(asset_ids.contains(&"Sofa"), "missing Sofa");
        assert!(asset_ids.contains(&"DiningTable"), "missing DiningTable");
        assert!(asset_ids.contains(&"DiningChair"), "missing DiningChair");
        assert!(asset_ids.contains(&"CoffeeTable"), "missing CoffeeTable");
        assert!(asset_ids.contains(&"Bookshelf"), "missing Bookshelf");
        // Kitchen
        assert!(
            asset_ids.contains(&"KitchenCounter"),
            "missing KitchenCounter"
        );
        assert!(asset_ids.contains(&"Refrigerator"), "missing Refrigerator");
        assert!(asset_ids.contains(&"SinkUnit"), "missing SinkUnit");
        assert!(asset_ids.contains(&"StoveOven"), "missing StoveOven");
        // Bathroom
        assert!(asset_ids.contains(&"Toilet"), "missing Toilet");
        assert!(asset_ids.contains(&"Bathtub"), "missing Bathtub");
        assert!(asset_ids.contains(&"ShowerTray"), "missing ShowerTray");
        assert!(asset_ids.contains(&"Washbasin"), "missing Washbasin");
        // Office
        assert!(asset_ids.contains(&"OfficeDesk"), "missing OfficeDesk");
        assert!(asset_ids.contains(&"OfficeChair"), "missing OfficeChair");
        assert!(asset_ids.contains(&"MeetingTable"), "missing MeetingTable");
        assert!(
            asset_ids.contains(&"FilingCabinet"),
            "missing FilingCabinet"
        );
        assert!(asset_ids.contains(&"MonitorStand"), "missing MonitorStand");
        // Common
        assert!(asset_ids.contains(&"Bench"), "missing Bench");
        assert!(asset_ids.contains(&"Planter"), "missing Planter");
    }

    fn slab_source(boundary: &str) -> String {
        format!(
            "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Slab: SL-L1\n    boundary: {boundary}\n    thickness: 0.2 m\n"
        )
    }

    fn analyze_with_tolerance(source: &str, tol: f64) -> (SemanticModel, Vec<Diagnostic>) {
        let ast = cst_to_ast::parse(source).expect("should parse");
        let config = ProjectConfig {
            curve_tolerance: tol,
            ..Default::default()
        };
        analyze_with_config(&ast, Some(&config))
    }

    fn boundary_of(elem: &model::ResolvedElement) -> Vec<(f64, f64)> {
        match &elem
            .properties
            .iter()
            .find(|p| p.key == "boundary")
            .unwrap()
            .value
        {
            model::ResolvedValue::Polygon(pts) => pts.clone(),
            v => panic!("expected Polygon, got {v:?}"),
        }
    }

    const SPEC_SLAB: &str =
        "[(0, 0), smooth (10, -2), (20, 0), (20, 15), smooth (10, 18), (0, 15)]";

    #[test]
    fn smooth_slab_resolves_to_dense_polygon() {
        let (model, diagnostics) = analyze(&cst_to_ast::parse(&slab_source(SPEC_SLAB)).unwrap());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != diagnostics::Severity::Error),
            "{diagnostics:?}"
        );
        let slab = model.elements().find(|e| e.id == "SL-L1").unwrap();
        let pts = boundary_of(slab);
        assert!(pts.len() > 20, "only {} points", pts.len());
        for q in [
            (0.0, 0.0),
            (10.0, -2.0),
            (20.0, 0.0),
            (20.0, 15.0),
            (10.0, 18.0),
            (0.0, 15.0),
        ] {
            assert!(
                pts.iter().any(|p| (p.0 - q.0).hypot(p.1 - q.1) < 1e-9),
                "{q:?} missing from boundary"
            );
        }
        assert!(matches!(
            slab.curve("boundary"),
            Some(model::ElementCurve::Region { holes, .. }) if holes.is_empty()
        ));
    }

    /// Spec §1 wall path: open, so no closing edge that could cross it.
    #[test]
    fn open_smooth_path_does_not_cross_itself() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Railing: RL-1\n    path: [(0, 0), smooth (6, 2), smooth (12, 1), (18, 5)]\n    height: 1 m\n";
        let (_, diagnostics) = containment::build_model(&cst_to_ast::parse(source).unwrap(), 0.005);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::E310),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn open_smooth_path_crossing_itself_is_an_error() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Railing: RL-1\n    path: [(0, 0), (10, 0), smooth (5, 5), (5, -5)]\n    height: 1 m\n";
        let (_, diagnostics) = containment::build_model(&cst_to_ast::parse(source).unwrap(), 0.005);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E310 && d.message.contains("RL-1")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn plain_polygon_has_no_curve() {
        let (model, _) = analyze(
            &cst_to_ast::parse(&slab_source("[(0, 0), (20, 0), (20, 15), (0, 15)]")).unwrap(),
        );
        let slab = model.elements().find(|e| e.id == "SL-L1").unwrap();
        assert_eq!(boundary_of(slab).len(), 4);
        assert!(slab.curves.is_empty());
    }

    #[test]
    fn curve_tolerance_from_config() {
        let fine = analyze_with_tolerance(&slab_source(SPEC_SLAB), 0.005).0;
        let coarse = analyze_with_tolerance(&slab_source(SPEC_SLAB), 0.05).0;
        let n = |m: &SemanticModel| boundary_of(m.elements().next().unwrap()).len();
        assert!(
            n(&coarse) < n(&fine),
            "coarse {} vs fine {}",
            n(&coarse),
            n(&fine)
        );
    }

    #[test]
    fn project_uses_config_curve_tolerance() {
        let n = |tol: f64| {
            let project = Project {
                config: ProjectConfig {
                    curve_tolerance: tol,
                    ..Default::default()
                },
                root: std::path::PathBuf::from("."),
                files: [(
                    std::path::PathBuf::from("a.bml"),
                    cst_to_ast::parse(&slab_source(SPEC_SLAB)).unwrap(),
                )]
                .into(),
                file_sources: Default::default(),
            };
            boundary_of(analyze_project(&project).0.elements().next().unwrap()).len()
        };
        assert!(n(0.05) < n(0.005));
    }

    #[test]
    fn curve_tolerance_defaults_to_5mm() {
        assert_eq!(ProjectConfig::default().curve_tolerance, 0.005);
        let config: ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(config.curve_tolerance, 0.005);
        let config: ProjectConfig = toml::from_str("curve_tolerance = 0.01").unwrap();
        assert_eq!(config.curve_tolerance, 0.01);
    }

    /// Point coordinates carry no unit and `units.length` scales no coordinate
    /// (plain polygons neither): curve points are metres like every other point.
    #[test]
    fn units_config_does_not_rescale_curve_points() {
        let ast = cst_to_ast::parse(&slab_source(SPEC_SLAB)).unwrap();
        let mut config = ProjectConfig::default();
        config.units.length = "mm".to_string();
        let mm = analyze_with_config(&ast, Some(&config)).0;
        let m = analyze(&ast).0;
        let (a, b) = (
            boundary_of(mm.elements().next().unwrap()),
            boundary_of(m.elements().next().unwrap()),
        );
        assert_eq!(a.len(), b.len());
        assert!(
            a.iter()
                .zip(&b)
                .all(|(p, q)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9)
        );
    }

    #[test]
    fn template_with_smooth_boundary() {
        let source = "\
Template: CurvedFloor
  Slab: SL-{f}
    boundary: [(0, 0), smooth (10, -2), (20, 0), (20, 15), smooth (10, 18), (0, 15)]
    thickness: 0.2 m

# Site: S
## Building: B
### Storey: Floor 1
  elevation: 0 m
  apply_template: CurvedFloor
  f: F1
### Storey: Floor 2
  elevation: 3.5 m
  apply_template: CurvedFloor
  f: F2
";
        let (model, diagnostics) = analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != diagnostics::Severity::Error),
            "{diagnostics:?}"
        );
        for id in ["SL-F1", "SL-F2"] {
            let slab = model.elements().find(|e| e.id == id).expect(id);
            assert!(boundary_of(slab).len() > 20, "{id}");
            assert!(slab.curve("boundary").is_some(), "{id}");
        }
    }

    #[test]
    fn smooth_space_boundary_is_tessellated() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n#### Space: Hall\n  boundary: [(0, 0), smooth (10, -2), (20, 0), (20, 15), (0, 15)]\n";
        let (model, _) = analyze(&cst_to_ast::parse(source).unwrap());
        let space = model.spaces().next().unwrap();
        match &space
            .properties
            .iter()
            .find(|p| p.key == "boundary")
            .unwrap()
            .value
        {
            model::ResolvedValue::Polygon(pts) => assert!(pts.len() > 10),
            v => panic!("expected Polygon, got {v:?}"),
        }
        assert!(matches!(
            space.curve("boundary"),
            Some(model::ElementCurve::Region { .. })
        ));
    }

    #[test]
    fn smooth_holes_resolve_to_region() {
        let src = slab_source(
            "{ outer: [(0, 0), (20, 0), (20, 15), (0, 15)], holes: [[(5, 5), smooth (7, 4), (9, 5), (9, 9), (5, 9)]] }",
        );
        let (model, diagnostics) = analyze(&cst_to_ast::parse(&src).unwrap());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != diagnostics::Severity::Error),
            "{diagnostics:?}"
        );
        let slab = model.elements().next().unwrap();
        match &slab
            .properties
            .iter()
            .find(|p| p.key == "boundary")
            .unwrap()
            .value
        {
            model::ResolvedValue::PolygonWithHoles { outer, holes } => {
                assert_eq!(outer.len(), 4);
                assert!(holes[0].len() > 5);
            }
            v => panic!("expected PolygonWithHoles, got {v:?}"),
        }
        assert!(matches!(
            slab.curve("boundary"),
            Some(model::ElementCurve::Region { holes, .. }) if holes.len() == 1
        ));
    }

    #[test]
    fn smooth_3d_path_resolves_to_path3d() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Duct: SA-FLEX\n    diameter: 0.25 m\n    path: [(0, 0, 3), smooth (2, 1, 3), (4, 0, 3.2)]\n";
        // Before MEP expansion replaces the route by its parts
        let (model, _) = containment::build_model(&cst_to_ast::parse(source).unwrap(), 0.005);
        let duct = model.elements().find(|e| e.id == "SA-FLEX").unwrap();
        assert!(matches!(
            duct.curve("path"),
            Some(model::ElementCurve::Path3(_))
        ));
        match &duct
            .properties
            .iter()
            .find(|p| p.key == "path")
            .unwrap()
            .value
        {
            model::ResolvedValue::Path3D(pts) => assert!(pts.len() > 3),
            v => panic!("expected Path3D, got {v:?}"),
        }
    }

    #[test]
    fn bend_into_smooth_run_is_tangent() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Duct: SA\n    diameter: 0.25 m\n    path: [(0, 0, 3), (4, 0, 3), smooth (6, 2, 3), smooth (8, 2, 3), (10, 4, 3)]\n";
        let (model, diags) = analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags
                .iter()
                .all(|d| d.severity != diagnostics::Severity::Error),
            "{diags:?}"
        );
        let parts: Vec<_> = model
            .elements()
            .filter(|e| e.id.starts_with("SA-"))
            .collect();
        let bend = parts.iter().find(|e| e.id == "SA-B1").expect("bend");
        let pts = match &bend
            .properties
            .iter()
            .find(|p| p.key == "path")
            .unwrap()
            .value
        {
            model::ResolvedValue::Path3D(pts) => pts.clone(),
            v => panic!("expected Path3D, got {v:?}"),
        };
        let [_, v, p1] = pts[..] else {
            panic!("{pts:?}")
        };
        let run = parts
            .iter()
            .find_map(|e| match e.curve("path") {
                Some(model::ElementCurve::Path3(c)) => Some(c),
                _ => None,
            })
            .expect("smooth segment");
        let t = run.tangent_at(0.0);
        let d = [p1.0 - v.0, p1.1 - v.1, p1.2 - v.2];
        let cos = (d[0] * t[0] + d[1] * t[1] + d[2] * t[2])
            / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!(cos.clamp(-1.0, 1.0).acos().to_degrees() < 1.0, "{cos}");
    }

    fn curve_error(boundary: &str) -> Diagnostic {
        let (_, diagnostics) = analyze(&cst_to_ast::parse(&slab_source(boundary)).unwrap());
        let errors: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.severity == diagnostics::Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        let e = errors.into_iter().next().unwrap();
        assert!(
            e.message.contains("SL-L1"),
            "message must name the element: {}",
            e.message
        );
        e
    }

    #[test]
    fn repeated_smooth_point_is_an_error() {
        let e = curve_error("[(0, 0), smooth (5, 2), smooth (5, 2), (10, 0), (10, 10)]");
        assert_eq!(e.code, DiagnosticCode::E308);
    }

    #[test]
    fn too_few_curve_points_is_an_error() {
        let e = curve_error("[smooth (0, 0), smooth (5, 2)]");
        assert_eq!(e.code, DiagnosticCode::E308);
    }

    #[test]
    fn self_crossing_curve_outline_is_an_error() {
        let e = curve_error("[(0, 0), (10, 0), smooth (5, 12), smooth (-3, 4), (10, 6), (0, 6)]");
        assert_eq!(e.code, DiagnosticCode::E310);
    }

    #[test]
    fn bad_nurbs_knots_is_an_error() {
        let e = curve_error(
            "[(0, 10), nurbs { degree: 2, points: [(0, 0), (5, -3), (10, 0)], knots: [0, 0, 1, 1] }, (10, 10)]",
        );
        assert_eq!(e.code, DiagnosticCode::E309);
        assert!(e.message.contains("knots"), "{}", e.message);
    }

    #[test]
    fn bad_nurbs_weight_is_an_error() {
        let e = curve_error(
            "[(0, 10), nurbs { degree: 2, points: [(0, 0), (5, -3), (10, 0)], weights: [1, 0, 1] }, (10, 10)]",
        );
        assert_eq!(e.code, DiagnosticCode::E309);
    }

    #[test]
    fn fractional_nurbs_degree_is_an_error() {
        let e = curve_error(
            "[(0, 10), nurbs { degree: 1.5, points: [(0, 0), (5, -3), (10, 0)] }, (10, 10)]",
        );
        assert_eq!(e.code, DiagnosticCode::E309);
        assert!(e.message.contains("degree"), "{}", e.message);
    }

    #[test]
    fn zero_nurbs_degree_is_an_error() {
        let e = curve_error("[(0, 10), nurbs { degree: 0, points: [(0, 0), (10, 0)] }, (10, 10)]");
        assert_eq!(e.code, DiagnosticCode::E309);
    }

    /// A wall on `path:` with a door `offset` / `width` and wall `thickness`.
    fn spline_wall(thickness: &str, offset: f64, width: f64) -> String {
        format!(
            "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Wall: W-S\n    path: [(0, 0), smooth (4, 1.5), smooth (8, 1), (12, 3)]\n    height: 3 m\n    thickness: {thickness}\n    Door: D-1\n      offset: {offset} m\n      width: {width} m\n      height: 2.1 m\n"
        )
    }

    #[test]
    fn wall_path_replaces_from_to() {
        let (model, diagnostics) =
            analyze(&cst_to_ast::parse(&spline_wall("0.2 m", 3.0, 1.0)).unwrap());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != diagnostics::Severity::Error),
            "{diagnostics:?}"
        );
        let wall = model.elements().find(|e| e.id == "W-S").unwrap();
        let axis = crate::geometry::axis::WallAxis::of(wall, 0.005).unwrap();
        assert!(!axis.is_straight());
        assert!(axis.length() > 12.4, "{}", axis.length());
    }

    #[test]
    fn door_offset_past_curve_end_errors() {
        let overflow = |offset: f64| {
            let (model, diagnostics) =
                analyze(&cst_to_ast::parse(&spline_wall("0.2 m", offset, 1.0)).unwrap());
            let wall = model.elements().find(|e| e.id == "W-S").unwrap();
            let len = crate::geometry::axis::WallAxis::of(wall, 0.005)
                .unwrap()
                .length();
            (
                len,
                diagnostics.iter().any(|d| d.code == DiagnosticCode::E304),
            )
        };
        let (len, _) = overflow(0.0);
        // Past the 12.37 m chord but inside the curve: fits.
        assert!(!overflow(len - 1.05).1);
        assert!(overflow(len - 0.9).1);
    }

    #[test]
    fn wall_radius_too_small_errors() {
        let source = |t: &str| {
            format!(
                "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Wall: W-T\n    path: [(0, 0), smooth (0.3, 0.5), (0.6, 0)]\n    height: 3 m\n    thickness: {t}\n"
            )
        };
        let (model, diagnostics) = analyze(&cst_to_ast::parse(&source("0.5 m")).unwrap());
        let wall = model.elements().find(|e| e.id == "W-T").unwrap();
        let r = crate::geometry::axis::WallAxis::of(wall, 0.005)
            .unwrap()
            .min_radius();
        let e = diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::E311)
            .unwrap_or_else(|| panic!("{diagnostics:?}"));
        assert_eq!(e.severity, diagnostics::Severity::Error);
        assert!(
            e.message.contains("W-T") && e.message.contains(&format!("radius {r:.3} m")),
            "{}",
            e.message
        );
        let (_, diagnostics) = analyze(&cst_to_ast::parse(&source("0.05 m")).unwrap());
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::E311),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn path_and_from_both_given_is_an_error() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Wall: W-B\n    path: [(0, 0), (5, 0)]\n    from: (0, 0) to: (5, 0)\n    height: 3 m\n";
        let (_, diagnostics) = analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E312
                    && d.severity == diagnostics::Severity::Error),
            "{diagnostics:?}"
        );
    }
}
