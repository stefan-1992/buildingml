//! Fit decompiled elements to the meshes of their source IFC.
//!
//! The reader recovers parametric geometry (wall axes, extruded profiles) where
//! the IFC uses it. Anything else - breps, sloped roofs, mapped furniture,
//! curtain wall panels - would come back wrong or not at all. This pass meshes
//! the scene the BML compiles to, compares each element's bounding box with
//! the IFC mesh, and replaces the geometry of every element that disagrees by
//! a prism: its footprint outline extruded between the mesh's z range.

use std::collections::{HashMap, HashSet};

use super::ifc_reader::prop;
use crate::codegen::geometry::signed_area;
use crate::scene::{SceneNode, generate_scene};
use crate::semantic::model::{
    ElementType, ResolvedElement, ResolvedProperty, ResolvedSpace, ResolvedValue, SemanticModel,
    contents_mut, number,
};
use crate::semantic::type_check::missing_required;

/// A triangle mesh in world coordinates, metres.
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Bounding boxes that agree within this many metres keep their geometry.
const TOLERANCE: f64 = 0.02;

/// Geometry properties replaced by a fitted prism. `type` stays (a slab's
/// `floor`/`roof` is semantic); a refitted roof drops its own below.
const GEOMETRY: &[&str] = &[
    "from",
    "to",
    "via",
    "path",
    "position",
    "profile",
    "height",
    "thickness",
    "width",
    "depth",
    "length",
    "boundary",
    "offset",
    "sill_height",
    "lateral_offset",
    "z_offset",
    "rotation",
    "pitch",
    "ridge_direction",
    "direction",
    "radius",
    "rise",
    "run",
];

type BBox = [f64; 6]; // xmin, ymin, zmin, xmax, ymax, zmax

/// Give every element whose required geometry is missing
/// `representation: none`, for decompiling without meshes.
pub fn drop_all_incomplete(model: &mut SemanticModel) {
    for e in model.elements_mut() {
        drop_incomplete(e, true);
    }
}

/// Fit every element and space carrying a `guid:` found in `meshes` (keyed by
/// GlobalId). Returns the number of elements whose geometry was replaced.
pub fn fit_meshes(model: &mut SemanticModel, meshes: &HashMap<String, Mesh>) -> usize {
    for (elev, spaces, _) in model.containers_mut() {
        for space in spaces.iter_mut() {
            if let Some(mesh) = guid(&space.properties).and_then(|g| meshes.get(g))
                && let Some(bb) = bbox(mesh.positions.iter().copied())
            {
                // A space keeps the exact B-spline outline the reader gave it
                // where that outline matches its mesh.
                if space.curve("boundary").is_some() && exact_fits(space, elev, &bb) {
                    continue;
                }
                space.curves.clear();
                space
                    .properties
                    .retain(|p| p.key != "representation" && !GEOMETRY.contains(&p.key.as_str()));
                space.properties.push(prop(
                    "boundary",
                    ResolvedValue::Polygon(footprint(mesh, &bb)),
                ));
                space.properties.push(num("z_offset", bb[2] - elev));
                space.properties.push(num("height", bb[5] - bb[2]));
            }
        }
    }
    // Each pass compiles the scene and refits what disagrees with its mesh: a
    // wall or beam first gets an axis, and a prism if the axis did not match.
    // Refitted hosts also move their wall-relative fillers.
    let mut tried = HashSet::new();
    for _ in 0..4 {
        let boxes = scene_boxes(model);
        let mut fit = Fit {
            meshes,
            boxes: &boxes,
            tried: &mut tried,
            changed: false,
        };
        for (elev, spaces, elements) in model.containers_mut() {
            for e in contents_mut(spaces, elements) {
                fit.tree(e, elev, true);
            }
        }
        if !fit.changed {
            break;
        }
    }
    drop_all_incomplete(model);
    tried.len()
}

/// An element neither the reader nor a mesh gave its required geometry (the
/// mesher could not process its body) is kept without one. Outside a host
/// (`top`), a window or door has a body only through `boundary`.
fn drop_incomplete(elem: &mut ResolvedElement, top: bool) {
    let keys: Vec<&str> = elem.properties.iter().map(|p| p.key.as_str()).collect();
    let flat = elem.properties.iter().any(|p| {
        p.key == "type" && matches!(&p.value, ResolvedValue::Identifier(t) if t == "flat")
    });
    // A covering or footing without `boundary` would need a space or walls to
    // take one from.
    let unhosted = (top && matches!(elem.element_type, ElementType::Window | ElementType::Door)
        || matches!(
            elem.element_type,
            ElementType::Covering | ElementType::Footing
        ))
        && !keys.contains(&"boundary");
    if unhosted || !missing_required(elem.element_type, &keys, flat).is_empty() {
        elem.properties
            .retain(|p| p.key != "representation" && !GEOMETRY.contains(&p.key.as_str()));
        elem.curves.clear();
        elem.properties.push(prop(
            "representation",
            ResolvedValue::Identifier("none".to_string()),
        ));
    }
    for c in &mut elem.children {
        drop_incomplete(c, false);
    }
}

struct Fit<'a> {
    meshes: &'a HashMap<String, Mesh>,
    boxes: &'a HashMap<String, BBox>,
    /// Elements refitted so far; a second refit falls back to a prism.
    tried: &'a mut HashSet<String>,
    changed: bool,
}

impl Fit<'_> {
    fn tree(&mut self, elem: &mut ResolvedElement, elev: f64, top: bool) {
        if !elem.properties.iter().any(|p| p.key == "representation")
            && let Some(mesh) = guid(&elem.properties).and_then(|g| self.meshes.get(g))
            && let Some(bb) = bbox(mesh.positions.iter().copied())
            && !self.boxes.get(&elem.id).is_some_and(|s| close(s, &bb))
        {
            let first = self.tried.insert(elem.id.clone());
            elem.properties
                .retain(|p| !GEOMETRY.contains(&p.key.as_str()));
            elem.curves.clear();
            let fp = footprint(mesh, &bb);
            if !(first && top && axis_fit(elem, mesh, &fp, &bb, elev)) {
                prism(elem, fp, &bb, elev, top);
            }
            self.changed = true;
        }
        for c in &mut elem.children {
            self.tree(c, elev, false);
        }
    }
}

/// Axis geometry for a straight wall, beam or member, from its mesh.
/// False when the element type has none or the mesh is not shaped like one.
fn axis_fit(
    elem: &mut ResolvedElement,
    mesh: &Mesh,
    fp: &[(f64, f64)],
    bb: &BBox,
    elev: f64,
) -> bool {
    let props = &mut elem.properties;
    match elem.element_type {
        ElementType::Wall => {
            let xy: Vec<[f32; 2]> = mesh.positions.iter().map(|p| [p[0], p[1]]).collect();
            let Some((from, to, t, None)) = wall_axis(&xy) else {
                return false;
            };
            // An L-shaped or stepped wall is no rectangle: compare plan areas.
            let len = ((to[0] - from[0]) as f64).hypot((to[1] - from[1]) as f64);
            if (signed_area(fp) - len * t as f64).abs() > 0.03 * len * t as f64 {
                return false;
            }
            props.push(axis(from.map(f64::from), to.map(f64::from), None));
            props.push(num("thickness", t as f64));
            props.push(num("height", bb[5] - bb[2]));
            props.push(num("z_offset", bb[2] - elev));
            true
        }
        // An axis-aligned rectangular column; other sections keep their outline.
        ElementType::Column => {
            let (w, d) = (bb[3] - bb[0], bb[4] - bb[1]);
            if (signed_area(fp) - w * d).abs() > 0.03 * w * d {
                return false;
            }
            props.push(prop(
                "position",
                ResolvedValue::Point((bb[0] + bb[3]) / 2.0, (bb[1] + bb[4]) / 2.0),
            ));
            props.push(prop(
                "profile",
                ResolvedValue::Profile {
                    profile_type: "rectangle".into(),
                    params: vec![w, d],
                },
            ));
            props.push(num("height", bb[5] - bb[2]));
            props.push(num("z_offset", bb[2] - elev));
            true
        }
        ElementType::Beam | ElementType::Member => {
            let Some((p0, p1, w, d)) = beam_axis(&mesh.positions) else {
                return false;
            };
            props.push(axis(
                [p0[0], p0[1]],
                [p1[0], p1[1]],
                Some((p0[2] - elev, p1[2] - elev)),
            ));
            props.push(prop(
                "profile",
                ResolvedValue::Profile {
                    profile_type: "rectangle".into(),
                    params: vec![w, d],
                },
            ));
            true
        }
        _ => false,
    }
}

fn axis(from: [f64; 2], to: [f64; 2], z: Option<(f64, f64)>) -> ResolvedProperty {
    prop(
        "from",
        ResolvedValue::Axis {
            from: (from[0], from[1]),
            to: (to[0], to[1]),
            via: None,
            z,
        },
    )
}

/// Centreline end points and section (width across, depth up) of a straight
/// extrusion, by principal component analysis of its vertices. The section
/// frame matches the compiler's: width along the horizontal perpendicular.
fn beam_axis(pts: &[[f32; 3]]) -> Option<([f64; 3], [f64; 3], f64, f64)> {
    let n = pts.len() as f64;
    if pts.len() < 4 {
        return None;
    }
    let mut m = [0.0; 3];
    for p in pts {
        for k in 0..3 {
            m[k] += p[k] as f64 / n;
        }
    }
    let mut cov = [[0.0; 3]; 3];
    for p in pts {
        let d = [p[0] as f64 - m[0], p[1] as f64 - m[1], p[2] as f64 - m[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    // Power iteration for the principal axis.
    let mut a = [1.0, 0.7, 0.3];
    for _ in 0..100 {
        let v = [0, 1, 2].map(|i| cov[i][0] * a[0] + cov[i][1] * a[1] + cov[i][2] * a[2]);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len < 1e-12 {
            return None;
        }
        a = v.map(|x| x / len);
    }
    let h = [-a[1], a[0], 0.0]; // z x axis
    let hl = h[0].hypot(h[1]);
    let x = if hl < 1e-9 {
        [1.0, 0.0, 0.0]
    } else {
        h.map(|c| c / hl)
    };
    let y = [
        a[1] * x[2] - a[2] * x[1],
        a[2] * x[0] - a[0] * x[2],
        a[0] * x[1] - a[1] * x[0],
    ];
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let mut r = [[f64::MAX, f64::MIN]; 3]; // ranges along axis, x, y
    for p in pts {
        let d = [p[0] as f64 - m[0], p[1] as f64 - m[1], p[2] as f64 - m[2]];
        for (k, dir) in [a, x, y].into_iter().enumerate() {
            let t = dot(d, dir);
            r[k] = [r[k][0].min(t), r[k][1].max(t)];
        }
    }
    let (w, d) = (r[1][1] - r[1][0], r[2][1] - r[2][0]);
    if r[0][1] - r[0][0] < w.max(d) {
        return None; // not elongated
    }
    let (cx, cy) = ((r[1][0] + r[1][1]) / 2.0, (r[2][0] + r[2][1]) / 2.0);
    let at = |t: f64| [0, 1, 2].map(|k| m[k] + t * a[k] + cx * x[k] + cy * y[k]);
    Some((at(r[0][0]), at(r[0][1]), w, d))
}

/// Replace an element's geometry by its mesh's footprint prism.
fn prism(elem: &mut ResolvedElement, fp: Vec<(f64, f64)>, bb: &BBox, elev: f64, top: bool) {
    elem.properties
        .push(prop("boundary", ResolvedValue::Polygon(fp)));
    extrusion(
        elem.element_type,
        &mut elem.properties,
        bb[2] - elev,
        bb[5] - elev,
        top,
    );
}

/// Height properties of a `boundary:` prism from storey-relative `lo` to `hi`.
pub(super) fn extrusion(
    et: ElementType,
    props: &mut Vec<ResolvedProperty>,
    lo: f64,
    hi: f64,
    top: bool,
) {
    let depth = hi - lo;
    match et {
        // Slabs, plates and footings hang down from their top face.
        ElementType::Slab | ElementType::Plate if top => {
            props.push(num("z_offset", hi));
            props.push(num("thickness", depth));
        }
        ElementType::Footing => {
            props.push(num("z_offset", hi));
            props.push(num("depth", depth));
        }
        // Coverings stand on their bottom face.
        ElementType::Covering => {
            props.push(num("z_offset", lo));
            props.push(num("thickness", depth));
        }
        ElementType::Roof if top => {
            props.retain(|p| p.key != "type");
            props.push(prop("type", ResolvedValue::Identifier("flat".into())));
            props.push(num("z_offset", lo));
            props.push(num("thickness", depth));
        }
        _ => {
            props.push(num("z_offset", lo));
            props.push(num("height", depth));
        }
    }
}

/// Footprint polygon: the top-face outline when it spans the whole mesh,
/// else the convex hull, else (for a mesh flat in plan) its bounding rectangle.
fn footprint(mesh: &Mesh, bb: &BBox) -> Vec<(f64, f64)> {
    let to64 = |poly: Vec<[f32; 2]>| -> Vec<(f64, f64)> {
        simplify(&poly)
            .iter()
            .map(|p| (p[0] as f64, p[1] as f64))
            .collect()
    };
    let fits = |poly: &[(f64, f64)]| {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(x, y) in poly {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        poly.len() >= 3
            && [x0 - bb[0], y0 - bb[1], x1 - bb[3], y1 - bb[4]]
                .iter()
                .all(|d| d.abs() < TOLERANCE)
    };
    let mut poly = to64(outline(&mesh.positions, &mesh.indices));
    if !fits(&poly) || signed_area(&poly).abs() < 1e-6 {
        let xy: Vec<[f32; 2]> = mesh.positions.iter().map(|p| [p[0], p[1]]).collect();
        poly = to64(convex_hull(&xy));
    }
    if poly.len() >= 3 && signed_area(&poly).abs() >= 1e-6 {
        if signed_area(&poly) < 0.0 {
            poly.reverse();
        }
        poly
    } else {
        // Flat in plan (a vertical panel): its bounding rectangle, 1 mm minimum.
        let (x0, y0) = (bb[0], bb[1]);
        let (x1, y1) = (bb[3].max(x0 + 0.001), bb[4].max(y0 + 0.001));
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }
}

/// Element id -> world bounding box of the scene BML compiles to. Nodes with
/// ids of their own sub-parts (mullions, frames) count toward their parent.
fn scene_boxes(model: &SemanticModel) -> HashMap<String, BBox> {
    fn walk(node: &SceneNode, owner: &str, ids: &HashSet<&str>, out: &mut HashMap<String, BBox>) {
        let owner = if ids.contains(node.id.as_str()) {
            node.id.as_str()
        } else {
            owner
        };
        if let Some(b) = bbox(node.mesh.vertices.iter().map(|v| v.position)) {
            let e = out.entry(owner.to_string()).or_insert(b);
            for k in 0..3 {
                e[k] = e[k].min(b[k]);
                e[k + 3] = e[k + 3].max(b[k + 3]);
            }
        }
        for c in &node.children {
            walk(c, owner, ids, out);
        }
    }
    fn collect<'a>(e: &'a ResolvedElement, ids: &mut HashSet<&'a str>) {
        ids.insert(e.id.as_str());
        for c in &e.children {
            collect(c, ids);
        }
    }
    let mut ids = HashSet::new();
    for e in model.elements() {
        collect(e, &mut ids);
    }
    let mut out = HashMap::new();
    for node in &generate_scene(model).nodes {
        walk(node, &node.id, &ids, &mut out);
    }
    out
}

/// The exact outline and height of `space` span the box `bb` of its mesh.
fn exact_fits(space: &ResolvedSpace, elev: f64, bb: &BBox) -> bool {
    let p = &space.properties;
    let outer = match p.iter().find(|q| q.key == "boundary").map(|q| &q.value) {
        Some(ResolvedValue::Polygon(o) | ResolvedValue::PolygonWithHoles { outer: o, .. }) => o,
        _ => return false,
    };
    let (Some(z), Some(h)) = (number(p, "z_offset"), number(p, "height")) else {
        return false;
    };
    let mut b = [
        f64::MAX,
        f64::MAX,
        elev + z,
        f64::MIN,
        f64::MIN,
        elev + z + h,
    ];
    for &(x, y) in outer {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[3] = b[3].max(x);
        b[4] = b[4].max(y);
    }
    close(&b, bb)
}

fn close(a: &BBox, b: &BBox) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < TOLERANCE)
}

fn bbox(pts: impl Iterator<Item = [f32; 3]>) -> Option<BBox> {
    let mut pts = pts.peekable();
    let first = *pts.peek()?;
    let mut b = [0.0; 6];
    for k in 0..3 {
        b[k] = first[k] as f64;
        b[k + 3] = first[k] as f64;
    }
    for p in pts {
        for k in 0..3 {
            b[k] = b[k].min(p[k] as f64);
            b[k + 3] = b[k + 3].max(p[k] as f64);
        }
    }
    Some(b)
}

/// Drop repeated and collinear vertices.
fn simplify(poly: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = poly.to_vec();
    loop {
        let n = out.len();
        if n < 3 {
            return out;
        }
        let drop = (0..n).find(|&i| {
            let (a, b, c) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
            let cross = (b[0] - a[0]) as f64 * (c[1] - a[1]) as f64
                - (b[1] - a[1]) as f64 * (c[0] - a[0]) as f64;
            let len = ((c[0] - a[0]) as f64).hypot((c[1] - a[1]) as f64);
            cross.abs() <= 1e-4 * len.max(1e-9) || (a == b)
        });
        match drop {
            Some(i) => {
                out.remove(i);
            }
            None => return out,
        }
    }
}

/// Outline of a mesh's footprint: the boundary loop of its top face
/// (preserves concavity), falling back to the convex hull.
pub fn outline(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 2]> {
    let xy_all: Vec<[f32; 2]> = positions.iter().map(|p| [p[0], p[1]]).collect();
    if positions.len() < 3 {
        return convex_hull(&xy_all);
    }
    let zmax = positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
    let eps = 1e-3;
    let ekey = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut edge_count: HashMap<(u32, u32), i32> = HashMap::new();
    for tri in indices.as_chunks::<3>().0 {
        if !tri
            .iter()
            .all(|&i| (positions[i as usize][2] - zmax).abs() < eps)
        {
            continue; // top-face triangles only
        }
        for k in 0..3 {
            *edge_count
                .entry(ekey(tri[k], tri[(k + 1) % 3]))
                .or_insert(0) += 1;
        }
    }
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&(a, b), &c) in &edge_count {
        if c == 1 {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    if adj.is_empty() {
        return convex_hull(&xy_all);
    }
    // Walk boundary loops; keep the longest (outer ring).
    let mut best: Vec<u32> = Vec::new();
    let mut seen = HashSet::new();
    let mut starts: Vec<u32> = adj.keys().copied().collect();
    starts.sort_unstable(); // deterministic output
    for start in starts {
        if seen.contains(&start) {
            continue;
        }
        let mut loop_v = vec![start];
        let (mut prev, mut cur) = (start, adj[&start][0]);
        seen.insert(start);
        while cur != start {
            seen.insert(cur);
            loop_v.push(cur);
            let next = adj[&cur].iter().copied().find(|&nbr| nbr != prev);
            match next {
                Some(nbr) => {
                    prev = cur;
                    cur = nbr;
                }
                None => break,
            }
            if loop_v.len() > edge_count.len() + 1 {
                break; // safety
            }
        }
        if loop_v.len() > best.len() {
            best = loop_v;
        }
    }
    if best.len() < 3 {
        return convex_hull(&xy_all);
    }
    best.iter()
        .map(|&i| [positions[i as usize][0], positions[i as usize][1]])
        .collect()
}

/// `(from, to, thickness, via)` of a wall centreline.
pub type FittedAxis = ([f32; 2], [f32; 2], f32, Option<[f32; 2]>);

/// Wall centreline + thickness via PCA of the footprint points, plus an
/// optional mid `via` point when the wall is noticeably curved/bent (the
/// floorplan Axis supports one via point).
pub fn wall_axis(xy: &[[f32; 2]]) -> Option<FittedAxis> {
    let n = xy.len() as f32;
    let mx = xy.iter().map(|p| p[0]).sum::<f32>() / n;
    let my = xy.iter().map(|p| p[1]).sum::<f32>() / n;
    let (mut a, mut b, mut c) = (0f32, 0f32, 0f32);
    for p in xy {
        let (dx, dy) = (p[0] - mx, p[1] - my);
        a += dx * dx;
        b += dx * dy;
        c += dy * dy;
    }
    let theta = 0.5 * (2.0 * b).atan2(a - c);
    let (ax, ay) = (theta.cos(), theta.sin()); // principal (long) axis
    let (px, py) = (-ay, ax); // perpendicular
    let (mut tmin, mut tmax, mut smin, mut smax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in xy {
        let (dx, dy) = (p[0] - mx, p[1] - my);
        let t = dx * ax + dy * ay;
        let s = dx * px + dy * py;
        tmin = tmin.min(t);
        tmax = tmax.max(t);
        smin = smin.min(s);
        smax = smax.max(s);
    }
    if (tmax - tmin) < 1e-3 {
        return None;
    }
    let sc = (smin + smax) * 0.5;
    let thickness = smax - smin;
    let from = [mx + tmin * ax + sc * px, my + tmin * ay + sc * py];
    let to = [mx + tmax * ax + sc * px, my + tmax * ay + sc * py];

    // Curvature: bin points along the axis, find the bin whose mean offset
    // deviates most from the chord; emit a via point if it exceeds thickness.
    const NB: usize = 7;
    let span_t = (tmax - tmin).max(1e-6);
    let mut sums = [0f32; NB];
    let mut cnts = [0u32; NB];
    for p in xy {
        let (dx, dy) = (p[0] - mx, p[1] - my);
        let t = dx * ax + dy * ay;
        let s = dx * px + dy * py;
        let bi = (((t - tmin) / span_t) * (NB as f32 - 1.0)).round() as usize;
        sums[bi.min(NB - 1)] += s;
        cnts[bi.min(NB - 1)] += 1;
    }
    let mut via = None;
    let mut maxdev = thickness; // only a real bend, not noise
    for bi in 1..NB - 1 {
        if cnts[bi] == 0 {
            continue;
        }
        let ms = sums[bi] / cnts[bi] as f32;
        let dev = (ms - sc).abs();
        if dev > maxdev {
            maxdev = dev;
            let t = tmin + (bi as f32 / (NB as f32 - 1.0)) * span_t;
            via = Some([mx + t * ax + ms * px, my + t * ay + ms * py]);
        }
    }
    Some((from, to, thickness, via))
}

/// Andrew's monotone-chain convex hull of 2D points.
pub fn convex_hull(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut p: Vec<[f32; 2]> = pts.to_vec();
    p.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f32; 2]> = Vec::new();
    for &pt in &p {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    let lower = hull.len() + 1;
    for &pt in p.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    hull.pop();
    hull
}

fn guid(props: &[ResolvedProperty]) -> Option<&str> {
    props
        .iter()
        .find(|p| p.key == "guid")
        .and_then(|p| match &p.value {
            ResolvedValue::String(s) => Some(s.as_str()),
            _ => None,
        })
}

fn num(key: &str, n: f64) -> ResolvedProperty {
    prop(key, ResolvedValue::Number(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    /// Box mesh with corners `lo`/`hi`.
    fn cuboid(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
        let mut positions = Vec::new();
        for i in 0..8 {
            let x = if i & 1 == 0 { lo[0] } else { hi[0] };
            let y = if i & 2 == 0 { lo[1] } else { hi[1] };
            let z = if i & 4 == 0 { lo[2] } else { hi[2] };
            positions.push([x, y, z]);
        }
        let indices = vec![
            0, 2, 1, 1, 2, 3, // bottom
            4, 5, 6, 5, 7, 6, // top
            0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, // y sides
            0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5, // x sides
        ];
        Mesh { positions, indices }
    }

    const SRC: &str = r#"# Site: S
## Building: B
### Storey: GF
  elevation: 1.0 m
  height: 3.0 m

  Wall: W-1
    guid: "0abcdefghijklmnopqrstu"
    from: (0, 0) to: (1, 0)
    thickness: 0.1 m
    height: 1.0 m

  Beam: B-2
    guid: "1abcdefghijklmnopqrstu"
    from: (0, 5) to: (1, 5)

  Slab: S-3
    guid: "2abcdefghijklmnopqrstu"
    boundary: [(0, 0), (1, 0), (1, 1), (0, 1)]
    thickness: 0.1 m

  Proxy: PX-4
    guid: "3abcdefghijklmnopqrstu"
    position: (0, 0)
"#;

    fn get<'a>(model: &'a SemanticModel, id: &str) -> &'a ResolvedElement {
        model.sites[0].buildings[0].storeys[0]
            .elements
            .iter()
            .find(|e| e.id == id)
            .unwrap()
    }

    #[test]
    fn exact_space_outline_is_kept_only_where_it_matches_its_mesh() {
        let src = r#"# Site: S
## Building: B
### Storey: GF
  elevation: 1.0 m
  height: 3.0 m

  #### Space: R
    guid: "4abcdefghijklmnopqrstu"
    boundary: [(0, 0), (4, 0), (4, 2), smooth (2, 3), (0, 2)]
    z_offset: 0 m
    height: 2.5 m
"#;
        let fitted = |lo: [f32; 3], hi: [f32; 3]| {
            let (mut model, _) = semantic::analyze(&cst_to_ast::parse(src).unwrap());
            let meshes: HashMap<String, Mesh> =
                [("4abcdefghijklmnopqrstu".to_string(), cuboid(lo, hi))].into();
            fit_meshes(&mut model, &meshes);
            let space = &model.sites[0].buildings[0].storeys[0].spaces[0];
            space.curve("boundary").is_some()
        };
        assert!(fitted([0.0, 0.0, 1.0], [4.0, 3.0, 3.5]));
        // The same outline moved by 2 m.
        assert!(!fitted([2.0, 0.0, 1.0], [6.0, 3.0, 3.5]));
    }

    #[test]
    fn fits_axes_and_prisms_to_meshes() {
        let (mut model, _) = semantic::analyze(&cst_to_ast::parse(SRC).unwrap());
        let meshes: HashMap<String, Mesh> = [
            ("0", cuboid([0.0, -0.1, 1.0], [4.0, 0.1, 3.5])),
            ("1", cuboid([0.0, 4.9, 2.0], [6.0, 5.1, 2.4])),
            ("2", cuboid([0.0, 0.0, 0.8], [3.0, 2.0, 1.0])),
            ("3", cuboid([1.0, 1.0, 1.0], [1.5, 2.0, 2.0])),
        ]
        .into_iter()
        .map(|(g, m)| (format!("{g}abcdefghijklmnopqrstu"), m))
        .collect();

        assert_eq!(fit_meshes(&mut model, &meshes), 4);

        let has = |e: &ResolvedElement, key: &str| e.properties.iter().any(|p| p.key == key);
        let wall = get(&model, "W-1");
        assert!(
            has(wall, "from") && !has(wall, "boundary"),
            "wall keeps an axis"
        );
        assert_eq!(
            crate::scene::graph::get_number(&wall.properties, "thickness")
                .map(|t| (t * 1e3).round()),
            Some(200.0)
        );
        let beam = get(&model, "B-2");
        assert!(
            beam.properties
                .iter()
                .any(|p| matches!(p.value, ResolvedValue::Axis { z: Some(_), .. }))
        );
        assert!(has(get(&model, "S-3"), "boundary"));
        assert!(has(get(&model, "PX-4"), "boundary"));

        // The compiled scene now matches every mesh.
        let boxes = scene_boxes(&model);
        for (g, id) in [("0", "W-1"), ("1", "B-2"), ("2", "S-3"), ("3", "PX-4")] {
            let mesh = &meshes[&format!("{g}abcdefghijklmnopqrstu")];
            let (want, got) = (bbox(mesh.positions.iter().copied()).unwrap(), boxes[id]);
            assert!(close(&want, &got), "{id}: {want:?} vs {got:?}");
        }
    }

    #[test]
    fn simplify_drops_collinear_points() {
        let square = [
            [0.0, 0.0],
            [0.5, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];
        assert_eq!(simplify(&square).len(), 4);
    }
}
