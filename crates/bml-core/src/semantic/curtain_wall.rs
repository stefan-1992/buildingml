//! Curtain wall grid (`Specs_V0_3.md` §22): a wall with `mullions:` becomes a
//! bodiless `IfcCurtainWall` whose parts are generated next to it: mullions
//! (vertical members), transoms (horizontal members) and one upright panel
//! plate per grid cell. Its doors and windows move out of it as parts too.
//! Irregular grids are written as explicit `Member`s with `part_of:` instead.

use std::collections::HashSet;

use super::covering::push_defaults;
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel, number,
};
use crate::codegen::geometry;
use crate::codegen::ifc_writer::{filler_extent, no_body};
use crate::geometry::axis::WallAxis;
use crate::scene::graph::get_profile;

const EPS: f64 = 1e-6;

/// Default mullion section: face width along the wall, depth across it.
pub(super) const MULLION: (f64, f64) = (0.05, 0.15);

/// Panel thickness when the wall gives none.
const THICKNESS: f64 = 0.2;

/// Expand every curtain wall grid. Returns the errors of grids that cannot be
/// built; their walls stay plain walls.
pub(super) fn resolve(model: &mut SemanticModel) -> Vec<Diagnostic> {
    super::parts::expand_all(model, is_grid, expand)
}

pub(super) fn is_grid(e: &ResolvedElement) -> bool {
    e.element_type == ElementType::Wall
        && e.properties.iter().any(|p| p.key == "mullions")
        && !no_body(e)
}

/// A door or window of the wall: `s` along the axis, `z` above the wall base.
struct Filler {
    s: (f64, f64),
    z: (f64, f64),
}

impl Filler {
    fn overlaps(&self, s: (f64, f64), z: (f64, f64)) -> bool {
        s.0 < self.s.1 - EPS && s.1 > self.s.0 + EPS && z.0 < self.z.1 - EPS && z.1 > self.z.0 + EPS
    }
}

/// Turn `wall` into the bodiless whole and return its parts.
fn expand(
    wall: &mut ResolvedElement,
    _storey_height: f64,
    ids: &HashSet<String>,
) -> Result<Vec<ResolvedElement>, Box<Diagnostic>> {
    let span = wall.span;
    let error = |message: String| Box::new(Diagnostic::error(DiagnosticCode::E101, message, span));
    let p = &wall.properties;
    let w = wall.id.as_str();
    let wall_axis = WallAxis::of(wall, crate::config::default_curve_tolerance())
        .ok_or_else(|| error(format!("curtain wall `{w}` has no axis")))?;
    let length = wall_axis.length();
    let height = number(p, "height")
        .ok_or_else(|| error(format!("curtain wall `{w}` is missing `height`")))?;
    if length < EPS || height < EPS {
        return Err(error(format!(
            "curtain wall `{w}` has zero length or height"
        )));
    }
    let base = number(p, "z_offset").unwrap_or(0.0);
    let thickness = number(p, "thickness").unwrap_or(THICKNESS);
    let (mullion, transom) = sections(p, w).map_err(error)?;
    let (bays, mullions) = grid_lines(p, "mullions", length, w).map_err(error)?;
    let (rows, transoms) = match p.iter().find(|q| q.key == "transoms") {
        Some(_) => grid_lines(p, "transoms", height, w).map_err(error)?,
        None => (vec![0.0, height], vec![]),
    };
    if mullion.0 >= length || transom.0 >= height {
        return Err(error(format!(
            "members of curtain wall `{w}` are wider than the wall"
        )));
    }
    if let Some(o) = wall
        .children
        .iter()
        .find(|c| c.element_type == ElementType::Opening)
    {
        return Err(error(format!(
            "opening `{}` in curtain wall `{w}`: use a door or window, or a cell left open",
            o.id
        )));
    }

    let point = |s: f64| {
        let [x, y] = wall_axis.point_at(s);
        (x, y)
    };
    let angle = |s: f64| {
        let (a, b) = (point(s - 1e-4), point(s + 1e-4));
        (b.1 - a.1).atan2(b.0 - a.0).to_degrees()
    };
    // Centre of a member at grid line `x` of `0..end`, kept inside the wall.
    let centre = |x: f64, face: f64, end: f64| x.clamp(face / 2.0, end - face / 2.0);
    // Inner edges of the cell between grid lines `a` and `b`.
    let inner = |a: f64, b: f64, lines: &[f64], face: f64, end: f64| {
        let has = |x: f64| lines.iter().any(|l| (l - x).abs() < EPS);
        (
            if has(a) {
                centre(a, face, end) + face / 2.0
            } else {
                a
            },
            if has(b) {
                centre(b, face, end) - face / 2.0
            } else {
                b
            },
        )
    };
    let cell_s = |a, b| inner(a, b, &mullions, mullion.0, length);
    let cell_z = |a, b| inner(a, b, &transoms, transom.0, height);
    let bay_cells: Vec<(f64, f64)> = bays.windows(2).map(|b| cell_s(b[0], b[1])).collect();

    let fillers: Vec<Filler> = wall
        .children
        .iter()
        .map(|c| {
            let (offset, width, sill, h) = filler_extent(c);
            Filler {
                s: (offset, offset + width),
                z: (sill, sill + h),
            }
        })
        .collect();
    if let Some((c, _)) =
        wall.children.iter().zip(&fillers).find(|(_, f)| {
            f.s.0 < -EPS || f.s.1 > length + EPS || f.z.0 < -EPS || f.z.1 > height + EPS
        })
    {
        return Err(error(format!(
            "`{}` does not fit in curtain wall `{w}` ({length:.2} x {height:.2} m)",
            c.id
        )));
    }

    let material = wall
        .properties
        .iter()
        .find(|q| q.key == "material")
        .cloned();
    let panel_material = wall
        .properties
        .iter()
        .find(|q| q.key == "panel_material")
        .map(|q| q.value.clone());
    let prop = |key: &str, value: ResolvedValue| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let ident = |s: &str| ResolvedValue::Identifier(s.to_string());
    let rect = |(a, b): (f64, f64)| ResolvedValue::Profile {
        profile_type: "rectangle".to_string(),
        params: vec![a, b],
    };
    let axis = |a: (f64, f64), b: (f64, f64), z: Option<(f64, f64)>| ResolvedValue::Axis {
        from: a,
        to: b,
        via: None,
        z,
    };
    let part = |element_type, id: String, mut properties: Vec<ResolvedProperty>| {
        properties.push(prop("part_of", ident(w)));
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

    // Doors and windows become parts with a prism body in the cell they fill,
    // between the faces of the members on its edges.
    for (mut c, f) in wall.children.iter().cloned().zip(&fillers) {
        let depth = number(&c.properties, "depth").unwrap_or(thickness);
        let s = cell_s(f.s.0, f.s.1);
        let z = cell_z(f.z.0, f.z.1);
        let ring = geometry::wall_outline(&wall_axis.sub(s.0, s.1), depth);
        c.properties.retain(|q| {
            !matches!(
                q.key.as_str(),
                "z_offset" | "width" | "height" | "boundary" | "part_of"
            )
        });
        c.curves.remove("boundary");
        push_defaults(
            &mut c,
            vec![
                ("boundary", ResolvedValue::Polygon(ring)),
                ("z_offset", ResolvedValue::Number(base + z.0)),
                ("width", ResolvedValue::Number(s.1 - s.0)),
                ("height", ResolvedValue::Number(z.1 - z.0)),
                ("part_of", ident(w)),
            ],
        );
        parts.push(c);
    }

    for (k, &x) in mullions.iter().enumerate() {
        let c = centre(x, mullion.0, length);
        let at = point(c);
        // Split around fillers its grid line runs through.
        let mut pieces: Vec<(f64, f64)> = vec![(0.0, height)];
        for f in fillers
            .iter()
            .filter(|f| x > f.s.0 + EPS && x < f.s.1 - EPS)
        {
            pieces = pieces
                .into_iter()
                .flat_map(|(lo, hi)| [(lo, hi.min(f.z.0)), (lo.max(f.z.1), hi)])
                .filter(|(lo, hi)| hi - lo > EPS)
                .collect();
        }
        let several = pieces.len() > 1;
        for (i, (lo, hi)) in pieces.into_iter().enumerate() {
            let id = if several {
                format!("{w}-M{}-{}", k + 1, i + 1)
            } else {
                format!("{w}-M{}", k + 1)
            };
            let mut props = vec![
                prop("from", axis(at, at, Some((base + lo, base + hi)))),
                prop("profile", rect(mullion)),
                prop("rotation", ResolvedValue::Number(angle(c))),
                prop("predefined_type", ident("mullion")),
            ];
            props.extend(material.clone());
            parts.push(part(ElementType::Member, id, props));
        }
    }

    for (j, &z) in transoms.iter().enumerate() {
        let zc = centre(z, transom.0, height);
        for (b, &(s0, s1)) in bay_cells.iter().enumerate() {
            // Skipped where its grid line runs through a door or window.
            if s1 - s0 < EPS || fillers.iter().any(|f| f.overlaps((s0, s1), (z, z))) {
                continue;
            }
            let z = Some((base + zc, base + zc));
            let mut props = vec![
                prop("from", axis(point(s0), point(s1), z)),
                prop("profile", rect((transom.1, transom.0))),
                // IFC4 has no TRANSOM member type; MULLION covers both directions.
                prop("predefined_type", ident("mullion")),
            ];
            props.extend(material.clone());
            parts.push(part(
                ElementType::Member,
                format!("{w}-T{}-{}", j + 1, b + 1),
                props,
            ));
        }
    }

    for r in 0..rows.len() - 1 {
        let (z0, z1) = cell_z(rows[r], rows[r + 1]);
        for (b, &(s0, s1)) in bay_cells.iter().enumerate() {
            if s1 - s0 < EPS
                || z1 - z0 < EPS
                || fillers.iter().any(|f| f.overlaps((s0, s1), (z0, z1)))
            {
                continue;
            }
            let mut props = vec![
                prop("from", axis(point(s0), point(s1), None)),
                prop("z_offset", ResolvedValue::Number(base + z0)),
                prop("height", ResolvedValue::Number(z1 - z0)),
                prop("thickness", ResolvedValue::Number(thickness)),
                prop("predefined_type", ident("curtain_panel")),
            ];
            props.extend(panel_material.clone().map(|m| prop("material", m)));
            parts.push(part(
                ElementType::Plate,
                format!("{w}-P{}-{}", r + 1, b + 1),
                props,
            ));
        }
    }

    // Doors and windows keep their own ids.
    let generated =
        |e: &&ResolvedElement| matches!(e.element_type, ElementType::Member | ElementType::Plate);
    if let Some(taken) = parts.iter().filter(generated).find(|e| ids.contains(&e.id)) {
        return Err(error(format!(
            "curtain wall `{w}` generates `{}`, which is already an element id",
            taken.id
        )));
    }

    let mut add = vec![("representation", ident("none"))];
    if !wall.properties.iter().any(|q| q.key == "ifc_class") {
        add.push((
            "ifc_class",
            ResolvedValue::String("IfcCurtainWall".to_string()),
        ));
    }
    push_defaults(wall, add);
    wall.children.clear();
    Ok(parts)
}

/// Plan depth of an expanded curtain wall grid: its panels or deepest
/// members.
pub(super) fn depth(p: &[ResolvedProperty]) -> Option<f64> {
    let (mullion, transom) = sections(p, "").ok()?;
    let thickness = number(p, "thickness").unwrap_or(THICKNESS);
    Some(thickness.max(mullion.1).max(transom.1))
}

/// `(face, depth)` of a member section.
type Section = (f64, f64);

/// Mullion and transom sections of curtain wall `w`.
fn sections(p: &[ResolvedProperty], w: &str) -> Result<(Section, Section), String> {
    let mullion = section(p, "mullion_profile", MULLION, w)?;
    Ok((mullion, section(p, "transom_profile", mullion, w)?))
}

/// `(face, depth)` of a rectangle profile property, else `default`.
fn section(
    p: &[ResolvedProperty],
    key: &str,
    default: (f64, f64),
    w: &str,
) -> Result<(f64, f64), String> {
    match get_profile(p, key) {
        None => Ok(default),
        Some((t, params)) if t == "rectangle" && params.len() >= 2 => Ok((params[0], params[1])),
        Some(_) => Err(format!(
            "`{key}` of curtain wall `{w}` must be `rectangle(face, depth)`"
        )),
    }
}

/// Cell edges (from 0 to `end`) and member lines of a `mullions:` or
/// `transoms:` value: a spacing puts members on all edges of equal cells, a
/// list puts them at the listed distances only.
fn grid_lines(
    p: &[ResolvedProperty],
    key: &str,
    end: f64,
    w: &str,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    match p.iter().find(|q| q.key == key).map(|q| &q.value) {
        Some(ResolvedValue::Number(spacing)) if *spacing > EPS => {
            let n = (end / spacing - EPS).ceil().max(1.0) as usize;
            let edges: Vec<f64> = (0..=n).map(|k| end * k as f64 / n as f64).collect();
            Ok((edges.clone(), edges))
        }
        Some(ResolvedValue::NumberList(list))
            if list.windows(2).all(|w| w[1] > w[0] + EPS)
                && list.iter().all(|&x| (-EPS..=end + EPS).contains(&x)) =>
        {
            let mut edges = list.clone();
            if edges.first().is_none_or(|&x| x > EPS) {
                edges.insert(0, 0.0);
            }
            if edges.last().is_none_or(|&x| x < end - EPS) {
                edges.push(end);
            }
            Ok((edges, list.clone()))
        }
        _ => Err(format!(
            "`{key}` of curtain wall `{w}` must be a spacing > 0 or increasing distances within 0..{end}"
        )),
    }
}
