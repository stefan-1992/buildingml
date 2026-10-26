//! Openings where a straight duct, pipe or cable tray passes through a wall,
//! slab or covering (`Specs_V0_3.md` §25). Each crossing adds an `Opening` child
//! `{run}-{host}` to the host, `clearance` larger than the run on every side,
//! and with `sleeve: true` a sleeve `{run}-{host}-SL` that fills it. An
//! element that already has a generated id replaces it; a sleeve then fills
//! the hand-written opening.
//!
//! A run with `hangers: <spacing>` also gets trapeze hangers `{run}-H{i}`
//! along its horizontal segments, each up to the lowest slab above it
//! (`Specs_V0_3.md` §26).
//!
//! The mullions of a curtain wall with `brackets: true` get a bracket
//! `{mullion}-{slab}` at each slab edge they pass (`Specs_V0_3.md` §27).

use std::collections::{HashMap, HashSet};

use crate::ast::span::Span;

use super::curtain_wall::{MULLION, is_grid};
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{
    ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel, boolean, number,
    string_or_ident,
};
use crate::codegen::geometry::{self, point_in_polygon};
use crate::codegen::ifc_writer::no_body;
use crate::codegen::mep::{self, CLEARANCE, Fitting, ROD, STRUT, Section};
use crate::codegen::step_writer::{Frame, add, cross, dot, length, normalize, sub};
use crate::geometry::axis::WallAxis;
use crate::scene::graph::{get_point, get_polygon, get_profile, outline_holes};

/// How far an opening reaches past the faces of its host.
const MARGIN: f64 = 0.05;

/// Where an element sits: container index (in [`SemanticModel::containers`]
/// order), space index within it and index in its element list.
#[derive(Clone, Copy)]
struct Loc {
    container: usize,
    space: Option<usize>,
    index: usize,
}

/// An element with its location, building and storey elevation.
#[derive(Clone, Copy)]
struct Placed<'a> {
    elem: &'a ResolvedElement,
    loc: Loc,
    building: *const (),
    elevation: f64,
}

/// Axis-aligned bounding box.
type Bounds = ([f64; 3], [f64; 3]);

fn overlaps(a: &Bounds, b: &Bounds) -> bool {
    (0..3).all(|i| a.0[i] <= b.1[i] && b.0[i] <= a.1[i])
}

/// A straight run with its centreline in building coordinates.
struct Run<'a> {
    at: Placed<'a>,
    /// Cuts openings (not `openings: false`).
    cuts: bool,
    a: [f64; 3],
    b: [f64; 3],
    section: Section,
    frame: Frame,
    clearance: f64,
    bounds: Bounds,
}

/// Body of a host: the slab between two parallel planes (normal `n`, plane
/// offsets `lo..hi`) and the test that a point on it lies within the host.
struct Host<'a> {
    at: Placed<'a>,
    /// Index of the chord along a curved wall (0 otherwise).
    chord: usize,
    n: [f64; 3],
    lo: f64,
    hi: f64,
    bounds: Bounds,
    kind: HostKind,
}

enum HostKind {
    /// Straight wall or chord of a curved one: axis start, direction,
    /// length, z range.
    Wall {
        o: [f64; 3],
        dir: [f64; 3],
        len: f64,
        z: (f64, f64),
    },
    /// Slab outline and the holes a run may pass through, with the id of
    /// the `Opening` that makes each (empty for `holes:`).
    /// `carries` is false for a covering, which hangers and brackets do not
    /// fix to.
    Slab {
        outline: Vec<(f64, f64)>,
        holes: Vec<(String, Vec<(f64, f64)>)>,
        carries: bool,
    },
}

pub(super) fn resolve(model: &mut SemanticModel) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let bracketed: HashMap<&str, Span> = model
        .elements()
        .filter(|e| {
            e.element_type == ElementType::Wall && boolean(&e.properties, "brackets") == Some(true)
        })
        .map(|e| (e.id.as_str(), e.span))
        .collect();
    if bracketed.is_empty() && !model.elements().any(|e| mep::is_run(e.element_type)) {
        return diagnostics;
    }
    let mut ids: HashSet<String> = model
        .elements()
        .flat_map(|e| std::iter::once(e).chain(&e.children))
        .map(|e| e.id.clone())
        .collect();
    let mut runs = Vec::new();
    let mut hosts = Vec::new();
    let mut mullions = Vec::new();
    for (container, c) in model.containers().enumerate() {
        let building = c.building.map_or(c.site as *const _ as *const (), |b| {
            b as *const _ as *const ()
        });
        let elevation = c.elevation();
        let lists = std::iter::once((None, c.elements)).chain(
            c.spaces
                .iter()
                .enumerate()
                .map(|(i, s)| (Some(i), &s.elements[..])),
        );
        for (space, list) in lists {
            for (index, elem) in list.iter().enumerate() {
                let at = Placed {
                    elem,
                    loc: Loc {
                        container,
                        space,
                        index,
                    },
                    building,
                    elevation,
                };
                if let Some(r) = run(at) {
                    runs.push(r);
                } else if let Some(h) = host(at) {
                    hosts.extend(h);
                } else if elem.element_type == ElementType::Member
                    && string_or_ident(&elem.properties, "part_of")
                        .is_some_and(|w| bracketed.contains_key(w))
                {
                    mullions.push(at);
                }
            }
        }
    }

    // Sorted by container below, for the single pass that inserts them.
    // Sleeves and hangers are added to the list of their run.
    let mut openings: Vec<(Loc, ResolvedElement)> = Vec::new();
    let mut added: Vec<(Loc, ResolvedElement)> = Vec::new();
    // Holes of the generated slab openings, so no hanger hangs from them.
    let mut cut_holes = Vec::new();
    for run in runs.iter().filter(|r| r.cuts) {
        // Per host element: last chord cut and crossings so far. Cuts on adjacent
        // chords of a curved wall are one crossing; later ones get `-2`, `-3`, ...
        let mut crossings: HashMap<*const ResolvedElement, (usize, usize)> = HashMap::new();
        let crossed = hosts
            .iter()
            .enumerate()
            .filter(|(_, h)| h.at.building == run.at.building && overlaps(&h.bounds, &run.bounds));
        for (i, host) in crossed {
            let id = format!("{}-{}", run.at.elem.id, host.at.elem.id);
            let Some((opening, sleeve)) = cut(run, host, id.clone()) else {
                continue;
            };
            let (last, n) = crossings
                .entry(host.at.elem as *const _)
                .or_insert((usize::MAX, 0));
            let adjacent = last.checked_add(1) == Some(host.chord);
            *last = host.chord;
            if adjacent {
                continue;
            }
            *n += 1;
            let (opening, sleeve) = if *n == 1 {
                (opening, sleeve)
            } else {
                cut(run, host, format!("{id}-{n}")).expect("cut above")
            };
            if let Some(b) = get_polygon(&opening.properties, "boundary") {
                cut_holes.push((i, opening.id.clone(), b));
            }
            if ids.insert(opening.id.clone()) {
                openings.push((host.at.loc, opening));
            }
            if let Some(s) = sleeve.filter(|s| ids.insert(s.id.clone())) {
                added.push((run.at.loc, s));
            }
        }
    }
    for (i, id, hole) in cut_holes {
        if let HostKind::Slab { holes, .. } = &mut hosts[i].kind {
            holes.push((id, hole));
        }
    }
    for run in &runs {
        let Some(spacing) = number(&run.at.elem.properties, "hangers").filter(|s| *s > 0.0) else {
            continue;
        };
        for h in hangers(run, spacing, &hosts, &runs, &mut diagnostics) {
            if ids.insert(h.id.clone()) {
                added.push((run.at.loc, h));
            }
        }
    }
    let mut bracketed = bracketed;
    // Walls, then each bracket placed, by building.
    let mut placed: Vec<(*const (), Obb)> = hosts
        .iter()
        .filter_map(|h| Some((h.at.building, Obb::of_wall(h)?)))
        .collect();
    for m in mullions {
        let made = brackets(m, &hosts, &placed);
        if !made.is_empty()
            && let Some(w) = string_or_ident(&m.elem.properties, "part_of")
        {
            bracketed.remove(w);
        }
        for (b, body) in made {
            if ids.insert(b.id.clone()) {
                placed.push((m.building, body));
                added.push((m.loc, b));
            }
        }
    }
    for (w, span) in bracketed {
        diagnostics.push(Diagnostic::warning(
            DiagnosticCode::E201,
            format!(
                "`brackets: true` on `{w}` made no bracket: no vertical mullion passes a slab \
                 edge within {:.2} m of its back face, or each bracket would cut into a wall or \
                 another bracket",
                mep::BRACKET[1] / 2.0
            ),
            span,
        ));
    }
    if openings.is_empty() && added.is_empty() {
        return diagnostics;
    }
    openings.sort_by_key(|o| o.0.container);
    added.sort_by_key(|s| s.0.container);

    let mut openings = openings.into_iter().peekable();
    let mut added = added.into_iter().peekable();
    for (container, (_, spaces, elements)) in model.containers_mut().enumerate() {
        // Openings first: the rest is appended, so host indices stay valid.
        while let Some((loc, opening)) = openings.next_if(|o| o.0.container == container) {
            let list = match loc.space {
                Some(i) => &mut spaces[i].elements,
                None => &mut *elements,
            };
            list[loc.index].children.push(opening);
        }
        while let Some((loc, elem)) = added.next_if(|s| s.0.container == container) {
            match loc.space {
                Some(i) => spaces[i].elements.push(elem),
                None => elements.push(elem),
            }
        }
    }
    diagnostics
}

/// A straight duct, pipe or cable tray segment; a curved one (`path:`, from a
/// smooth run) cuts nothing, like a bend.
fn run(at: Placed) -> Option<Run> {
    let elem = at.elem;
    let p = &elem.properties;
    if !mep::is_run(elem.element_type) || mep::fitting(p) != Fitting::Segment || no_body(elem) {
        return None;
    }
    let (a, b) = mep::axis3(p)?;
    let lift = |q: [f64; 3]| [q[0], q[1], q[2] + at.elevation];
    let (a, b) = (lift(a), lift(b));
    let section = Section::of(elem.element_type, p);
    let frame = Frame::along(a, sub(b, a), mep::degrees(p, "rotation"));
    // A non-positive clearance is reported by `mep::resolve`.
    let clearance = number(p, "clearance")
        .filter(|c| *c > 0.0)
        .unwrap_or(CLEARANCE);
    let unit = |i: usize| [0, 1, 2].map(|j| if i == j { 1.0 } else { 0.0 });
    let r = [0, 1, 2].map(|i| section.half_extent(&frame, unit(i)) + clearance);
    let bounds = (
        [0, 1, 2].map(|i| a[i].min(b[i]) - r[i]),
        [0, 1, 2].map(|i| a[i].max(b[i]) + r[i]),
    );
    Some(Run {
        at,
        cuts: boolean(p, "openings") != Some(false),
        a,
        b,
        section,
        frame,
        clearance,
        bounds,
    })
}

/// The bodies of a wall (one per chord of a curved axis), or of a slab or
/// covering.
fn host(at: Placed) -> Option<Vec<Host>> {
    let p = &at.elem.properties;
    if no_body(at.elem) || is_grid(at.elem) {
        return None;
    }
    let base = at.elevation + number(p, "z_offset").unwrap_or(0.0);
    match at.elem.element_type {
        ElementType::Wall => {
            let axis = WallAxis::of(at.elem, crate::config::default_curve_tolerance())?;
            let height = number(p, "height")?;
            let half = number(p, "thickness").unwrap_or(0.2) / 2.0;
            let z = (base, base + height);
            let pts = &axis.points;
            let hosts: Vec<Host> = if axis.is_straight() {
                wall_host(at, 0, [pts[0], pts[pts.len() - 1]], half, z)
                    .into_iter()
                    .collect()
            } else {
                pts.windows(2)
                    .enumerate()
                    .filter_map(|(i, w)| wall_host(at, i, [w[0], w[1]], half, z))
                    .collect()
            };
            (!hosts.is_empty()).then_some(hosts)
        }
        ElementType::Slab | ElementType::Covering => {
            let outline = get_polygon(p, "boundary").filter(|b| b.len() >= 3)?;
            let mut holes: Vec<_> = outline_holes(p)
                .into_iter()
                .map(|h| (String::new(), h))
                .collect();
            holes.extend(
                at.elem
                    .children
                    .iter()
                    .filter(|c| c.element_type == ElementType::Opening)
                    .filter_map(|c| Some((c.id.clone(), get_polygon(&c.properties, "boundary")?))),
            );
            // A slab hangs down from `z_offset`, a covering stands on it.
            let carries = at.elem.element_type == ElementType::Slab;
            let (lo, hi) = if carries {
                (base - number(p, "thickness").unwrap_or(0.2), base)
            } else {
                let t = number(p, "thickness").unwrap_or(geometry::COVERING_THICKNESS);
                (base, base + t)
            };
            let (mut min, mut max) = (
                [f64::INFINITY, f64::INFINITY, lo],
                [f64::NEG_INFINITY, f64::NEG_INFINITY, hi],
            );
            for &(x, y) in &outline {
                (min[0], min[1]) = (min[0].min(x), min[1].min(y));
                (max[0], max[1]) = (max[0].max(x), max[1].max(y));
            }
            Some(vec![Host {
                chord: 0,
                at,
                n: Z,
                lo,
                hi,
                bounds: (min, max),
                kind: HostKind::Slab {
                    outline,
                    holes,
                    carries,
                },
            }])
        }
        _ => None,
    }
}

/// Body of the straight wall piece `from`..`to`, `half` its thickness to
/// either side, over `z`.
fn wall_host(
    at: Placed,
    chord: usize,
    [from, to]: [[f64; 2]; 2],
    half: f64,
    z: (f64, f64),
) -> Option<Host> {
    let o = [from[0], from[1], 0.0];
    let d = sub([to[0], to[1], 0.0], o);
    let len = length(d);
    if len < 1e-9 {
        return None;
    }
    let dir = normalize(d);
    let n = [-dir[1], dir[0], 0.0];
    let s = dot(o, n);
    let bounds = (
        [from[0].min(to[0]) - half, from[1].min(to[1]) - half, z.0],
        [from[0].max(to[0]) + half, from[1].max(to[1]) + half, z.1],
    );
    Some(Host {
        at,
        chord,
        n,
        lo: s - half,
        hi: s + half,
        bounds,
        kind: HostKind::Wall { o, dir, len, z },
    })
}

/// The opening (and sleeve) where `run` crosses `host`, if it does.
fn cut(run: &Run, host: &Host, id: String) -> Option<(ResolvedElement, Option<ResolvedElement>)> {
    let (a, b) = (run.a, run.b);
    // Clip the centreline to the host's slab of space.
    let (sa, sb) = (dot(a, host.n), dot(b, host.n));
    if (sb - sa).abs() < 1e-9 {
        return None;
    }
    let t = |s: f64| ((s - sa) / (sb - sa)).clamp(0.0, 1.0);
    let (t0, t1) = (t(host.lo), t(host.hi));
    let (t0, t1) = (t0.min(t1), t0.max(t1));
    if t1 - t0 < 1e-9 {
        return None;
    }
    let d = sub(b, a);
    let (p0, p1) = (add(a, d, t0), add(a, d, t1));
    let mid = add(a, d, (t0 + t1) / 2.0);

    // Extent along `axis` of the run's section where it enters and leaves
    // the host, plus the clearance.
    let range = |axis: [f64; 3]| {
        let h = run.section.half_extent(&run.frame, axis) + run.clearance;
        let (s0, s1) = (dot(p0, axis), dot(p1, axis));
        (s0.min(s1) - h, s0.max(s1) + h)
    };

    let (boundary, z0, z1) = match &host.kind {
        HostKind::Wall { o, dir, len, z } => {
            let start = dot(*o, *dir);
            if !(0.0..=*len).contains(&(dot(mid, *dir) - start)) || !(z.0..=z.1).contains(&mid[2]) {
                return None;
            }
            let (u0, u1) = range(*dir);
            let (z0, z1) = range(Z);
            let l = (host.hi - host.lo) / 2.0 + MARGIN;
            let at = |u: f64, l: f64| {
                let q = add(add(*o, *dir, u - start), host.n, l);
                (q[0], q[1])
            };
            let boundary = vec![at(u0, -l), at(u1, -l), at(u1, l), at(u0, l)];
            (boundary, z0, z1)
        }
        HostKind::Slab { outline, holes, .. } => {
            let inside = |poly: &[(f64, f64)]| point_in_polygon(mid[0], mid[1], poly);
            // The run's own (hand-written) opening is no hole to pass through.
            if !inside(outline) || holes.iter().any(|(i, h)| *i != id && inside(h)) {
                return None;
            }
            let (x0, x1) = range([1.0, 0.0, 0.0]);
            let (y0, y1) = range([0.0, 1.0, 0.0]);
            let boundary = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            (boundary, host.lo - MARGIN, host.hi + MARGIN)
        }
    };

    let p = &run.at.elem.properties;
    let span = run.at.elem.span;
    let prop = |key: &str, value| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let opening = ResolvedElement {
        element_type: ElementType::Opening,
        id: id.clone(),
        properties: vec![
            prop("boundary", ResolvedValue::Polygon(boundary)),
            prop("z_offset", ResolvedValue::Number(z0 - host.at.elevation)),
            prop("height", ResolvedValue::Number(z1 - z0)),
        ],
        children: vec![],
        span,
        curves: Default::default(),
    };

    let sleeve = (boolean(p, "sleeve") == Some(true)).then(|| {
        // Through the host's faces, in the run's storey.
        let lower = |q: [f64; 3]| sub(q, [0.0, 0.0, run.at.elevation]);
        let (s0, s1) = (lower(p0), lower(p1));
        let mut properties: Vec<ResolvedProperty> = p
            .iter()
            .filter(|q| {
                matches!(
                    q.key.as_str(),
                    "diameter" | "width" | "height" | "rotation" | "clearance"
                )
            })
            .cloned()
            .collect();
        properties.extend([
            prop(
                "from",
                ResolvedValue::Axis {
                    from: (s0[0], s0[1]),
                    to: (s1[0], s1[1]),
                    via: None,
                    z: Some((s0[2], s1[2])),
                },
            ),
            prop("fitting", ResolvedValue::Identifier("sleeve".to_string())),
            prop("opening", ResolvedValue::Identifier(id.clone())),
        ]);
        ResolvedElement {
            element_type: run.at.elem.element_type,
            id: format!("{id}-SL"),
            properties,
            children: vec![],
            span,
            curves: Default::default(),
        }
    });
    Some((opening, sleeve))
}

/// Hangers every `spacing` along a horizontal run (sloped no more than 1:20),
/// in the middle of equal parts of it, or moved within its part to the
/// nearest place clear of walls and other runs. Each hangs from the lowest
/// slab above it. None when the building has no slab above the run at all,
/// as in a storey file analyzed alone; a warning when some lack one.
fn hangers(
    run: &Run,
    spacing: f64,
    hosts: &[Host],
    runs: &[Run],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ResolvedElement> {
    let d = sub(run.b, run.a);
    let len = d[0].hypot(d[1]);
    // `spacing` is the largest distance between hangers.
    let n = (len / spacing).ceil() as usize;
    if n == 0 || d[2].abs() > len / 20.0 {
        return vec![];
    }
    let half = run.section.half_extent(&run.frame, Z);
    let top = run.a[2].max(run.b[2]) + half;
    let slabs: Vec<_> = hosts
        .iter()
        .filter_map(|h| match &h.kind {
            HostKind::Slab {
                outline,
                holes,
                carries: true,
            } if h.at.building == run.at.building && h.lo >= top - 1e-6 => {
                Some((h.lo, outline, holes))
            }
            _ => None,
        })
        .collect();
    let across = normalize([-d[1], d[0], 0.0]);
    let width = 2.0 * run.section.half_extent(&run.frame, across);
    let rotation = across[1].atan2(across[0]).to_degrees();
    let elem = run.at.elem;
    let span = elem.span;
    let prop = |key: &str, value| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let soffit = |q: [f64; 3]| {
        slabs
            .iter()
            .filter(|(_, outline, holes)| on_slab(outline, holes, (q[0], q[1])))
            .map(|s| s.0)
            .min_by(f64::total_cmp)
    };
    // Middle of each part and the soffit above it.
    let middles: Vec<_> = (0..n)
        .map(|i| {
            let mid = (i as f64 + 0.5) / n as f64;
            (mid, soffit(add(run.a, d, mid)))
        })
        .collect();
    // Walls and the other runs within reach of the hangers, which they must
    // clear.
    let (rod, strut_half) = mep::hanger_span(width);
    let ceiling = middles.iter().filter_map(|m| m.1).fold(top, f64::max);
    let reach = (
        [
            run.bounds.0[0] - strut_half,
            run.bounds.0[1] - strut_half,
            run.bounds.0[2] - STRUT,
        ],
        [
            run.bounds.1[0] + strut_half,
            run.bounds.1[1] + strut_half,
            ceiling,
        ],
    );
    let near = |b: &Bounds| overlaps(b, &reach);
    let obstacles: Vec<Obb> = hosts
        .iter()
        .filter(|h| h.at.building == run.at.building && near(&h.bounds))
        .filter_map(Obb::of_wall)
        .chain(
            runs.iter()
                .filter(|r| {
                    r.at.building == run.at.building
                        && !std::ptr::eq(r.at.elem, elem)
                        && near(&r.bounds)
                })
                .map(Obb::of_run),
        )
        .collect();
    let along = normalize([d[0], d[1], 0.0]);
    // Strut and rods of a hanger under the run at `q`, up to `soffit`.
    let parts = |q: [f64; 3], soffit: f64| {
        let bottom = q[2] - half - STRUT;
        let rod_at = |s: f64| Obb {
            c: [
                q[0] + across[0] * s,
                q[1] + across[1] * s,
                (bottom + soffit) / 2.0,
            ],
            axes: [along, across, Z],
            half: [ROD / 2.0, ROD / 2.0, (soffit - bottom) / 2.0],
        };
        [
            Obb {
                c: [q[0], q[1], bottom + STRUT / 2.0],
                axes: [along, across, Z],
                half: [STRUT / 2.0, strut_half, STRUT / 2.0],
            },
            rod_at(-rod),
            rod_at(rod),
        ]
    };
    let clear = |q: [f64; 3], soffit: f64| {
        parts(q, soffit)
            .iter()
            .all(|p| obstacles.iter().all(|o| !p.overlaps(o)))
    };
    // Offsets from the middle of a part in `NUDGE` steps, nearest first, up
    // to a quarter of the part, so neighbours stay within 1.5 parts.
    let steps = (len / n as f64 / 4.0 / NUDGE).floor() as i32;
    let mut out = Vec::new();
    for (i, &(mid, above)) in middles.iter().enumerate() {
        let Some(above) = above else {
            continue;
        };
        // Only under the same soffit, never through a hole to a higher slab.
        let q = std::iter::once(0)
            .chain((1..=steps).flat_map(|k| [k, -k]))
            .map(|k| add(run.a, d, mid + k as f64 * NUDGE / len))
            .find(|&q| soffit(q).is_some_and(|s| (s - above).abs() < 1e-9) && clear(q, above))
            .unwrap_or_else(|| add(run.a, d, mid));
        let bottom = q[2] - half;
        out.push(ResolvedElement {
            element_type: ElementType::Accessory,
            id: format!("{}-H{}", elem.id, i + 1),
            properties: vec![
                prop("type", ResolvedValue::Identifier("hanger".to_string())),
                prop("position", ResolvedValue::Point(q[0], q[1])),
                prop("z_offset", ResolvedValue::Number(bottom - run.at.elevation)),
                prop("width", ResolvedValue::Number(width)),
                prop("height", ResolvedValue::Number(above - bottom)),
                prop("rotation", ResolvedValue::Number(rotation)),
            ],
            children: vec![],
            span,
            curves: Default::default(),
        });
    }
    // No hanger found a slab: the slab above is likely in another file.
    if !out.is_empty() && out.len() < n {
        diagnostics.push(Diagnostic::warning(
            DiagnosticCode::E201,
            format!(
                "no slab above {} of the {n} hangers of `{}`; left out",
                n - out.len(),
                elem.id
            ),
            span,
        ));
    }
    out
}

/// Step by which a hanger moves away from the middle of its part.
const NUDGE: f64 = 0.05;

const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// Oriented box: centre, unit axes and half extents along them.
struct Obb {
    c: [f64; 3],
    axes: [[f64; 3]; 3],
    half: [f64; 3],
}

impl Obb {
    /// Body of a straight wall; none for a slab or covering.
    fn of_wall(h: &Host) -> Option<Obb> {
        let HostKind::Wall { o, dir, len, z } = h.kind else {
            return None;
        };
        Some(Obb {
            c: [
                o[0] + dir[0] * len / 2.0,
                o[1] + dir[1] * len / 2.0,
                (z.0 + z.1) / 2.0,
            ],
            axes: [dir, h.n, Z],
            half: [len / 2.0, (h.hi - h.lo) / 2.0, (z.1 - z.0) / 2.0],
        })
    }

    /// Body of a straight run (a round one as its square hull).
    fn of_run(r: &Run) -> Obb {
        let (x, y) = (r.frame.x, r.frame.y());
        Obb {
            c: add(r.a, sub(r.b, r.a), 0.5),
            axes: [r.frame.z, x, y],
            half: [
                length(sub(r.b, r.a)) / 2.0,
                r.section.half_extent(&r.frame, x),
                r.section.half_extent(&r.frame, y),
            ],
        }
    }

    /// Separating axis test; boxes that only touch do not overlap.
    fn overlaps(&self, o: &Obb) -> bool {
        let radius = |b: &Obb, l: [f64; 3]| {
            (0..3)
                .map(|i| dot(l, b.axes[i]).abs() * b.half[i])
                .sum::<f64>()
        };
        let t = sub(o.c, self.c);
        let crossed = self
            .axes
            .iter()
            .flat_map(|a| o.axes.iter().map(|b| cross(*a, *b)))
            .filter(|l| dot(*l, *l) > 1e-12)
            .map(normalize);
        !self
            .axes
            .iter()
            .chain(&o.axes)
            .copied()
            .chain(crossed)
            .any(|l| dot(t, l).abs() >= radius(self, l) + radius(o, l) - 1e-4)
    }
}

/// Whether `q` lies on a slab: in its outline and none of its holes.
fn on_slab(outline: &[(f64, f64)], holes: &[(String, Vec<(f64, f64)>)], q: (f64, f64)) -> bool {
    point_in_polygon(q.0, q.1, outline) && !holes.iter().any(|(_, h)| point_in_polygon(q.0, q.1, h))
}

/// Brackets of a vertical mullion: one at each slab of its building whose
/// edge it passes, on the slab top or, where the mullion ends within the
/// slab, under its soffit. Each sits against the mullion on the slab side
/// and connects the slab and the mullion. One that would cut into a body in
/// `placed` (a wall, or a bracket at a corner of two curtain walls) is left out;
/// the caller adds the body of each bracket it keeps.
fn brackets(m: Placed, hosts: &[Host], placed: &[(*const (), Obb)]) -> Vec<(ResolvedElement, Obb)> {
    const TOL: f64 = 0.05;
    let p = &m.elem.properties;
    // A vertical axis, or `position` and `height`, both above `z_offset`.
    let (from, (z0, z1)) = match p.iter().find(|q| q.key == "from").map(|q| &q.value) {
        Some(ResolvedValue::Axis {
            from,
            to,
            z: Some(z),
            ..
        }) if from == to => (*from, *z),
        Some(_) => return vec![],
        None => match (get_point(p, "position"), number(p, "height")) {
            (Some(at), Some(h)) => (at, (0.0, h)),
            _ => return vec![],
        },
    };
    let base = m.elevation + number(p, "z_offset").unwrap_or(0.0);
    let (z0, z1) = (z0 + base, z1 + base);
    let depth = get_profile(p, "profile")
        .and_then(|(_, params)| params.get(1).copied())
        .unwrap_or(MULLION.1);
    let rotation = number(p, "rotation").unwrap_or(0.0);
    let [w, d, h] = mep::BRACKET;
    let a = rotation.to_radians();
    let n = (-a.sin(), a.cos());
    let reach = (depth + d) / 2.0;
    let span = m.elem.span;
    let prop = |key: &str, value| ResolvedProperty {
        key: key.to_string(),
        value,
        span,
    };
    let mut out = Vec::new();
    for host in hosts.iter().filter(|h| h.at.building == m.building) {
        let HostKind::Slab {
            outline,
            holes,
            carries: true,
        } = &host.kind
        else {
            continue;
        };
        let near = (0..2).all(|i| {
            let c = [from.0, from.1][i];
            host.bounds.0[i] - reach <= c && c <= host.bounds.1[i] + reach
        });
        if !near || host.lo > z1 + TOL || host.hi < z0 - TOL {
            continue;
        }
        let Some(q) = [1.0, -1.0]
            .map(|side| (from.0 + side * reach * n.0, from.1 + side * reach * n.1))
            .into_iter()
            .find(|q| on_slab(outline, holes, *q))
        else {
            continue;
        };
        let z = if host.hi + h <= z1 + TOL {
            host.hi
        } else if host.lo - h >= z0 - TOL {
            host.lo - h
        } else {
            continue;
        };
        let body = Obb {
            c: [q.0, q.1, z + h / 2.0],
            axes: [[n.1, -n.0, 0.0], [n.0, n.1, 0.0], Z],
            half: [w / 2.0, d / 2.0, h / 2.0],
        };
        if placed
            .iter()
            .any(|(b, o)| *b == m.building && body.overlaps(o))
        {
            continue;
        }
        let slab = &host.at.elem.id;
        out.push((
            ResolvedElement {
                element_type: ElementType::Accessory,
                id: format!("{}-{slab}", m.elem.id),
                properties: vec![
                    prop("type", ResolvedValue::Identifier("bracket".to_string())),
                    prop("position", ResolvedValue::Point(q.0, q.1)),
                    prop("z_offset", ResolvedValue::Number(z - m.elevation)),
                    prop("rotation", ResolvedValue::Number(rotation)),
                    prop(
                        "connects",
                        ResolvedValue::EnumList(vec![slab.clone(), m.elem.id.clone()]),
                    ),
                ],
                children: vec![],
                span,
                curves: Default::default(),
            },
            body,
        ));
    }
    out
}
