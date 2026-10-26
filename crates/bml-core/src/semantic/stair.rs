//! Stairs with landings (`Specs_V0_3.md` §23): an `l_shaped` or `u_shaped`
//! stair, or a `straight` one with `landing_after:`, becomes a bodiless
//! `IfcStair` whose parts are generated next to it: two straight flights
//! (`IfcStairFlight`) and the landing slab between them. Other layouts are
//! written as explicit flights and landings with `part_of:` instead.

use std::collections::HashSet;

use super::covering::push_defaults;
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel, number,
    string_or_ident as ident,
};
use crate::codegen::geometry::{Flight, placed};
use crate::codegen::ifc_writer::no_body;
use crate::scene::graph::get_point;

/// Expand every stair with a landing. Returns the errors of stairs that
/// cannot be built; they stay single straight flights.
pub(super) fn resolve(model: &mut SemanticModel) -> Vec<Diagnostic> {
    super::parts::expand_all(model, has_landing, expand)
}

fn has_landing(e: &ResolvedElement) -> bool {
    e.element_type == ElementType::Stair
        && !no_body(e)
        && match ident(&e.properties, "type") {
            Some("l_shaped" | "u_shaped") => true,
            Some("straight") | None => e.properties.iter().any(|p| p.key == "landing_after"),
            _ => false,
        }
}

/// Turn `stair` into the bodiless whole and return its flights and landing.
fn expand(
    stair: &mut ResolvedElement,
    storey_height: f64,
    ids: &HashSet<String>,
) -> Result<Vec<ResolvedElement>, Box<Diagnostic>> {
    let span = stair.span;
    let error = |message: String| Box::new(Diagnostic::error(DiagnosticCode::E101, message, span));
    let p = &stair.properties;
    let s = stair.id.as_str();
    let kind = ident(p, "type").unwrap_or("straight");
    let Flight {
        width,
        risers,
        riser,
        tread,
        ..
    } = Flight::of(p, storey_height);
    let risers = risers as f64;
    let first = number(p, "landing_after").map_or((risers / 2.0).ceil(), f64::round);
    let landing = number(p, "landing_depth").unwrap_or(width);
    let gap = number(p, "gap").unwrap_or(0.1);
    if width <= 0.0 || riser <= 0.0 || tread <= 0.0 || landing <= 0.0 || gap < 0.0 {
        return Err(error(format!(
            "stair `{s}` needs a positive width, riser height, tread depth and landing depth"
        )));
    }
    if risers < 2.0 {
        return Err(error(format!(
            "stair `{s}` needs at least 2 risers for a landing"
        )));
    }
    if first < 1.0 || first >= risers {
        return Err(error(format!(
            "`landing_after` of stair `{s}` must be between 1 and {} risers",
            risers - 1.0
        )));
    }
    // `turn` is an enum, so type checking reports any other value.
    let left = ident(p, "turn") != Some("right");

    // Each flight ends with its top tread flush with the level it arrives
    // at, so the landing starts after the first flight's last tread.
    let a = first * tread;
    let w = width;
    let (deck, origin, turn) = match (kind, left) {
        ("l_shaped", true) => ((0.0, w, a, a + w), (0.0, a), 90.0),
        ("l_shaped", false) => ((0.0, w, a, a + w), (w, a + w), -90.0),
        ("u_shaped", true) => ((-(w + gap), w, a, a + landing), (-gap, a), 180.0),
        ("u_shaped", false) => (
            (0.0, 2.0 * w + gap, a, a + landing),
            (2.0 * w + gap, a),
            180.0,
        ),
        _ => ((0.0, w, a, a + landing), (0.0, a + landing), 0.0),
    };
    // Start corner, turn in degrees and risers of the two flights.
    let flights = [((0.0, 0.0), 0.0, first), (origin, turn, risers - first)];

    let position = get_point(p, "position").unwrap_or((0.0, 0.0));
    let rotation = number(p, "rotation").unwrap_or(0.0);
    let base = number(p, "z_offset").unwrap_or(0.0);
    // A flight above the landing is not solid down to its base, so generated
    // flights always get a waist: by default the landing thickness.
    let waist = number(p, "waist_thickness")
        .filter(|&t| t > 0.0)
        .unwrap_or(0.2);
    let to_plan = |pts: Vec<(f64, f64)>| placed(pts, position, rotation);
    let material = p.iter().find(|q| q.key == "material").cloned();
    let prop = |key: &str, value: ResolvedValue| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let name = |v: &str| ResolvedValue::Identifier(v.to_string());
    let num = ResolvedValue::Number;
    let part = |element_type, id: String, mut properties: Vec<ResolvedProperty>| {
        properties.push(prop("part_of", name(s)));
        properties.extend(material.clone());
        ResolvedElement {
            element_type,
            id,
            properties,
            children: vec![],
            span,
            curves: Default::default(),
        }
    };

    let mut parts = Vec::new();
    let mut z = base;
    for (i, &(start, turn, n)) in flights.iter().enumerate() {
        let (x, y) = to_plan(vec![start])[0];
        let props = vec![
            prop("type", name("straight")),
            prop("position", ResolvedValue::Point(x, y)),
            prop("rotation", num(rotation + turn)),
            prop("z_offset", num(z)),
            prop("width", num(width)),
            prop("riser_count", num(n)),
            prop("riser_height", num(riser)),
            prop("tread_depth", num(tread)),
            prop(
                "ifc_class",
                ResolvedValue::String("IfcStairFlight".to_string()),
            ),
            prop("predefined_type", name("straight")),
            prop("waist_thickness", num(waist)),
        ];
        parts.push(part(ElementType::Stair, format!("{s}-F{}", i + 1), props));
        z += n * riser;
    }
    let landing_top = base + first * riser;
    let (x0, x1, y0, y1) = deck;
    let outline = to_plan(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    parts.push(part(
        ElementType::Slab,
        format!("{s}-L1"),
        vec![
            prop("boundary", ResolvedValue::Polygon(outline)),
            prop("z_offset", num(landing_top)),
            prop("thickness", num(waist)),
            prop("predefined_type", name("landing")),
        ],
    ));

    if let Some(taken) = parts.iter().find(|e| ids.contains(&e.id)) {
        return Err(error(format!(
            "stair `{s}` generates `{}`, which is already an element id",
            taken.id
        )));
    }

    let predefined = match kind {
        "l_shaped" => "quarter_turn_stair",
        "u_shaped" => "half_turn_stair",
        _ => "two_straight_run_stair",
    };
    let mut defaults = vec![("representation", name("none"))];
    if !p.iter().any(|q| q.key == "predefined_type") {
        defaults.push(("predefined_type", name(predefined)));
    }
    // Checks on `Stair` see the whole, not its flights.
    if number(p, "riser_height").is_none() {
        defaults.push(("riser_height", num(riser)));
    }
    push_defaults(stair, defaults);
    Ok(parts)
}
