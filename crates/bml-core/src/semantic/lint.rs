use crate::config::{LintConfig, LintLevel};

use super::diagnostics::{Diagnostic, DiagnosticCode, RelatedInformation, Severity};
use super::model::*;

/// Run all lint checks on the semantic model.
pub fn run_lint_checks(model: &SemanticModel, config: &LintConfig) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for c in model.containers() {
        if let Some(storey) = c.storey
            && config.storey_height != LintLevel::Off
        {
            check_storey_height(storey, config.storey_height, &mut diagnostics);
        }
        if config.wall_overlap != LintLevel::Off {
            check_wall_overlap(c.spaces, c.elements, config.wall_overlap, &mut diagnostics);
        }
        if config.space_door != LintLevel::Off {
            for space in c.spaces {
                check_space_has_door(space, config.space_door, &mut diagnostics);
            }
        }
    }
    for elem in model.elements() {
        lint_element(elem, config, &mut diagnostics);
    }

    diagnostics
}

fn lint_severity(level: LintLevel) -> Severity {
    match level {
        LintLevel::Warn => Severity::Warning,
        LintLevel::Error => Severity::Error,
        LintLevel::Off => Severity::Warning, // shouldn't be called when Off
    }
}

fn lint_element(elem: &ResolvedElement, config: &LintConfig, diagnostics: &mut Vec<Diagnostic>) {
    match elem.element_type {
        // A bodiless wall (e.g. a curtain wall) has no thickness of its own.
        ElementType::Wall if !crate::codegen::ifc_writer::no_body(elem) => {
            if config.wall_thickness != LintLevel::Off {
                check_wall_thickness(elem, config.wall_thickness, diagnostics);
            }
        }
        ElementType::Door => {
            if config.door_width != LintLevel::Off {
                check_door_width(elem, config.door_width, diagnostics);
            }
        }
        ElementType::Window => {
            if config.window_sill != LintLevel::Off {
                check_window_sill(elem, config.window_sill, diagnostics);
            }
            if config.door_width != LintLevel::Off {
                check_window_width(elem, config.door_width, diagnostics);
            }
        }
        ElementType::Column => {
            if config.column_aspect != LintLevel::Off {
                check_column_aspect(elem, config.column_aspect, diagnostics);
            }
        }
        ElementType::Beam if config.beam_span != LintLevel::Off => {
            check_beam_span(elem, config.beam_span, diagnostics);
        }
        _ => {}
    }

    // Recurse into children (for door/window inside wall)
    for child in &elem.children {
        lint_element(child, config, diagnostics);
    }
}

fn get_number_prop(elem: &ResolvedElement, key: &str) -> Option<(f64, crate::ast::span::Span)> {
    elem.properties.iter().find_map(|p| {
        if p.key == key
            && let ResolvedValue::Number(n) = &p.value
        {
            Some((*n, p.span))
        } else {
            None
        }
    })
}

/// Wall thickness: warn if > 1.0m or < 0.05m
fn check_wall_thickness(
    elem: &ResolvedElement,
    level: LintLevel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some((thickness, span)) = get_number_prop(elem, "thickness") {
        if thickness > 1.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E401,
                severity: lint_severity(level),
                message: format!(
                    "Wall `{}` has unusually thick walls ({:.2}m > 1.0m)",
                    elem.id, thickness
                ),
                span,
                help: Some("typical wall thickness is 0.10m–0.50m".to_string()),
                file: None,
                related: Vec::new(),
            });
        } else if thickness < 0.05 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E401,
                severity: lint_severity(level),
                message: format!(
                    "Wall `{}` has unusually thin walls ({:.3}m < 0.05m)",
                    elem.id, thickness
                ),
                span,
                help: Some("typical wall thickness is 0.10m–0.50m".to_string()),
                file: None,
                related: Vec::new(),
            });
        }
    }
}

/// Door width: warn if > 3.0m or < 0.5m
fn check_door_width(elem: &ResolvedElement, level: LintLevel, diagnostics: &mut Vec<Diagnostic>) {
    if let Some((width, span)) = get_number_prop(elem, "width") {
        if width > 3.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E402,
                severity: lint_severity(level),
                message: format!(
                    "Door `{}` has unusually wide opening ({:.2}m > 3.0m)",
                    elem.id, width
                ),
                span,
                help: Some("typical door width is 0.70m–2.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        } else if width < 0.5 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E402,
                severity: lint_severity(level),
                message: format!(
                    "Door `{}` has unusually narrow opening ({:.2}m < 0.5m)",
                    elem.id, width
                ),
                span,
                help: Some("typical door width is 0.70m–2.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        }
    }
}

/// Window sill: warn if > 2.0m
fn check_window_sill(elem: &ResolvedElement, level: LintLevel, diagnostics: &mut Vec<Diagnostic>) {
    if let Some((sill, span)) = get_number_prop(elem, "sill_height")
        && sill > 2.0
    {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::E402,
            severity: lint_severity(level),
            message: format!(
                "Window `{}` has unusually high sill ({:.2}m > 2.0m)",
                elem.id, sill
            ),
            span,
            help: Some("typical sill height is 0.5m–1.2m".to_string()),
            file: None,
            related: Vec::new(),
        });
    }
}

/// Window width: warn if > 5.0m or < 0.2m
fn check_window_width(elem: &ResolvedElement, level: LintLevel, diagnostics: &mut Vec<Diagnostic>) {
    if let Some((width, span)) = get_number_prop(elem, "width") {
        if width > 5.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E402,
                severity: lint_severity(level),
                message: format!(
                    "Window `{}` has unusually wide opening ({:.2}m > 5.0m)",
                    elem.id, width
                ),
                span,
                help: Some("typical window width is 0.5m–3.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        } else if width < 0.2 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E402,
                severity: lint_severity(level),
                message: format!(
                    "Window `{}` has unusually narrow opening ({:.2}m < 0.2m)",
                    elem.id, width
                ),
                span,
                help: Some("typical window width is 0.5m–3.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        }
    }
}

/// Storey height: warn if > 10m or < 2.0m
fn check_storey_height(
    storey: &ResolvedStorey,
    level: LintLevel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Skip height check for dedicated roof-only storeys (all elements across all spaces are Roofs).
    let roof_only_storey = {
        let mut elems = storey
            .elements
            .iter()
            .chain(storey.spaces.iter().flat_map(|s| s.elements.iter()));
        let has_any = elems.clone().count() > 0;
        has_any && elems.all(|e| e.element_type == ElementType::Roof)
    };
    if roof_only_storey {
        return;
    }

    if let Some(height_prop) = storey.properties.iter().find(|p| p.key == "height")
        && let ResolvedValue::Number(height) = &height_prop.value
    {
        if *height > 10.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E403,
                severity: lint_severity(level),
                message: format!(
                    "Storey `{}` has unusually high ceiling ({:.1}m > 10m)",
                    storey.name, height
                ),
                span: height_prop.span,
                help: Some("typical storey height is 2.5m–5.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        } else if *height < 2.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E403,
                severity: lint_severity(level),
                message: format!(
                    "Storey `{}` has unusually low ceiling ({:.1}m < 2.0m)",
                    storey.name, height
                ),
                span: height_prop.span,
                help: Some("typical storey height is 2.5m–5.0m".to_string()),
                file: None,
                related: Vec::new(),
            });
        }
    }
}

/// Column aspect ratio: warn if width/depth > 5:1
fn check_column_aspect(
    elem: &ResolvedElement,
    level: LintLevel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Column profile is a rectangle(w, d) or circle(r)
    if let Some(profile_prop) = elem.properties.iter().find(|p| p.key == "profile")
        && let ResolvedValue::Profile {
            profile_type,
            params,
        } = &profile_prop.value
        && profile_type == "rectangle"
        && params.len() >= 2
    {
        let w = params[0];
        let d = params[1];
        if w > 0.0 && d > 0.0 {
            let ratio = w.max(d) / w.min(d);
            if ratio > 5.0 {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::E404,
                    severity: lint_severity(level),
                    message: format!(
                        "Column `{}` has extreme aspect ratio ({:.1}:1, profile {:.3}m x {:.3}m)",
                        elem.id, ratio, w, d
                    ),
                    span: profile_prop.span,
                    help: Some("typical column aspect ratio is < 3:1".to_string()),
                    file: None,
                    related: Vec::new(),
                });
            }
        }
    }
}

/// Space has no door: warn if a walled space has no Door children anywhere in its walls.
///
/// Skips spaces with no Wall elements (e.g. roof-only attic spaces) since a door
/// requires a wall to be placed on.
fn check_space_has_door(
    space: &ResolvedSpace,
    level: LintLevel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let has_walls = space
        .elements
        .iter()
        .any(|e| e.element_type == ElementType::Wall);
    if !has_walls {
        return;
    }

    // A curtain wall's doors are parts next to it, not children.
    let has_door = space.elements.iter().any(|e| {
        e.element_type == ElementType::Door
            || e.element_type == ElementType::Wall
                && e.children
                    .iter()
                    .any(|c| c.element_type == ElementType::Door)
    });

    if !has_door {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::E406,
            severity: lint_severity(level),
            message: format!("Space `{}` has no door", space.name),
            span: space.span,
            help: Some("add a Door to one of the walls in this space to allow access".to_string()),
            file: None,
            related: Vec::new(),
        });
    }
}

/// Beam span: warn if > 20m
fn check_beam_span(elem: &ResolvedElement, level: LintLevel, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(axis_prop) = elem.properties.iter().find(|p| p.key == "from")
        && let ResolvedValue::Axis { from, to, .. } = &axis_prop.value
    {
        let dx = to.0 - from.0;
        let dy = to.1 - from.1;
        let span_length = (dx * dx + dy * dy).sqrt();

        if span_length > 20.0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E405,
                severity: lint_severity(level),
                message: format!(
                    "Beam `{}` has unusually long span ({:.1}m > 20m)",
                    elem.id, span_length
                ),
                span: axis_prop.span,
                help: Some("consider adding intermediate supports for spans > 20m".to_string()),
                file: None,
                related: Vec::new(),
            });
        }
    }
}

/// Distance tolerance (m) for collinearity and minimum overlap length.
const WALL_OVERLAP_TOL: f64 = 0.01;

/// Straight wall axis segment with its thickness, used by `check_wall_overlap`.
struct WallSegment<'a> {
    elem: &'a ResolvedElement,
    axis_span: crate::ast::span::Span,
    from: (f64, f64),
    to: (f64, f64),
    thickness: f64,
}

/// Coincident walls: warn when two walls on the same storey overlap.
///
/// Two straight walls overlap when their axes are parallel, their projected
/// lengths overlap by more than `WALL_OVERLAP_TOL`, and the perpendicular
/// offset between them is smaller than half their combined thickness (or
/// `WALL_OVERLAP_TOL` for collinear axes). Direction does not matter, so a
/// party wall declared once from each neighbouring space is reported.
/// Curved walls are skipped. The warning goes on the later wall.
fn check_wall_overlap(
    spaces: &[ResolvedSpace],
    elements: &[ResolvedElement],
    level: LintLevel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let walls: Vec<WallSegment> = contents(spaces, elements)
        .filter(|e| e.element_type == ElementType::Wall && !crate::codegen::ifc_writer::no_body(e))
        .filter_map(|e| {
            let prop = e
                .properties
                .iter()
                .find(|p| p.key == "path" || p.key == "from")?;
            let [from, to] = crate::geometry::axis::WallAxis::straight_ends(
                e,
                crate::config::default_curve_tolerance(),
            )?;
            Some(WallSegment {
                elem: e,
                axis_span: prop.span,
                from: (from[0], from[1]),
                to: (to[0], to[1]),
                thickness: get_number_prop(e, "thickness").map_or(0.0, |(t, _)| t),
            })
        })
        .collect();

    for (j, b) in walls.iter().enumerate() {
        for a in &walls[..j] {
            let Some((overlap, offset)) = wall_overlap(a, b) else {
                continue;
            };
            let message = if offset <= WALL_OVERLAP_TOL {
                format!(
                    "Wall `{}` coincides with wall `{}` over {:.2}m",
                    b.elem.id, a.elem.id, overlap
                )
            } else {
                format!(
                    "Wall `{}` overlaps wall `{}` over {:.2}m (axes {:.2}m apart, combined half-thickness {:.2}m)",
                    b.elem.id,
                    a.elem.id,
                    overlap,
                    offset,
                    (a.thickness + b.thickness) / 2.0
                )
            };
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::E407,
                severity: lint_severity(level),
                message,
                span: b.axis_span,
                help: Some(
                    "a shared wall between two spaces should be declared only once".to_string(),
                ),
                file: None,
                related: vec![RelatedInformation {
                    span: a.elem.span,
                    message: format!("wall `{}` declared here", a.elem.id),
                }],
            });
        }
    }
}

/// Returns `(overlap_length, perpendicular_offset)` if walls `a` and `b` overlap.
fn wall_overlap(a: &WallSegment, b: &WallSegment) -> Option<(f64, f64)> {
    let (dx, dy) = (a.to.0 - a.from.0, a.to.1 - a.from.1);
    let len_a = (dx * dx + dy * dy).sqrt();
    let len_b = ((b.to.0 - b.from.0).powi(2) + (b.to.1 - b.from.1).powi(2)).sqrt();
    if len_a < WALL_OVERLAP_TOL || len_b < WALL_OVERLAP_TOL {
        return None;
    }
    let (ux, uy) = (dx / len_a, dy / len_a);

    // Position of a point along a's axis (t) and signed distance from it (n).
    let local = |p: (f64, f64)| {
        let (px, py) = (p.0 - a.from.0, p.1 - a.from.1);
        (px * ux + py * uy, px * -uy + py * ux)
    };
    let (t1, n1) = local(b.from);
    let (t2, n2) = local(b.to);

    // Parallel: both endpoints of b at (nearly) the same offset from a's axis.
    if (n1 - n2).abs() > WALL_OVERLAP_TOL {
        return None;
    }
    let offset = ((n1 + n2) / 2.0).abs();
    let max_offset = ((a.thickness + b.thickness) / 2.0 - WALL_OVERLAP_TOL).max(WALL_OVERLAP_TOL);
    if offset > max_offset {
        return None;
    }

    let overlap = t1.max(t2).min(len_a) - t1.min(t2).max(0.0);
    (overlap > WALL_OVERLAP_TOL).then_some((overlap, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    #[test]
    fn test_wall_thickness_too_thick() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 1.5 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E401);
        assert!(lint.is_some(), "expected wall thickness lint warning");
    }

    #[test]
    fn test_wall_thickness_too_thin() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.02 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E401);
        assert!(lint.is_some(), "expected wall thickness lint warning");
    }

    #[test]
    fn test_wall_thickness_normal() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E401);
        assert!(lint.is_none(), "should not lint normal wall thickness");
    }

    #[test]
    fn test_lint_config_off() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 1.5 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let config = crate::config::ProjectConfig {
            lint: Some(LintConfig {
                wall_thickness: LintLevel::Off,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, diagnostics) = semantic::analyze_with_config(&ast, Some(&config));

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E401);
        assert!(lint.is_none(), "should not lint when wall_thickness is off");
    }

    #[test]
    fn test_door_width_too_wide() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 3 m
    thickness: 0.24 m
    Door: D1
      offset: 1 m
      width: 4.0 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E402);
        assert!(lint.is_some(), "expected door width lint warning");
    }

    #[test]
    fn test_storey_height_too_high() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 15 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E403);
        assert!(lint.is_some(), "expected storey height lint warning");
    }

    #[test]
    fn test_column_aspect_ratio() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: C1
    position: (5, 5)
    profile: rectangle(0.1, 1.0)
    height: 3 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E404);
        assert!(lint.is_some(), "expected column aspect ratio lint warning");
    }

    #[test]
    fn test_beam_span_too_long() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Beam: B1
    from: (0, 0) to: (25, 0)
    profile: rectangle(0.3, 0.5)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);

        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E405);
        assert!(lint.is_some(), "expected beam span lint warning");
    }

    #[test]
    fn test_space_no_door() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
#### Space: LivingRoom
  usage: living
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E406);
        assert!(lint.is_some(), "expected space-no-door warning");
        assert!(lint.unwrap().message.contains("LivingRoom"));
    }

    #[test]
    fn test_space_with_door_ok() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
#### Space: LivingRoom
  usage: living
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
    Door: D1
      offset: 1.5 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E406);
        assert!(lint.is_none(), "space with door should not warn");
    }

    #[test]
    fn test_space_door_in_curtain_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
#### Space: Lobby
  usage: lobby
  Wall: CW
    from: (0, 0) to: (3, 0)
    height: 3 m
    thickness: 0.03 m
    mullions: 1.5 m
    Door: CW-D
      offset: 0 m
      width: 1.5 m
      height: 2.2 m
";
        let (_, diagnostics) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(!diagnostics.iter().any(|d| d.code == DiagnosticCode::E406));
    }

    #[test]
    fn test_space_no_walls_skipped() {
        // Roof-only space (attic) should not trigger the warning
        let source = "\
# Site: S
## Building: B
### Storey: RoofLevel
  elevation: 3 m
  height: 0 m
#### Space: AtticSpace
  usage: storage
  Roof: RF-01
    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]
    type: gable
    ridge_height: 2.0 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E406);
        assert!(
            lint.is_none(),
            "roof-only space should not warn about missing door"
        );
    }

    #[test]
    fn test_space_door_lint_off() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
#### Space: LivingRoom
  usage: living
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let config = crate::config::ProjectConfig {
            lint: Some(LintConfig {
                space_door: LintLevel::Off,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, diagnostics) = semantic::analyze_with_config(&ast, Some(&config));
        let lint = diagnostics.iter().find(|d| d.code == DiagnosticCode::E406);
        assert!(lint.is_none(), "should not warn when space_door is off");
    }

    #[test]
    fn test_lint_config_from_toml() {
        let toml = r#"
[lint]
wall_thickness = "warn"
door_width = "error"
window_sill = "off"
storey_height = "warn"
column_aspect = "warn"
beam_span = "off"
wall_overlap = "error"
"#;
        let config = crate::config::ProjectConfig::parse(toml).unwrap();
        let lint = config.lint.unwrap();
        assert_eq!(lint.wall_thickness, LintLevel::Warn);
        assert_eq!(lint.door_width, LintLevel::Error);
        assert_eq!(lint.window_sill, LintLevel::Off);
        assert_eq!(lint.beam_span, LintLevel::Off);
        assert_eq!(lint.wall_overlap, LintLevel::Error);
    }

    fn wall_overlap_diags(walls: &str) -> Vec<Diagnostic> {
        let source = format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  height: 3 m\n{walls}"
        );
        let ast = cst_to_ast::parse(&source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::E407)
            .collect()
    }

    fn wall(id: &str, from: (f64, f64), to: (f64, f64), thickness: f64) -> String {
        format!(
            "  Wall: {id}\n    from: ({}, {}) to: ({}, {})\n    height: 3 m\n    thickness: {thickness} m\n",
            from.0, from.1, to.0, to.1
        )
    }

    #[test]
    fn test_wall_overlap_reversed_party_wall() {
        // From six_storey_residential.bml: W-{f}-{n}01-E and W-{f}-{n}02-W
        let walls = wall("W-101-E", (12.0, 0.0), (12.0, 8.0), 0.2)
            + &wall("W-102-W", (12.0, 8.0), (12.0, 0.0), 0.2);
        let diags = wall_overlap_diags(&walls);
        assert_eq!(diags.len(), 1, "expected one overlap warning: {diags:?}");
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert!(d.message.contains("`W-102-W`") && d.message.contains("`W-101-E`"));
        assert!(d.message.contains("8.00m"));
        assert_eq!(d.related.len(), 1);
        assert!(d.related[0].message.contains("W-101-E"));
        // Warning on the second wall, related info on the first
        assert!(d.span.start.line > d.related[0].span.start.line);
    }

    #[test]
    fn test_wall_overlap_partial_same_direction() {
        let walls =
            wall("W1", (0.0, 0.0), (10.0, 0.0), 0.2) + &wall("W2", (7.0, 0.0), (12.0, 0.0), 0.2);
        let diags = wall_overlap_diags(&walls);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("3.00m"));
    }

    #[test]
    fn test_wall_overlap_across_spaces() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
#### Space: A
  Wall: W-A-E
    from: (12, 0) to: (12, 8)
    height: 3 m
    thickness: 0.2 m
#### Space: B
  Wall: W-B-W
    from: (12, 8) to: (12, 0)
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        assert!(diagnostics.iter().any(|d| d.code == DiagnosticCode::E407));
    }

    #[test]
    fn test_wall_overlap_thickness_band() {
        // Parallel, 0.15m apart, combined half-thickness 0.25m
        let walls =
            wall("W1", (0.0, 0.0), (6.0, 0.0), 0.3) + &wall("W2", (1.0, 0.15), (5.0, 0.15), 0.2);
        let diags = wall_overlap_diags(&walls);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("overlaps"));
    }

    #[test]
    fn test_wall_overlap_parallel_touching_faces_ok() {
        // Faces touch exactly (0.25m apart, combined half-thickness 0.25m)
        let walls =
            wall("W1", (0.0, 0.0), (6.0, 0.0), 0.3) + &wall("W2", (0.0, 0.25), (6.0, 0.25), 0.2);
        assert!(wall_overlap_diags(&walls).is_empty());
    }

    #[test]
    fn test_wall_overlap_l_junction_ok() {
        let walls =
            wall("W1", (0.0, 0.0), (6.0, 0.0), 0.3) + &wall("W2", (6.0, 0.0), (6.0, 4.0), 0.3);
        assert!(wall_overlap_diags(&walls).is_empty());
    }

    #[test]
    fn test_wall_overlap_t_junction_ok() {
        let walls =
            wall("W1", (0.0, 0.0), (10.0, 0.0), 0.3) + &wall("W2", (5.0, 0.0), (5.0, 4.0), 0.2);
        assert!(wall_overlap_diags(&walls).is_empty());
    }

    #[test]
    fn test_wall_overlap_collinear_end_to_end_ok() {
        // Collinear walls that only share an endpoint (different thickness)
        let walls =
            wall("W1", (0.0, 0.0), (5.0, 0.0), 0.3) + &wall("W2", (5.0, 0.0), (9.0, 0.0), 0.2);
        assert!(wall_overlap_diags(&walls).is_empty());
    }

    #[test]
    fn test_wall_overlap_different_storeys_ok() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Wall: W-GF
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.2 m
### Storey: F1
  elevation: 3 m
  height: 3 m
  Wall: W-F1
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        assert!(!diagnostics.iter().any(|d| d.code == DiagnosticCode::E407));
    }

    #[test]
    fn test_building_and_site_level_lints() {
        // Overlapping walls directly under the building, a site space without a door
        let source = "\
# Site: S
#### Space: Yard
  Wall: W-Yard
    from: (0, 0) to: (5, 0)
    height: 2 m
    thickness: 0.2 m
## Building: B
  Wall: W-B1
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.2 m
  Wall: W-B2
    from: (6, 0) to: (0, 0)
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (_, diagnostics) = semantic::analyze(&ast);
        let overlap: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E407)
            .collect();
        assert_eq!(overlap.len(), 1, "{diagnostics:?}");
        assert!(overlap[0].message.contains("`W-B2`"));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E406 && d.message.contains("Yard"))
        );
    }

    #[test]
    fn test_wall_overlap_lint_off() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Wall: W1
    from: (12, 0) to: (12, 8)
    height: 3 m
    thickness: 0.2 m
  Wall: W2
    from: (12, 8) to: (12, 0)
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let config = crate::config::ProjectConfig {
            lint: Some(LintConfig {
                wall_overlap: LintLevel::Off,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, diagnostics) = semantic::analyze_with_config(&ast, Some(&config));
        assert!(!diagnostics.iter().any(|d| d.code == DiagnosticCode::E407));
    }

    #[test]
    fn test_wall_overlap_fixtures_clean() {
        // Party/corridor walls used to be declared once per neighbouring space
        let fixtures = [
            include_str!("../../../../tests/fixtures/six_storey_residential.bml"),
            include_str!("../../../../tests/fixtures/six_storey_residential_initial.bml"),
            include_str!("../../../../tests/fixtures/two_storey_house.bml"),
            include_str!("../../../../tests/fixtures/two_storey_house_furnished.bml"),
            include_str!("../../../../tests/fixtures/template_edge_cases.bml"),
        ];
        for source in fixtures {
            let ast = cst_to_ast::parse(source).unwrap();
            let (_, diagnostics) = semantic::analyze(&ast);
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
    }
}
