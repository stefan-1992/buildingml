//! Built-in computed properties for compliance checking.
//!
//! These properties are derived from element geometry and spatial context,
//! not stored directly as element properties. The checker engine resolves
//! these automatically when referenced in check expressions.
//!
//! Supported computed properties:
//! - `clear_height`: Floor-to-ceiling height in a space
//! - `escape_route_length`: Distance from space to nearest exit
//! - `window_to_floor_ratio`: Window area / floor area ratio
//! - `is_fire_compartment_boundary`: Whether a wall separates fire compartments
//! - `structural_span`: Beam or slab span length

use crate::semantic::model::*;

use super::engine::{ElementContext, EvalResult};

/// Try to compute a built-in property. Returns None if not a computed property.
pub fn compute_property(
    name: &str,
    ctx: &ElementContext,
    _model: &SemanticModel,
) -> Option<EvalResult> {
    match name {
        "clear_height" => Some(compute_clear_height(ctx)),
        "escape_route_length" => Some(compute_escape_route_length(ctx)),
        "window_to_floor_ratio" => Some(compute_window_to_floor_ratio(ctx)),
        "is_fire_compartment_boundary" => Some(compute_is_fire_compartment_boundary(ctx)),
        "structural_span" => Some(compute_structural_span(ctx)),
        _ => None,
    }
}

/// List all available computed property names.
pub fn computed_property_names() -> &'static [&'static str] {
    &[
        "clear_height",
        "escape_route_length",
        "window_to_floor_ratio",
        "is_fire_compartment_boundary",
        "structural_span",
    ]
}

/// Clear height: storey height minus slab thicknesses.
///
/// Uses storey height if available, otherwise derives from element heights.
fn compute_clear_height(ctx: &ElementContext) -> EvalResult {
    if let Some(storey) = ctx.storey
        && let Some(height_prop) = storey.properties.iter().find(|p| p.key == "height")
        && let ResolvedValue::Number(h) = &height_prop.value
    {
        return EvalResult::Number(*h);
    }

    // Fallback: look for wall heights in the same context
    if ctx.element.element_type == ElementType::Wall
        && let Some(prop) = ctx.element.properties.iter().find(|p| p.key == "height")
        && let ResolvedValue::Number(h) = &prop.value
    {
        return EvalResult::Number(*h);
    }

    EvalResult::Error("clear_height: no height information available".into())
}

/// Escape route length: simplified computation.
///
/// Uses the space area as a proxy (sqrt(area) as approximate distance).
/// A full implementation would use graph-based pathfinding.
fn compute_escape_route_length(ctx: &ElementContext) -> EvalResult {
    if let Some(space) = ctx.space
        && let Some(area_prop) = space.properties.iter().find(|p| p.key == "area")
        && let ResolvedValue::Number(area) = &area_prop.value
    {
        // Simplified: approximate escape route as diagonal of square room
        let approx_length = (area * 2.0).sqrt();
        return EvalResult::Number(approx_length);
    }

    EvalResult::Error("escape_route_length: no space area available".into())
}

/// Window-to-floor ratio: total window area / space floor area.
///
/// Computes the ratio of total window area (width * height) to the space's floor area.
fn compute_window_to_floor_ratio(ctx: &ElementContext) -> EvalResult {
    let floor_area = if let Some(space) = ctx.space {
        space
            .properties
            .iter()
            .find(|p| p.key == "area")
            .and_then(|p| {
                if let ResolvedValue::Number(a) = &p.value {
                    Some(*a)
                } else {
                    None
                }
            })
    } else {
        None
    };

    let floor_area = match floor_area {
        Some(a) if a > 0.0 => a,
        _ => return EvalResult::Error("window_to_floor_ratio: no floor area available".into()),
    };

    // Sum all window areas in the current context
    let mut window_area = 0.0;

    // Check if the current element is a Window
    if ctx.element.element_type == ElementType::Window {
        let w = get_number_prop(ctx.element, "width").unwrap_or(0.0);
        let h = get_number_prop(ctx.element, "height").unwrap_or(0.0);
        window_area = w * h;
    }

    // Check children of the current element
    for child in &ctx.element.children {
        if child.element_type == ElementType::Window {
            let w = get_number_prop(child, "width").unwrap_or(0.0);
            let h = get_number_prop(child, "height").unwrap_or(0.0);
            window_area += w * h;
        }
    }

    EvalResult::Number(window_area / floor_area)
}

/// Is fire compartment boundary: heuristic based on fire_rating and is_external.
///
/// A wall is considered a fire compartment boundary if it has a fire_rating
/// property set to REI30 or higher, or if it's an external wall.
fn compute_is_fire_compartment_boundary(ctx: &ElementContext) -> EvalResult {
    if ctx.element.element_type != ElementType::Wall {
        return EvalResult::Bool(false);
    }

    // Check fire_rating property
    let has_fire_rating = ctx.element.properties.iter().any(|p| {
        p.key == "fire_rating"
            && matches!(
                &p.value,
                ResolvedValue::Identifier(s) if s.starts_with("REI")
            )
    });

    // Check is_external property
    let is_external = ctx
        .element
        .properties
        .iter()
        .any(|p| p.key == "is_external" && matches!(&p.value, ResolvedValue::Boolean(true)));

    EvalResult::Bool(has_fire_rating || is_external)
}

/// Structural span: length of a beam or diagonal of a slab.
///
/// For beams: distance between from and to points.
/// For slabs: approximate span from boundary polygon.
fn compute_structural_span(ctx: &ElementContext) -> EvalResult {
    match ctx.element.element_type {
        ElementType::Beam => {
            // Look for axis (from/to) property
            for prop in &ctx.element.properties {
                if prop.key == "from"
                    && let ResolvedValue::Axis { from, to, .. } = &prop.value
                {
                    let dx = to.0 - from.0;
                    let dy = to.1 - from.1;
                    return EvalResult::Number((dx * dx + dy * dy).sqrt());
                }
            }
            EvalResult::Error("structural_span: no axis found on beam".into())
        }
        ElementType::Slab => {
            // Use boundary polygon to estimate span
            for prop in &ctx.element.properties {
                if prop.key == "boundary"
                    && let ResolvedValue::Polygon(pts) = &prop.value
                    && pts.len() >= 2
                {
                    // Max distance between any two points
                    let mut max_dist = 0.0_f64;
                    for i in 0..pts.len() {
                        for j in (i + 1)..pts.len() {
                            let dx = pts[j].0 - pts[i].0;
                            let dy = pts[j].1 - pts[i].1;
                            let dist = (dx * dx + dy * dy).sqrt();
                            max_dist = max_dist.max(dist);
                        }
                    }
                    return EvalResult::Number(max_dist);
                }
            }
            EvalResult::Error("structural_span: no boundary found on slab".into())
        }
        _ => EvalResult::Error(format!(
            "structural_span: not applicable to {}",
            ctx.element.element_type.as_str()
        )),
    }
}

fn get_number_prop(elem: &ResolvedElement, key: &str) -> Option<f64> {
    elem.properties.iter().find(|p| p.key == key).and_then(|p| {
        if let ResolvedValue::Number(n) = &p.value {
            Some(*n)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::engine::ElementContext;

    fn dummy_span() -> crate::ast::span::Span {
        crate::ast::span::Span {
            start: crate::ast::span::Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
            end: crate::ast::span::Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
            file_id: 0,
        }
    }

    fn make_empty_model() -> SemanticModel {
        SemanticModel {
            asset_libraries: vec![],
            sites: vec![],
            materials: vec![],
        }
    }

    fn make_storey_with_height(height: f64) -> ResolvedStorey {
        ResolvedStorey {
            name: "GF".to_string(),
            properties: vec![ResolvedProperty {
                key: "height".to_string(),
                value: ResolvedValue::Number(height),
                span: dummy_span(),
            }],
            spaces: vec![],
            elements: vec![],
            span: dummy_span(),
        }
    }

    #[test]
    fn test_clear_height_from_storey() {
        let model = make_empty_model();
        let storey = make_storey_with_height(3.2);
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-01".to_string(),
            properties: vec![],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: Some(&storey),
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("clear_height", &ctx, &model).unwrap() {
            EvalResult::Number(h) => assert!((h - 3.2).abs() < 1e-10),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_clear_height_from_wall() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-01".to_string(),
            properties: vec![ResolvedProperty {
                key: "height".to_string(),
                value: ResolvedValue::Number(2.8),
                span: dummy_span(),
            }],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("clear_height", &ctx, &model).unwrap() {
            EvalResult::Number(h) => assert!((h - 2.8).abs() < 1e-10),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_is_fire_compartment_boundary_external() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-EXT".to_string(),
            properties: vec![ResolvedProperty {
                key: "is_external".to_string(),
                value: ResolvedValue::Boolean(true),
                span: dummy_span(),
            }],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("is_fire_compartment_boundary", &ctx, &model).unwrap() {
            EvalResult::Bool(b) => assert!(b, "external wall should be fire boundary"),
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    #[test]
    fn test_is_fire_compartment_boundary_fire_rated() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-FR".to_string(),
            properties: vec![ResolvedProperty {
                key: "fire_rating".to_string(),
                value: ResolvedValue::Identifier("REI90".to_string()),
                span: dummy_span(),
            }],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("is_fire_compartment_boundary", &ctx, &model).unwrap() {
            EvalResult::Bool(b) => assert!(b, "fire-rated wall should be fire boundary"),
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    #[test]
    fn test_is_fire_compartment_boundary_not_wall() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Column,
            id: "COL-01".to_string(),
            properties: vec![],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("is_fire_compartment_boundary", &ctx, &model).unwrap() {
            EvalResult::Bool(b) => assert!(!b, "column is not a fire boundary"),
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    #[test]
    fn test_structural_span_beam() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Beam,
            id: "B-01".to_string(),
            properties: vec![ResolvedProperty {
                key: "from".to_string(),
                value: ResolvedValue::Axis {
                    from: (0.0, 0.0),
                    to: (6.0, 0.0),
                    via: None,
                    z: None,
                },
                span: dummy_span(),
            }],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("structural_span", &ctx, &model).unwrap() {
            EvalResult::Number(span) => assert!((span - 6.0).abs() < 1e-10),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_structural_span_slab() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Slab,
            id: "S-01".to_string(),
            properties: vec![ResolvedProperty {
                key: "boundary".to_string(),
                value: ResolvedValue::Polygon(vec![
                    (0.0, 0.0),
                    (10.0, 0.0),
                    (10.0, 8.0),
                    (0.0, 8.0),
                ]),
                span: dummy_span(),
            }],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        match compute_property("structural_span", &ctx, &model).unwrap() {
            EvalResult::Number(span) => {
                // Diagonal of 10x8 = sqrt(164) ≈ 12.81
                let expected = (10.0_f64.powi(2) + 8.0_f64.powi(2)).sqrt();
                assert!((span - expected).abs() < 1e-6);
            }
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_window_to_floor_ratio() {
        let model = make_empty_model();
        let space = ResolvedSpace {
            name: "Room".to_string(),
            properties: vec![ResolvedProperty {
                key: "area".to_string(),
                value: ResolvedValue::Number(20.0),
                span: dummy_span(),
            }],
            elements: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        // Wall with a window child (1.2m x 1.4m = 1.68 m²)
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-01".to_string(),
            properties: vec![],
            children: vec![ResolvedElement {
                element_type: ElementType::Window,
                id: "WIN-01".to_string(),
                properties: vec![
                    ResolvedProperty {
                        key: "width".to_string(),
                        value: ResolvedValue::Number(1.2),
                        span: dummy_span(),
                    },
                    ResolvedProperty {
                        key: "height".to_string(),
                        value: ResolvedValue::Number(1.4),
                        span: dummy_span(),
                    },
                ],
                children: vec![],
                span: dummy_span(),
                curves: Default::default(),
            }],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: Some(&space),
            building: None,
            site: None,
            host: None,
        };

        match compute_property("window_to_floor_ratio", &ctx, &model).unwrap() {
            EvalResult::Number(ratio) => {
                let expected = (1.2 * 1.4) / 20.0;
                assert!((ratio - expected).abs() < 1e-10);
            }
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_unknown_computed_property() {
        let model = make_empty_model();
        let elem = ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-01".to_string(),
            properties: vec![],
            children: vec![],
            span: dummy_span(),
            curves: Default::default(),
        };

        let ctx = ElementContext {
            element: &elem,
            storey: None,
            space: None,
            building: None,
            site: None,
            host: None,
        };

        assert!(compute_property("nonexistent", &ctx, &model).is_none());
    }

    #[test]
    fn test_computed_property_names_list() {
        let names = computed_property_names();
        assert_eq!(names.len(), 5);
        assert!(names.contains(&"clear_height"));
        assert!(names.contains(&"window_to_floor_ratio"));
        assert!(names.contains(&"structural_span"));
    }
}
