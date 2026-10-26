//! Defaults of a `Footing` (`Specs_V0_3.md` §21): a strip footing takes its
//! outline from the walls it runs `under:`, a pad footing from `position:`
//! and `width:`/`length:`. A strip footing's top defaults to the lowest base
//! of its walls.

use std::collections::HashMap;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use super::covering::{ccw, polygon, push_defaults, rect, shape_value};
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementType, ResolvedElement, ResolvedValue, SemanticModel, contents, contents_mut, number,
    refs, string_or_ident as ident,
};
use crate::codegen::geometry::{self, placed};
use crate::geometry::axis::WallAxis;
use crate::scene::graph::get_point;

/// Fill in `boundary` (and `z_offset` of strip footings) of every footing that
/// has none. Returns the errors of footings whose outline cannot be built.
pub(super) fn resolve(model: &mut SemanticModel) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (_, spaces, elements) in model.containers_mut() {
        let is_footing = |e: &&ResolvedElement| e.element_type == ElementType::Footing;
        if !contents(spaces, elements).any(|e| is_footing(&e)) {
            continue;
        }
        let walls: HashMap<&str, &ResolvedElement> = contents(spaces, elements)
            .filter(|e| e.element_type == ElementType::Wall)
            .map(|e| (e.id.as_str(), e))
            .collect();
        let mut additions: HashMap<String, Vec<(&str, ResolvedValue)>> = contents(spaces, elements)
            .filter(is_footing)
            .filter_map(|f| match defaults(f, &walls) {
                Ok(add) => Some((f.id.clone(), add)),
                Err(d) => {
                    out.push(*d);
                    None
                }
            })
            .collect();
        for elem in contents_mut(spaces, elements) {
            if let Some(add) = additions.remove(&elem.id) {
                push_defaults(elem, add);
            }
        }
    }
    out
}

/// Properties to add to footing `f`, given the walls of its container.
fn defaults(
    f: &ResolvedElement,
    walls: &HashMap<&str, &ResolvedElement>,
) -> Result<Vec<(&'static str, ResolvedValue)>, Box<Diagnostic>> {
    let p = &f.properties;
    if polygon(p, "boundary").is_some() || ident(p, "representation") == Some("none") {
        return Ok(vec![]);
    }
    let error = |code, message: String| Box::new(Diagnostic::error(code, message, f.span));
    let width = || {
        number(p, "width").ok_or_else(|| {
            error(
                DiagnosticCode::E101,
                format!("Footing `{}` is missing `width`", f.id),
            )
        })
    };
    let under = refs(p, "under");
    if !under.is_empty() {
        let width = width()?;
        let mut strips = Vec::new();
        let mut top = f64::INFINITY;
        for id in under {
            let wall = walls.get(id).ok_or_else(|| {
                // Not E301: projects drop per-file reference errors and
                // re-check them on the merged model, which skips `under:`.
                error(
                    DiagnosticCode::E201,
                    format!(
                        "`{id}` in `under:` of footing `{}` is not a wall of its storey",
                        f.id
                    ),
                )
            })?;
            let axis =
                WallAxis::of(wall, crate::config::default_curve_tolerance()).ok_or_else(|| {
                    error(
                        DiagnosticCode::E101,
                        format!("wall `{id}` under footing `{}` has no axis", f.id),
                    )
                })?;
            if axis.length() < 1e-9 {
                return Err(error(
                    DiagnosticCode::E101,
                    format!("wall `{id}` under footing `{}` has zero length", f.id),
                ));
            }
            strips.push(ccw(strip(&axis, width)));
            top = top.min(number(&wall.properties, "z_offset").unwrap_or(0.0));
        }
        let mut pieces = strips.overlay(
            &Vec::<[f64; 2]>::new(),
            OverlayRule::Subject,
            FillRule::NonZero,
        );
        if pieces.len() != 1 {
            return Err(error(
                DiagnosticCode::E101,
                format!(
                    "the walls under footing `{}` do not touch; give each connected group its own footing",
                    f.id
                ),
            ));
        }
        let mut add = vec![("boundary", shape_value(pieces.remove(0)))];
        if number(p, "z_offset").is_none() {
            add.push(("z_offset", ResolvedValue::Number(top)));
        }
        return Ok(add);
    }
    if let Some((x, y)) = get_point(p, "position") {
        let w = width()?;
        let l = number(p, "length").unwrap_or(w);
        let ring = placed(rect(w, l), (x, y), number(p, "rotation").unwrap_or(0.0));
        return Ok(vec![("boundary", ResolvedValue::Polygon(ring))]);
    }
    let mut d = error(
        DiagnosticCode::E101,
        format!("Footing `{}` is missing `boundary`", f.id),
    );
    d.help = Some("add `under:` walls, `position:` with `width:`, or `boundary:`".to_string());
    Err(d)
}

/// Plan outline of a strip of `width` under a wall axis. Straight strips run
/// past both ends by half their width, so strips meeting at a corner overlap.
fn strip(axis: &WallAxis, width: f64) -> Vec<(f64, f64)> {
    if !axis.is_straight() {
        return geometry::wall_outline(axis, width);
    }
    let (len, h) = (axis.length(), width / 2.0);
    let ends = vec![axis.point_at(0.0), axis.point_at(len)];
    let [dx, dy] = axis.tangent_at(0.0);
    let ends = ends
        .into_iter()
        .zip([-h, h])
        .map(|(p, e)| [p[0] + dx * e, p[1] + dy * e])
        .collect();
    WallAxis::polyline(ends).map_or_else(Vec::new, |a| geometry::wall_outline(&a, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `via:` on the axis line draws a straight wall, so its strip runs
    /// past the ends like any straight strip.
    #[test]
    fn collinear_via_strip_is_extended() {
        let source = "# Site: S\n## Building: B\n### Storey: L1\n  elevation: 0 m\n  Wall: W\n    from: (0, 0) via: (3, 0) to: (6, 0)\n    height: 3 m\n";
        let (model, _) = super::super::analyze(&crate::parser::cst_to_ast::parse(source).unwrap());
        let wall = model.elements().next().unwrap();
        let xs: Vec<f64> = strip(&WallAxis::of(wall, 0.005).unwrap(), 0.6)
            .iter()
            .map(|p| p.0)
            .collect();
        let (lo, hi) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(
            (lo + 0.3).abs() < 1e-9 && (hi - 6.3).abs() < 1e-9,
            "{lo} {hi}"
        );
    }
}
