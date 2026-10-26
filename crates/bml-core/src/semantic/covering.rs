//! Defaults of a `Covering` inside a `####` space (`Specs_V0_3.md` §20): its
//! outline is the space boundary minus the plan footprints of the walls and
//! columns of the same storey that pass through it, and a ceiling hangs at the
//! top of the space.

use std::collections::HashSet;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementType, ResolvedElement, ResolvedProperty, ResolvedSpace, ResolvedValue, SemanticModel,
    contents, number, string_or_ident as ident,
};
use crate::codegen::geometry::{self, placed};
use crate::geometry::axis::WallAxis;
use crate::scene::graph::{get_point, get_polygon, get_polygon_with_holes, get_profile};

/// Fill in `boundary` and, for ceilings, `z_offset` of the coverings in the
/// spaces of every storey, building and site.
pub(super) fn resolve(model: &mut SemanticModel) {
    for site in &mut model.sites {
        for building in &mut site.buildings {
            for storey in &mut building.storeys {
                let height = number(&storey.properties, "height").unwrap_or(DEFAULT_HEIGHT);
                resolve_container(&mut storey.spaces, &storey.elements, height);
            }
            resolve_container(&mut building.spaces, &building.elements, DEFAULT_HEIGHT);
        }
        resolve_container(&mut site.spaces, &site.elements, DEFAULT_HEIGHT);
    }
}

/// Storey height the IFC writer assumes when none is given.
const DEFAULT_HEIGHT: f64 = 3.0;

/// A wall or column footprint with the heights its body spans.
struct Cut {
    ring: Vec<[f64; 2]>,
    bottom: f64,
    top: f64,
}

/// `elements` are the container-level elements next to `spaces`, `height`
/// the container height that spaces without `height` take.
fn resolve_container(spaces: &mut [ResolvedSpace], elements: &[ResolvedElement], height: f64) {
    let is_covering = |e: &ResolvedElement| e.element_type == ElementType::Covering;
    if !spaces.iter().any(|s| s.elements.iter().any(is_covering)) {
        return;
    }
    // Wholes of generated parts: an expanded curtain wall is one of them.
    let wholes: HashSet<&str> = contents(spaces, elements)
        .filter_map(|e| ident(&e.properties, "part_of"))
        .collect();
    let cuts: Vec<Cut> = contents(spaces, elements)
        .filter_map(|e| {
            let bottom = number(&e.properties, "z_offset").unwrap_or(0.0);
            let top = number(&e.properties, "height").map_or(f64::INFINITY, |h| bottom + h);
            Some(Cut {
                ring: ccw(footprint(e, &wholes)?),
                bottom,
                top,
            })
        })
        .collect();

    for space in spaces.iter_mut() {
        let base = number(&space.properties, "z_offset").unwrap_or(0.0);
        let ceiling = base + number(&space.properties, "height").unwrap_or(height);
        let room = polygon(&space.properties, "boundary");
        let room_holes = get_polygon_with_holes(&space.properties, "boundary");
        // Outlines already computed in this space, by covering z range.
        let mut outlines: Vec<((f64, f64), Option<ResolvedValue>)> = Vec::new();
        for elem in space.elements.iter_mut().filter(|e| is_covering(e)) {
            let p = &elem.properties;
            let mut add = Vec::new();
            let z = number(p, "z_offset").or_else(|| {
                (ident(p, "predefined_type") == Some("ceiling")).then(|| {
                    add.push(("z_offset", ResolvedValue::Number(ceiling)));
                    ceiling
                })
            });
            if polygon(p, "boundary").is_none()
                && let Some(room) = &room
            {
                let z = z.unwrap_or(0.0);
                let t = number(p, "thickness").unwrap_or(geometry::COVERING_THICKNESS);
                let outline = match outlines.iter().find(|(range, _)| *range == (z, t)) {
                    Some((_, o)) => o.clone(),
                    None => {
                        // Bodies that pass through the covering's thickness.
                        let mut rings: Vec<Vec<[f64; 2]>> = cuts
                            .iter()
                            .filter(|c| c.top > z + EPS && c.bottom < z + t - EPS)
                            .map(|c| c.ring.clone())
                            .collect();
                        rings.extend(room_holes.iter().map(|h| ccw(h.clone())));
                        let o = subtract(room, &rings);
                        outlines.push(((z, t), o.clone()));
                        o
                    }
                };
                if let Some(o) = outline {
                    add.push(("boundary", o));
                }
            }
            push_defaults(elem, add);
        }
    }
}

const EPS: f64 = 1e-3;

/// `ring` counter-clockwise, as `[x, y]` points, so that overlapping cuts
/// add up under the non-zero fill rule.
pub(super) fn ccw(mut ring: Vec<(f64, f64)>) -> Vec<[f64; 2]> {
    let n = ring.len();
    let signed: f64 = (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    if signed < 0.0 {
        ring.reverse();
    }
    ring.into_iter().map(|(x, y)| [x, y]).collect()
}

/// E101 for coverings whose outline or ceiling height `resolve` could not
/// default: outside a space, or in a space without boundary.
pub(super) fn missing_geometry(model: &SemanticModel) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for elem in model.elements() {
        let p = &elem.properties;
        if elem.element_type != ElementType::Covering || ident(p, "representation") == Some("none")
        {
            continue;
        }
        let ceiling = ident(p, "predefined_type") == Some("ceiling");
        for (key, missing) in [
            ("boundary", polygon(p, "boundary").is_none()),
            ("z_offset", ceiling && number(p, "z_offset").is_none()),
        ] {
            if missing {
                out.push(
                    Diagnostic::error(
                        DiagnosticCode::E101,
                        format!("Covering `{}` is missing `{key}`", elem.id),
                        elem.span,
                    )
                    .with_help(format!(
                        "inside a space with `boundary` it defaults; else add `{key}:`"
                    )),
                );
            }
        }
    }
    out
}

/// Largest piece of `room` outside all `cuts`, with its holes.
fn subtract(room: &[(f64, f64)], cuts: &[Vec<[f64; 2]>]) -> Option<ResolvedValue> {
    let subject: Vec<[f64; 2]> = room.iter().map(|&(x, y)| [x, y]).collect();
    let pieces = subject.overlay(&cuts, OverlayRule::Difference, FillRule::NonZero);
    let (_, largest) = pieces
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            let outer: Vec<(f64, f64)> = piece[0].iter().map(|p| (p[0], p[1])).collect();
            (geometry::polygon_area(&outer), piece)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    Some(shape_value(largest))
}

/// A boolean-op result shape (outer contour, then holes) as a boundary value,
/// rounded to a micrometre to drop the float noise of the operation.
pub(super) fn shape_value(shape: Vec<Vec<[f64; 2]>>) -> ResolvedValue {
    let round = |v: f64| (v * 1e6).round() / 1e6;
    let mut rings = shape.into_iter().map(|c| {
        c.into_iter()
            .map(|p| (round(p[0]), round(p[1])))
            .collect::<Vec<_>>()
    });
    let outer = rings.next().unwrap_or_default();
    let holes: Vec<_> = rings.collect();
    if holes.is_empty() {
        ResolvedValue::Polygon(outer)
    } else {
        ResolvedValue::PolygonWithHoles { outer, holes }
    }
}

/// Plan footprint of a wall or column, `None` for other elements and for
/// elements without a body. A curtain wall whose grid was expanded (one of
/// `wholes`) is bodiless but covers the depth of its members.
fn footprint(elem: &ResolvedElement, wholes: &HashSet<&str>) -> Option<Vec<(f64, f64)>> {
    let p = &elem.properties;
    let is_wall = elem.element_type == ElementType::Wall;
    let grid =
        (is_wall && wholes.contains(elem.id.as_str()) && p.iter().any(|q| q.key == "mullions"))
            .then(|| super::curtain_wall::depth(p))
            .flatten();
    if !(is_wall || elem.element_type == ElementType::Column)
        || (ident(p, "representation") == Some("none") && grid.is_none())
    {
        return None;
    }
    if let Some(boundary) = polygon(p, "boundary") {
        return Some(boundary);
    }
    if is_wall {
        let axis = WallAxis::of(elem, crate::config::default_curve_tolerance())?;
        let thickness = grid.or_else(|| number(p, "thickness")).unwrap_or(0.2);
        return Some(geometry::wall_outline(&axis, thickness));
    }
    let (x, y) = get_point(p, "position")?;
    let local: Vec<(f64, f64)> = match get_profile(p, "profile") {
        Some((kind, params)) if kind == "circle" && !params.is_empty() => (0..16)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 16.0;
                (params[0] * a.cos(), params[0] * a.sin())
            })
            .collect(),
        Some((kind, params)) if kind == "rectangle" && params.len() >= 2 => {
            rect(params[0], params[1])
        }
        _ => rect(0.3, 0.3),
    };
    Some(placed(local, (x, y), number(p, "rotation").unwrap_or(0.0)))
}

/// Add defaulted properties to `elem`.
pub(super) fn push_defaults(elem: &mut ResolvedElement, add: Vec<(&str, ResolvedValue)>) {
    for (key, value) in add {
        elem.properties.push(ResolvedProperty {
            key: key.to_string(),
            value,
            span: elem.span,
        });
    }
}

pub(super) fn rect(w: f64, d: f64) -> Vec<(f64, f64)> {
    let (hw, hd) = (w / 2.0, d / 2.0);
    vec![(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)]
}

/// Outer ring of a polygon property with at least three points.
pub(super) fn polygon(props: &[ResolvedProperty], key: &str) -> Option<Vec<(f64, f64)>> {
    get_polygon(props, key).filter(|pts| pts.len() >= 3)
}
