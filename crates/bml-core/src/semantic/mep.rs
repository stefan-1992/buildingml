//! Routes of ducts, pipes and cable trays (`Specs_V0_3.md` §24): a `path:`
//! of three or more points becomes straight segments with a bend at every
//! corner, each smooth run one curved segment, and a route that ends on a
//! segment of another route of the same type joins it with a tee. The parts
//! replace the route; their ends meet exactly, so the IFC writer connects
//! their ports.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementCurve, ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel,
    number, string_or_ident as ident,
};
use crate::codegen::mep::{self, Fitting, Section};
use crate::codegen::step_writer::{Frame, add, cross, dot, length, normalize, rotate, sub};
use crate::geometry::curve::{Curve3, Segment};

/// Points closer than this are one point (1 mm).
const EPS: f64 = 1e-3;
/// Turns below this (radians) need no bend: where smooth pieces meet tangentially.
const NO_TURN: f64 = 1e-3;
/// Straight length a tee adds on each side of the run it joins.
const TEE_CLEARANCE: f64 = 0.05;

/// Properties a part does not inherit from its route.
const OWN: &[&str] = &[
    "path",
    "from",
    "z_offset",
    "guid",
    "ifc_name",
    "fitting",
    "branch",
    "rotation",
    "branch_diameter",
    "branch_width",
    "branch_height",
    "branch_rotation",
];

/// `tol`: chord error of a curved segment's polyline.
pub(super) fn resolve(model: &mut SemanticModel, tol: f64) -> Vec<Diagnostic> {
    let mut out = check_kinds(model);
    if !model.elements().any(is_route) {
        return out;
    }
    let ids: HashSet<String> = model
        .elements()
        .flat_map(|e| std::iter::once(e).chain(&e.children))
        .map(|e| e.id.clone())
        .collect();
    // Routes replaced by parts that do not keep the route id.
    let mut expanded: HashSet<String> = HashSet::new();
    for (_, spaces, elements) in model.containers_mut() {
        let mut lists: Vec<&mut Vec<ResolvedElement>> = std::iter::once(elements)
            .chain(spaces.iter_mut().map(|s| &mut s.elements))
            .collect();
        let routes_of = |lists: &[&mut Vec<ResolvedElement>]| -> Vec<ResolvedElement> {
            lists
                .iter()
                .flat_map(|l| l.iter())
                .filter(|e| is_route(e))
                .cloned()
                .collect()
        };
        let mut elems = routes_of(&lists);
        if elems.is_empty() {
            continue;
        }
        if join_straight(&mut lists, &elems) {
            elems = routes_of(&lists);
        }
        let routes: Vec<Route> = elems.iter().map(Route::of).collect();
        let index: HashMap<&str, usize> = elems
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.as_str(), i))
            .collect();
        let mut parts = expand(&routes, &ids, tol);
        for list in &mut lists {
            for elem in std::mem::take(*list) {
                let Some(&i) = is_route(&elem)
                    .then(|| index.get(elem.id.as_str()))
                    .flatten()
                else {
                    list.push(elem);
                    continue;
                };
                match std::mem::replace(&mut parts[i], Ok(vec![])) {
                    Ok(p) => {
                        if p.iter().all(|e| e.id != elem.id) {
                            expanded.insert(elem.id.clone());
                        }
                        list.extend(p);
                    }
                    Err(d) => {
                        out.push(*d);
                        list.push(elem);
                    }
                }
            }
        }
    }
    out.extend(refs_to_routes(model, &expanded));
    out
}

/// Error for every property that names a route which no longer exists
/// because it was replaced by its parts.
fn refs_to_routes(model: &SemanticModel, expanded: &HashSet<String>) -> Vec<Diagnostic> {
    if expanded.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for e in model
        .elements()
        .flat_map(|e| std::iter::once(e).chain(&e.children))
    {
        for p in &e.properties {
            let names: Vec<&str> = match &p.value {
                ResolvedValue::Identifier(v) | ResolvedValue::String(v) => vec![v],
                ResolvedValue::EnumList(v) => v.iter().map(String::as_str).collect(),
                _ => continue,
            };
            for n in names.into_iter().filter(|n| expanded.contains(*n)) {
                out.push(Diagnostic::error(
                    DiagnosticCode::E101,
                    format!(
                        "`{}` of `{}` names route `{n}`, which is replaced by its parts                          (`{n}-S1`, `{n}-B1`, ...); name a part instead",
                        p.key, e.id
                    ),
                    p.span,
                ));
            }
        }
    }
    out
}

/// A duct, pipe or cable tray given by a `path:` route.
fn is_route(e: &ResolvedElement) -> bool {
    mep::is_run(e.element_type)
        && mep::fitting(&e.properties) == Fitting::Segment
        && e.properties.iter().any(|p| p.key == "path")
}

/// `IfcDistributionSystemEnum` (IFC4), lower case.
const SYSTEM_TYPES: &[&str] = &[
    "airconditioning",
    "audiovisual",
    "chemical",
    "chilledwater",
    "communication",
    "compressedair",
    "condenserwater",
    "control",
    "conveying",
    "data",
    "disposal",
    "domesticcoldwater",
    "domestichotwater",
    "drainage",
    "earthing",
    "electrical",
    "electroacoustic",
    "exhaust",
    "fireprotection",
    "fuel",
    "gas",
    "hazardous",
    "heating",
    "lighting",
    "lightningprotection",
    "municipalsolidwaste",
    "oil",
    "operational",
    "powergeneration",
    "rainwater",
    "refrigeration",
    "security",
    "sewage",
    "signal",
    "stormwater",
    "telephone",
    "tv",
    "vacuum",
    "vent",
    "ventilation",
    "wastewater",
    "watersupply",
    "userdefined",
    "notdefined",
];

/// Error for a terminal, equipment or accessory `type` without an IFC class, or a
/// system `predefined_type` IFC does not know.
fn check_kinds(model: &SemanticModel) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for e in model.elements() {
        if mep::is_run(e.element_type) {
            for key in ["clearance", "hangers"] {
                if number(&e.properties, key).is_some_and(|c| c <= 0.0) {
                    out.push(Diagnostic::error(
                        DiagnosticCode::E102,
                        format!("`{key}` on {} must be positive", e.id),
                        e.span,
                    ));
                }
            }
        }
        let names = || -> Vec<&str> {
            mep::kinds(e.element_type).map_or_else(
                || SYSTEM_TYPES.to_vec(),
                |k| k.iter().map(|k| k.name).collect(),
            )
        };
        let key = match e.element_type {
            ElementType::Terminal | ElementType::Equipment | ElementType::Accessory => "type",
            ElementType::System => "predefined_type",
            _ => continue,
        };
        if let Some(t) = ident(&e.properties, key)
            && !names().contains(&t)
        {
            out.push(Diagnostic::error(
                DiagnosticCode::E102,
                format!(
                    "invalid value for `{key}` on {}: expected one of: {}",
                    e.element_type.as_str(),
                    names().join(", ")
                ),
                e.span,
            ));
        }
    }
    out
}

struct Route<'a> {
    elem: &'a ResolvedElement,
    /// Storey coordinates (`z_offset` added), without repeated or collinear points.
    pts: Vec<[f64; 3]>,
    /// Smooth run from `pts[leg]` to `pts[leg + 1]`, `None` for a straight leg.
    runs: Vec<Option<Curve3>>,
    section: Section,
}

impl<'a> Route<'a> {
    fn of(elem: &'a ResolvedElement) -> Self {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut runs: Vec<Option<Curve3>> = Vec::new();
        for (p, run) in legs(elem) {
            if run.is_some() || pts.last().is_none_or(|q| length(sub(p, *q)) > EPS) {
                if !pts.is_empty() {
                    runs.push(run);
                }
                pts.push(p);
            }
        }
        let mut i = 1;
        while i + 1 < pts.len() {
            let a = normalize(sub(pts[i], pts[i - 1]));
            let b = normalize(sub(pts[i + 1], pts[i]));
            if runs[i - 1].is_none()
                && runs[i].is_none()
                && length(cross(a, b)) < 1e-9
                && dot(a, b) > 0.0
            {
                pts.remove(i);
                runs.remove(i);
            } else {
                i += 1;
            }
        }
        Route {
            elem,
            pts,
            runs,
            section: Section::of(elem.element_type, &elem.properties),
        }
    }

    /// Direction of a straight leg.
    fn dir(&self, leg: usize) -> [f64; 3] {
        normalize(sub(self.pts[leg + 1], self.pts[leg]))
    }

    /// Direction leaving the start of a leg.
    fn dir_out(&self, leg: usize) -> [f64; 3] {
        match &self.runs[leg] {
            Some(c) => c.tangent_at(0.0),
            None => self.dir(leg),
        }
    }

    /// Direction arriving at the end of a leg.
    fn dir_in(&self, leg: usize) -> [f64; 3] {
        match &self.runs[leg] {
            Some(c) => c.tangent_at(c.length()),
            None => self.dir(leg),
        }
    }

    fn len(&self, leg: usize) -> f64 {
        match &self.runs[leg] {
            Some(c) => c.length(),
            None => length(sub(self.pts[leg + 1], self.pts[leg])),
        }
    }

    /// Direction of a leg `s` along it from its start.
    fn tangent(&self, leg: usize, s: f64) -> [f64; 3] {
        match &self.runs[leg] {
            Some(c) => c.tangent_at(s),
            None => self.dir(leg),
        }
    }

    /// Corner of the bend between the cut ends of `leg` (`s` along it) and
    /// the next leg (`t` along it): where the tangents there meet, so the bend
    /// is tangent to a smooth run it joins. The midpoint of the closest
    /// points if the tangents are skew; the route point if they do not meet
    /// ahead of both ends.
    fn bend_corner(&self, leg: usize, s: f64, t: f64) -> [f64; 3] {
        let (p, a) = (self.point(leg, s), self.tangent(leg, s));
        let (q, b) = (self.point(leg + 1, t), self.tangent(leg + 1, t));
        let w = sub(p, q);
        let (ab, aw, bw) = (dot(a, b), dot(a, w), dot(b, w));
        let den = 1.0 - ab * ab;
        if den < 1e-9 {
            return self.pts[leg + 1];
        }
        let (u, v) = ((ab * bw - aw) / den, (bw - ab * aw) / den);
        if u < EPS || v > -EPS {
            return self.pts[leg + 1];
        }
        let (c0, c1) = (add(p, a, u), add(q, b, v));
        std::array::from_fn(|i| (c0[i] + c1[i]) / 2.0)
    }

    /// Point `s` along a leg from its start.
    fn point(&self, leg: usize, s: f64) -> [f64; 3] {
        match &self.runs[leg] {
            Some(c) => c.point_at(s),
            None => add(self.pts[leg], self.dir(leg), s),
        }
    }

    /// Section rotation (radians) at the start of each leg: the route's
    /// `rotation` on the first, carried along every run and through every bend.
    fn rotations(&self) -> Vec<f64> {
        let mut r = number(&self.elem.properties, "rotation")
            .unwrap_or(0.0)
            .to_radians();
        let mut out = vec![r];
        for leg in 1..self.pts.len().saturating_sub(1) {
            r = turn(r, self.dir_out(leg - 1), self.dir_in(leg - 1));
            r = turn(r, self.dir_in(leg - 1), self.dir_out(leg));
            out.push(r);
        }
        out
    }
}

/// Points of a route with the leg arriving at each: `None` for a straight
/// leg, else the smooth run (storey coordinates). Consecutive smooth pieces
/// meeting tangentially are one run.
fn legs(elem: &ResolvedElement) -> Vec<([f64; 3], Option<Curve3>)> {
    let Some(ElementCurve::Path3(curve)) = elem.curve("path") else {
        return mep::path(&elem.properties)
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p, None))
            .collect();
    };
    let base = number(&elem.properties, "z_offset").unwrap_or(0.0);
    let curve = curve.translate([0.0, 0.0, base]);
    let Some(first) = curve.segments.first() else {
        return vec![];
    };
    let mut out: Vec<([f64; 3], Option<Curve3>)> = vec![(first.start(), None)];
    for seg in &curve.segments {
        if matches!(seg, Segment::Line(..)) {
            out.push((seg.end(), None));
            continue;
        }
        let piece = Curve3 {
            segments: vec![seg.clone()],
            closed: false,
        };
        if let Some((end, Some(run))) = out.last_mut()
            && angle(run.tangent_at(run.length()), piece.tangent_at(0.0)) < NO_TURN
        {
            run.segments.push(seg.clone());
            *end = seg.end();
            continue;
        }
        out.push((seg.end(), Some(piece)));
    }
    out
}

fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(a, b).clamp(-1.0, 1.0).acos()
}

/// Section rotation `r` about direction `a`, turned with the run onto `b`.
fn turn(r: f64, a: [f64; 3], b: [f64; 3]) -> f64 {
    let x = Frame::along([0.0; 3], a, r).x;
    let n = cross(a, b);
    if length(n) < 1e-12 {
        return rotation_for(x, b);
    }
    // x turned with the run about the bend normal.
    rotation_for(rotate(x, normalize(n), angle(a, b)), b)
}

/// Rotation about direction `d` that turns a section's local X onto `x`.
fn rotation_for(x: [f64; 3], d: [f64; 3]) -> f64 {
    let f = Frame::along([0.0; 3], d, 0.0);
    dot(x, f.y()).atan2(dot(x, f.x))
}

/// Where one route ends on a leg of another and joins it with a tee.
struct Tee {
    main: usize,
    leg: usize,
    /// Distance of the joint along the main leg.
    at: f64,
    /// Half-length of the tee along the main run.
    half: f64,
    /// End of the tee's branch (moved-back end of the branch route).
    branch_end: [f64; 3],
    branch_section: Section,
    /// Branch section rotation about the tee's stub, radians.
    branch_rotation: f64,
}

/// Parts of every route, or the error that leaves it as written.
fn expand(
    routes: &[Route],
    ids: &HashSet<String>,
    tol: f64,
) -> Vec<Result<Vec<ResolvedElement>, Box<Diagnostic>>> {
    let rotations: Vec<Vec<f64>> = routes.iter().map(Route::rotations).collect();
    // Trims at the start and end of each route where it joins a tee.
    let mut end_trims = vec![[0.0; 2]; routes.len()];
    let mut tees: Vec<Tee> = Vec::new();
    let mut errors: Vec<Option<Box<Diagnostic>>> = routes.iter().map(|_| None).collect();
    for (b, branch) in routes.iter().enumerate() {
        let n = branch.pts.len();
        if n < 2 {
            continue;
        }
        let mut trims = [0.0; 2];
        for (end, end_trim) in trims.iter_mut().enumerate() {
            // End point, direction into the route, and its leg.
            let (e, u, leg) = if end == 0 {
                (branch.pts[0], branch.dir_out(0), 0)
            } else {
                (branch.pts[n - 1], branch.dir_in(n - 2).map(|v| -v), n - 2)
            };
            let Some((m, ml, at)) = on_leg(routes, branch.elem.element_type, Some(b), e) else {
                continue;
            };
            let main = &routes[m];
            let a = main.dir(ml);
            if dot(u, a).abs() > 1e-3 {
                errors[b] = Some(error(
                    branch,
                    format!(
                        "route `{}` must leave `{}` at a right angle to join it with a tee",
                        branch.elem.id, main.elem.id
                    ),
                ));
                continue;
            }
            let main_frame = Frame::along(main.pts[ml], a, rotations[m][ml]);
            // The branch's rotation is measured along the route's direction.
            let along = if end == 0 { u } else { u.map(|v| -v) };
            let branch_frame = Frame::along(e, along, rotations[b][leg]);
            let half = branch.section.half_extent(&branch_frame, a) + TEE_CLEARANCE;
            let trim = main.section.half_extent(&main_frame, u) + TEE_CLEARANCE;
            *end_trim = trim;
            tees.push(Tee {
                main: m,
                leg: ml,
                at,
                half,
                branch_end: if end == 0 {
                    branch.point(0, trim)
                } else {
                    branch.point(leg, branch.len(leg) - trim)
                },
                branch_section: branch.section,
                branch_rotation: rotation_for(branch_frame.x, u),
            });
        }
        end_trims[b] = trims;
    }
    let mut by_main: Vec<Vec<&Tee>> = vec![vec![]; routes.len()];
    for t in &tees {
        by_main[t.main].push(t);
    }
    routes
        .iter()
        .enumerate()
        .zip(errors)
        .map(|((i, r), err)| {
            if let Some(e) = err {
                return Err(e);
            }
            let parts = route_parts(r, &rotations[i], end_trims[i], &by_main[i], tol)?;
            if let Some(taken) = parts
                .iter()
                .find(|e| e.id != r.elem.id && ids.contains(&e.id))
            {
                return Err(error(
                    r,
                    format!(
                        "route `{}` generates `{}`, which is already an element id",
                        r.elem.id, taken.id
                    ),
                ));
            }
            Ok(parts)
        })
        .collect()
}

/// The route (other than `skip`) of type `et`, leg and distance along it
/// where point `e` lies inside a leg.
fn on_leg(
    routes: &[Route],
    et: ElementType,
    skip: Option<usize>,
    e: [f64; 3],
) -> Option<(usize, usize, f64)> {
    routes.iter().enumerate().find_map(|(m, main)| {
        if Some(m) == skip || main.elem.element_type != et {
            return None;
        }
        (0..main.pts.len().saturating_sub(1)).find_map(|leg| {
            if main.runs[leg].is_some() {
                return None;
            }
            let p0 = main.pts[leg];
            let a = main.dir(leg);
            let at = dot(sub(e, p0), a);
            let len = length(sub(main.pts[leg + 1], p0));
            let off = length(sub(e, add(p0, a, at)));
            (off < EPS && at > EPS && at < len - EPS).then_some((m, leg, at))
        })
    })
}

/// Turns each straight `from`/`to` run that ends on a leg of a route of its
/// type, at a right angle, into a two-point route, so it joins with a tee.
/// Whether any was turned.
fn join_straight(lists: &mut [&mut Vec<ResolvedElement>], elems: &[ResolvedElement]) -> bool {
    let routes: Vec<Route> = elems.iter().map(Route::of).collect();
    let mut changed = false;
    for e in lists.iter_mut().flat_map(|l| l.iter_mut()) {
        // A `path:` route has no `from` axis.
        if !mep::is_run(e.element_type) || mep::fitting(&e.properties) != Fitting::Segment {
            continue;
        }
        let Some((a, b)) = mep::axis3(&e.properties) else {
            continue;
        };
        let u = normalize(sub(b, a));
        let joins = [a, b].into_iter().any(|p| {
            on_leg(&routes, e.element_type, None, p)
                .is_some_and(|(m, leg, _)| dot(u, routes[m].dir(leg)).abs() <= 1e-3)
        });
        let Some(from) = e.properties.iter_mut().find(|p| p.key == "from") else {
            continue;
        };
        let ResolvedValue::Axis { from: f, to, z, .. } = &from.value else {
            continue;
        };
        if !joins {
            continue;
        }
        let (z0, z1) = z.unwrap_or((0.0, 0.0));
        from.key = "path".to_string();
        from.value = ResolvedValue::Path3D(vec![(f.0, f.1, z0), (to.0, to.1, z1)]);
        changed = true;
    }
    changed
}

fn error(r: &Route, message: String) -> Box<Diagnostic> {
    Box::new(Diagnostic::error(
        DiagnosticCode::E101,
        message,
        r.elem.span,
    ))
}

/// Round sections only can follow a smooth run.
fn check_section(r: &Route) -> Result<(), Box<Diagnostic>> {
    if matches!(r.section, Section::Round(_)) || r.runs.iter().all(Option::is_none) {
        return Ok(());
    }
    let what = match r.elem.element_type {
        ElementType::CableTray => "cable tray",
        _ => "rectangular duct",
    };
    Err(Box::new(Diagnostic::error(
        DiagnosticCode::E313,
        format!(
            "{what} `{}` cannot follow a smooth run; use corners or a round section",
            r.elem.id
        ),
        r.elem.span,
    )))
}

/// Segments, bends and tees of one route, in order along it.
fn route_parts(
    r: &Route,
    rotations: &[f64],
    end_trims: [f64; 2],
    tees: &[&Tee],
    tol: f64,
) -> Result<Vec<ResolvedElement>, Box<Diagnostic>> {
    let n = r.pts.len();
    if n < 2 {
        return Err(error(
            r,
            format!("route `{}` needs at least two distinct points", r.elem.id),
        ));
    }
    check_section(r)?;
    let et = r.elem.element_type;
    let radius = number(&r.elem.properties, "bend_radius").unwrap_or(r.section.bend_radius(et));
    // Straight length a bend takes from each leg at every corner.
    let mut corner = vec![0.0; n];
    for (v, c) in corner.iter_mut().enumerate().take(n - 1).skip(1) {
        let angle = angle(r.dir_in(v - 1), r.dir_out(v));
        if angle > std::f64::consts::PI - 1e-3 {
            return Err(error(
                r,
                format!(
                    "route `{}` turns back on itself at point {}",
                    r.elem.id,
                    v + 1
                ),
            ));
        }
        if angle >= NO_TURN {
            *c = radius * (angle / 2.0).tan();
        }
    }
    corner[0] = end_trims[0];
    corner[n - 1] = end_trims[1];

    let span = r.elem.span;
    let prop = |key: &str, value| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let inherited: Vec<ResolvedProperty> = r
        .elem
        .properties
        .iter()
        .filter(|p| !OWN.contains(&p.key.as_str()))
        .cloned()
        .collect();
    let part = |kind: char, k: usize, mut props: Vec<ResolvedProperty>| {
        props.extend(inherited.iter().cloned());
        ResolvedElement {
            element_type: et,
            id: format!("{}-{kind}{k}", r.elem.id),
            properties: props,
            children: vec![],
            span,
            curves: Default::default(),
        }
    };
    let too_short = |leg: usize, missing: f64| {
        error(
            r,
            format!(
                "leg {} of route `{}` is too short for its bends and tees ({missing:.3} m missing)",
                leg + 1,
                r.elem.id,
            ),
        )
    };
    let axis = |p0: [f64; 3], p1: [f64; 3]| ResolvedValue::Axis {
        from: (p0[0], p0[1]),
        to: (p1[0], p1[1]),
        via: None,
        z: Some((p0[2], p1[2])),
    };
    let deg = |r: f64| ResolvedValue::Number(r.to_degrees());

    let mut parts = Vec::new();
    let (mut segments, mut bends, mut tee_count) = (0, 0, 0);
    for leg in 0..n - 1 {
        let len = r.len(leg);
        let rot = deg(rotations[leg]);
        if let Some(run) = &r.runs[leg] {
            // One curved segment along the smooth run, its polyline as `path`.
            let (start, end) = (corner[leg], len - corner[leg + 1]);
            if end < start - EPS {
                return Err(too_short(leg, start - end));
            }
            if end - start > EPS {
                let path = ElementCurve::Path3(run.between(start, end));
                segments += 1;
                let mut seg = part(
                    'S',
                    segments,
                    vec![
                        prop("path", path.tessellated(tol)),
                        prop("rotation", rot.clone()),
                    ],
                );
                seg.curves = BTreeMap::from([("path".to_string(), path)]);
                parts.push(seg);
            }
        }
        let (p0, a) = (r.pts[leg], r.dir(leg));
        let mut cursor = corner[leg];
        let mut on_leg: Vec<&&Tee> = tees.iter().filter(|t| t.leg == leg).collect();
        on_leg.sort_by(|x, y| x.at.total_cmp(&y.at));
        let mut cuts: Vec<(f64, f64, Option<&Tee>)> = on_leg
            .iter()
            .map(|t| (t.at - t.half, t.at + t.half, Some(**t)))
            .collect();
        if r.runs[leg].is_none() {
            cuts.push((len - corner[leg + 1], len - corner[leg + 1], None));
        }
        for (start, end, tee) in cuts {
            if start < cursor - EPS {
                return Err(too_short(leg, cursor - start));
            }
            if start - cursor > EPS {
                segments += 1;
                parts.push(part(
                    'S',
                    segments,
                    vec![
                        prop("from", axis(add(p0, a, cursor), add(p0, a, start))),
                        prop("rotation", rot.clone()),
                    ],
                ));
            }
            if let Some(t) = tee {
                tee_count += 1;
                let mut props = vec![
                    prop("fitting", ResolvedValue::Identifier("tee".to_string())),
                    prop("from", axis(add(p0, a, start), add(p0, a, end))),
                    prop("rotation", rot.clone()),
                    prop("branch", {
                        let [x, y, z] = t.branch_end;
                        ResolvedValue::Point3D(x, y, z)
                    }),
                    prop("branch_rotation", deg(t.branch_rotation)),
                ];
                props.extend(section_props(t.branch_section, &prop));
                parts.push(part('T', tee_count, props));
            }
            cursor = end;
        }
        // A bend at the corner, ending where the legs either side are cut
        // (on a smooth run, at its curve).
        if leg + 1 < n - 1 && angle(r.dir_in(leg), r.dir_out(leg + 1)) >= NO_TURN {
            let t = corner[leg + 1];
            let v = r.bend_corner(leg, len - t, t);
            bends += 1;
            parts.push(part(
                'B',
                bends,
                vec![
                    prop("fitting", ResolvedValue::Identifier("bend".to_string())),
                    prop(
                        "path",
                        ResolvedValue::Path3D(
                            [r.point(leg, len - t), v, r.point(leg + 1, t)]
                                .map(|[x, y, z]| (x, y, z))
                                .to_vec(),
                        ),
                    ),
                    prop("rotation", rot),
                ],
            ));
        }
    }
    // A single straight segment keeps the route's id.
    if let [only] = &mut parts[..]
        && mep::fitting(&only.properties) == Fitting::Segment
    {
        only.id = r.elem.id.to_string();
        only.properties.extend(
            r.elem
                .properties
                .iter()
                .filter(|p| p.key == "guid" || p.key == "ifc_name")
                .cloned(),
        );
    }
    Ok(parts)
}

/// `branch_` section properties of a tee from its branch route's section.
fn section_props(
    s: Section,
    prop: &dyn Fn(&str, ResolvedValue) -> ResolvedProperty,
) -> Vec<ResolvedProperty> {
    let num = ResolvedValue::Number;
    match s {
        Section::Round(d) => vec![prop("branch_diameter", num(d))],
        Section::Rect(w, h) | Section::Tray(w, h) => {
            vec![prop("branch_width", num(w)), prop("branch_height", num(h))]
        }
    }
}
