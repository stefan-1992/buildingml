//! `ifc-scene` — generate a 3D SceneGraph directly from an IFC file using the
//! native ifc-lite reader (parse + mesh). Produces the same SceneGraph JSON
//! the viewers consume.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use bml_core::config::default_curve_tolerance;
use bml_core::decompile::mesh_fit::{outline, wall_axis};
use bml_core::geometry::curve::{Curve, CurveError, Nurbs, Segment, expand_knots};
use bml_core::scene::{SceneGraph, SceneMaterial, SceneMesh, SceneNode, Vertex, appearance};
use bml_core::semantic::model::ElementType;
use bml_core::semantic::registry::ELEMENT_REGISTRY;
use ifc_lite_core::{
    EntityDecoder, EntityIndex, EntityScanner, build_entity_index, has_geometry_by_name,
};
use ifc_lite_geometry::GeometryRouter;

use super::ifc_properties::PropertyIndex;
use super::ifc_styles::{NAME_SEP, StyleIndex};

/// Product types that can carry geometry but are not drawn: spatial containers
/// (a space volume would hide its contents), voids, ports and annotations.
/// Structural analysis items (`IfcStructural*`) are skipped by prefix.
const SKIPPED: &[&str] = &[
    "IFCSITE",
    "IFCBUILDING",
    "IFCSPACE",
    "IFCSPATIALZONE",
    "IFCEXTERNALSPATIALELEMENT",
    "IFCOPENINGELEMENT",
    "IFCOPENINGSTANDARDCASE",
    "IFCVOIDINGFEATURE",
    "IFCVIRTUALELEMENT",
    "IFCDISTRIBUTIONPORT",
    "IFCANNOTATION",
    "IFCGRID",
];

/// Whether elements of this IFC class are meshed: every `IfcProduct` subtype
/// that can carry geometry, which includes MEP classes such as
/// `IfcPipeSegment` or `IfcCableCarrierFitting`, minus [`SKIPPED`].
fn is_rendered(t: &str) -> bool {
    has_geometry_by_name(t) && !SKIPPED.contains(&t) && !t.starts_with("IFCSTRUCTURAL")
}

/// Strip the IFC4 `STANDARDCASE` / `ELEMENTEDCASE` suffix, e.g.
/// `IFCSLABSTANDARDCASE` -> `IFCSLAB`, so subtypes share their parent's handling.
fn base_type(t: &str) -> &str {
    t.strip_suffix("STANDARDCASE")
        .or_else(|| t.strip_suffix("ELEMENTEDCASE"))
        .unwrap_or(t)
}

/// Metres per file length unit of the `IfcProject` `project`, seeded into
/// `decoder`. ifc-lite's `length_unit_scale()` answers 1.0 for
/// conversion-based units such as FOOT (`ifc_bench_models/4351`), so the full
/// unit chain is resolved here.
pub(crate) fn length_unit_scale(project: Option<u32>, decoder: &mut EntityDecoder) -> f64 {
    let scale = match project {
        Some(id) => ifc_lite_core::units::extract_length_unit_scale(decoder, id)
            .unwrap_or_else(|_| decoder.length_unit_scale()),
        None => decoder.length_unit_scale(),
    };
    let angle = decoder.plane_angle_to_radians();
    decoder.seed_unit_scales(scale, angle);
    scale
}

/// `content` with dangling references unset (see [`null_dangling_refs`]) and
/// B-spline curves sampled (see [`bsplines_to_polycurves`]), and its entity index.
pub(crate) fn repair(mut content: Vec<u8>) -> (Vec<u8>, EntityIndex) {
    let mut index = Arc::new(build_entity_index(&content));
    type Pass = fn(&[u8], &Arc<EntityIndex>) -> Option<Vec<u8>>;
    let passes: [Pass; 2] = [|c, i| null_dangling_refs(c, i), bsplines_to_polycurves];
    for pass in passes {
        if let Some(repaired) = pass(&content, &index) {
            content = repaired;
            index = Arc::new(build_entity_index(&content));
        }
    }
    (content, Arc::unwrap_or_clone(index))
}

/// Replace references to entities the file never defines with `$` (unset).
///
/// ifc-lite fails an element whose placement chain reaches a missing entity.
/// The West Riverside Hospital MEP exports point the site placement's axis at
/// an undefined `#19`, and since every element is placed relative to the site,
/// none of them rendered. The attributes affected so far are optional, so
/// unset is what a tolerant reader makes of them. Returns `None` when nothing
/// dangles, which is the usual case and costs one pass over the bytes.
fn null_dangling_refs(content: &[u8], index: &EntityIndex) -> Option<Vec<u8>> {
    // References only occur in the DATA section, which starts at the first entity.
    let start = index.values().map(|&(s, _)| s).min()?;
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0; // bytes of `content` already moved to `out`
    let mut in_string = false;
    let mut i = start;
    while i < content.len() {
        let b = content[i];
        if b == b'\'' {
            // `''` inside a string is an escaped quote: toggling twice keeps us inside.
            in_string = !in_string;
        } else if b == b'#' && !in_string {
            let digits = content[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let end = i + 1 + digits;
            // `#12=` defines an entity; anything else is a reference.
            if digits > 0 && content.get(end) != Some(&b'=') {
                let id = std::str::from_utf8(&content[i + 1..end])
                    .ok()
                    .and_then(|d| d.parse::<u32>().ok());
                if id.is_some_and(|id| !index.contains_key(&id)) {
                    let buf = out.get_or_insert_with(|| Vec::with_capacity(content.len()));
                    buf.extend_from_slice(&content[copied..i]);
                    buf.push(b'$');
                    copied = end;
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    let mut buf = out?;
    buf.extend_from_slice(&content[copied..]);
    Some(buf)
}

/// IFC class name of the entity whose line starts at `start` (`#12=IFCWALL(...`).
fn entity_type(content: &[u8], start: usize) -> &[u8] {
    let line = &content[start..];
    let eq = line.iter().position(|&b| b == b'=').map_or(0, |i| i + 1);
    let line = &line[eq..];
    let skip = line.iter().take_while(|b| b.is_ascii_whitespace()).count();
    let len = line[skip..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    &line[skip..skip + len]
}

/// Replace B-spline curves, which ifc-lite cannot mesh, with
/// `IfcIndexedPolyCurve`s sampled to the default curve tolerance.
///
/// Each `IfcBSplineCurveWithKnots` / `IfcRationalBSplineCurveWithKnots` line
/// becomes a polycurve over its whole domain, keeping its id (an unclamped one
/// sampled like `decompile` does). A trimmed curve over a B-spline is
/// rewritten too, to the polycurve of its trimmed range: its parameter trims
/// are B-spline parameters, which mean something else on a polycurve, so a
/// B-spline is kept when one of its trimmed curves cannot be read or it is
/// unclamped. Cartesian trims take the whole range. The new point lists are
/// appended before the closing `ENDSEC;`. Returns `None` when there is nothing
/// to rewrite (usually after one pass over the entity index) or no `ENDSEC;`.
fn bsplines_to_polycurves(content: &[u8], index: &Arc<EntityIndex>) -> Option<Vec<u8>> {
    // Also matches IFCRATIONALBSPLINECURVEWITHKNOTS.
    if !content.windows(21).any(|w| w == b"BSPLINECURVEWITHKNOTS") {
        return None;
    }
    let (mut splines, mut trims, mut project) = (Vec::new(), Vec::new(), None);
    for (&id, &(start, _)) in index.iter() {
        match entity_type(content, start) {
            b"IFCBSPLINECURVEWITHKNOTS" | b"IFCRATIONALBSPLINECURVEWITHKNOTS" => splines.push(id),
            b"IFCTRIMMEDCURVE" => trims.push(id),
            b"IFCPROJECT" => project = project.or(Some(id)),
            _ => {}
        }
    }
    if splines.is_empty() {
        return None;
    }
    // Index order is hash order; sorted ids keep the output reproducible.
    splines.sort_unstable();
    trims.sort_unstable();
    let mut decoder = EntityDecoder::with_arc_index(content, index.clone());
    let tol = default_curve_tolerance() / length_unit_scale(project, &mut decoder);

    let mut curves: HashMap<u32, (Nurbs<[f64; 3]>, usize, bool)> = HashMap::new();
    for &id in &splines {
        if let Some(c) = decode_bspline(id, &mut decoder) {
            curves.insert(id, c);
        }
    }
    // A B-spline is rewritten only together with every trimmed curve over it:
    // a trim left as it is would read B-spline parameters on a polycurve.
    let mut trimmed = Vec::new();
    let mut keep = std::collections::HashSet::new();
    for &id in &trims {
        let Some(basis) = trim_basis(content, index[&id]).filter(|b| curves.contains_key(b)) else {
            continue;
        };
        match decoder.decode_by_id(id) {
            Ok(t) if curves[&basis].2 => trimmed.push((id, basis, t)),
            _ => {
                keep.insert(basis);
            }
        }
    }
    curves.retain(|id, _| !keep.contains(id));
    if curves.is_empty() {
        return None;
    }
    // (entity id, sampled points, dimension)
    let mut polycurves: Vec<(u32, Vec<[f64; 3]>, usize)> = Vec::new();
    for id in &splines {
        if let Some((n, dim, clamped)) = curves.get(id) {
            let points = if *clamped {
                sample(n, n.domain(), true, tol)
            } else {
                n.samples()
            };
            polycurves.push((*id, points, *dim));
        }
    }
    for (id, basis, t) in trimmed {
        let Some((n, dim, _)) = curves.get(&basis) else {
            continue;
        };
        // The curve between the two trims, reversed unless SenseAgreement is
        // set, as `decompile` reads it.
        let forward = t.get(3).and_then(|a| a.as_enum()) != Some("F");
        let range = match (trim_param(t.get(1)), trim_param(t.get(2))) {
            (Some(u0), Some(u1)) => (u0.min(u1), u0.max(u1)),
            _ => n.domain(),
        };
        polycurves.push((id, sample(n, range, forward, tol), *dim));
    }

    let first_id = index.keys().max().copied().unwrap_or(0) + 1;
    let mut lines: Vec<(usize, usize, String)> = Vec::new();
    let mut appended = String::new();
    for (next_id, (id, points, dim)) in (first_id..).zip(polycurves) {
        let coords: Vec<String> = points
            .iter()
            .map(|p| {
                let c: Vec<String> = p[..dim].iter().map(|v| step_real(*v)).collect();
                format!("({})", c.join(","))
            })
            .collect();
        appended.push_str(&format!(
            "#{next_id}=IFCCARTESIANPOINTLIST{dim}D(({}));\n",
            coords.join(",")
        ));
        let (start, end) = index[&id];
        lines.push((
            start,
            end,
            format!("#{id}=IFCINDEXEDPOLYCURVE(#{next_id},$,.F.);"),
        ));
    }
    lines.sort_unstable_by_key(|l| l.0);

    let data_end = last_endsec(content)?;
    let mut out = Vec::with_capacity(content.len() + appended.len());
    let mut copied = 0;
    for (start, end, line) in lines {
        out.extend_from_slice(&content[copied..start]);
        out.extend_from_slice(line.as_bytes());
        copied = end;
    }
    out.extend_from_slice(&content[copied..data_end]);
    out.extend_from_slice(appended.as_bytes());
    out.extend_from_slice(&content[data_end..]);
    Some(out)
}

/// The B-spline `id`, the dimension of its control points (2D points padded
/// with 0) and whether its knots are clamped. `None` for malformed curves,
/// which stay as they are.
fn decode_bspline(id: u32, decoder: &mut EntityDecoder) -> Option<(Nurbs<[f64; 3]>, usize, bool)> {
    // IfcBSplineCurve(Degree, ControlPointsList, CurveForm, ClosedCurve,
    // SelfIntersect), WithKnots(KnotMultiplicities, Knots, KnotSpec),
    // Rational(WeightsData).
    let e = decoder.decode_by_id(id).ok()?;
    let degree = e.get(0)?.as_int()? as usize;
    let mut dim = 2;
    let mut points = Vec::new();
    for r in e.get(1)?.as_list()? {
        let p = decoder.resolve_ref(r).ok()??;
        let c: Vec<f64> = p
            .get(0)?
            .as_list()?
            .iter()
            .filter_map(|v| v.as_float())
            .collect();
        dim = c.len().clamp(2, 3);
        points.push([*c.first()?, *c.get(1)?, c.get(2).copied().unwrap_or(0.0)]);
    }
    let mults = e.get(5)?.as_list()?;
    let values = e.get(6)?.as_list()?;
    let (values, mults): (Vec<f64>, Vec<usize>) = mults
        .iter()
        .zip(values)
        .map(|(m, k)| Some((k.as_float()?, m.as_int()? as usize)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .unzip();
    let knots = expand_knots(&values, &mults);
    let weights = match e.get(8).and_then(|a| a.as_list()) {
        Some(w) => Some(
            w.iter()
                .map(|v| v.as_float())
                .collect::<Option<Vec<f64>>>()?,
        ),
        None => None,
    };
    match Nurbs::new(degree, points.clone(), weights.clone(), Some(knots.clone())) {
        Ok(n) => Some((n, dim, true)),
        // `Nurbs::new` checked the weights and knot count before clamping.
        Err(CurveError::UnclampedKnots) => {
            let weights = weights.unwrap_or_else(|| vec![1.0; points.len()]);
            let n = Nurbs {
                degree,
                points,
                weights,
                knots,
            };
            Some((n, dim, false))
        }
        Err(_) => None,
    }
}

/// Basis curve id of the `IfcTrimmedCurve` line at `start..end`, read from the
/// text so that a line the decoder rejects still names its basis.
fn trim_basis(content: &[u8], (start, end): (usize, usize)) -> Option<u32> {
    let line = &content[start..end];
    let open = line.iter().position(|&b| b == b'(')?;
    let rest = &line[open + 1..];
    let rest = &rest[rest.iter().take_while(|b| b.is_ascii_whitespace()).count()..];
    let digits = rest.strip_prefix(b"#")?;
    let len = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    std::str::from_utf8(&digits[..len]).ok()?.parse().ok()
}

/// Offset of the last `ENDSEC;` (whitespace allowed before the `;`), which
/// closes the DATA section.
fn last_endsec(content: &[u8]) -> Option<usize> {
    let mut end = content.len();
    while let Some(at) = content[..end].windows(6).rposition(|w| w == b"ENDSEC") {
        let rest = &content[at + 6..];
        let skip = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
        if rest.get(skip) == Some(&b';') {
            return Some(at);
        }
        end = at;
    }
    None
}

/// The `IfcParameterValue` of a trim select list, if it has one.
fn trim_param(attr: Option<&ifc_lite_core::AttributeValue>) -> Option<f64> {
    attr?.as_list()?.iter().find_map(|v| match v.as_list() {
        Some([name, value]) if name.as_string() == Some("IFCPARAMETERVALUE") => value.as_float(),
        _ => None,
    })
}

/// Points of `n` over the parameter `range` to chord error `tol`, reversed
/// unless `forward`.
fn sample(n: &Nurbs<[f64; 3]>, range: (f64, f64), forward: bool, tol: f64) -> Vec<[f64; 3]> {
    let curve = Curve {
        segments: vec![Segment::Nurbs(n.trim(range.0, range.1))],
        closed: false,
    };
    let mut points = curve.tessellate(tol);
    if !forward {
        points.reverse();
    }
    points
}

/// A STEP REAL, which always has a decimal point.
fn step_real(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { s + "." }
}

/// Colour of an element with neither a surface style nor a palette look.
const DEFAULT_COLOR: [f32; 4] = [0.72, 0.72, 0.74, 1.0];

/// BML element type of an IFC product class.
fn element_type(t: &str) -> Option<ElementType> {
    match base_type(t) {
        "IFCSTAIRFLIGHT" => Some(ElementType::Stair),
        "IFCFURNITURE" => Some(ElementType::Furniture),
        t => ELEMENT_REGISTRY
            .iter()
            .find(|d| d.ifc_entity.eq_ignore_ascii_case(t))
            .map(|d| d.element_type),
    }
}

/// Look of an element without a surface style, chosen as the BML preview of the
/// decompiled model does: the palette entry of its first material (the one
/// `decompile` keeps), else the default of its element type.
fn fallback_look(type_name: &str, material_names: Option<&str>) -> Option<SceneMaterial> {
    material_names
        .and_then(|names| names.split(NAME_SEP).next())
        .and_then(appearance::guess)
        .or_else(|| element_type(type_name).and_then(appearance::for_type))
}

/// Weld vertices on a 0.1 mm grid (after scaling to metres), remapping indices.
///
/// `origin` is the mesh's local frame origin: ifc-lite may store positions
/// relative to it to keep building-scale coordinates f32-precise, so a vertex's
/// world position is `origin + position`.
pub(crate) fn weld(
    positions: &[f32],
    indices: &[u32],
    scale: f32,
    origin: [f64; 3],
) -> (Vec<[f32; 3]>, Vec<u32>) {
    use std::collections::HashMap;
    const Q: f32 = 10_000.0; // 0.1 mm
    let o = [origin[0] as f32, origin[1] as f32, origin[2] as f32];
    let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut out_pos: Vec<[f32; 3]> = Vec::new();
    let mut remap = vec![0u32; positions.len() / 3];
    for i in 0..positions.len() / 3 {
        let p = [
            (positions[3 * i] + o[0]) * scale,
            (positions[3 * i + 1] + o[1]) * scale,
            (positions[3 * i + 2] + o[2]) * scale,
        ];
        let key = (
            (p[0] * Q).round() as i64,
            (p[1] * Q).round() as i64,
            (p[2] * Q).round() as i64,
        );
        let idx = *map.entry(key).or_insert_with(|| {
            out_pos.push([
                (p[0] * Q).round() / Q,
                (p[1] * Q).round() / Q,
                (p[2] * Q).round() / Q,
            ]);
            (out_pos.len() - 1) as u32
        });
        remap[i] = idx;
    }
    let out_idx = indices.iter().map(|&i| remap[i as usize]).collect();
    (out_pos, out_idx)
}

/// One renderable piece of an element: the material it resolved to, plus its
/// welded positions and triangle indices.
type Part = (Option<u32>, Vec<[f32; 3]>, Vec<u32>);

/// Append a welded sub-mesh to the part list, merging into the existing entry
/// when another sub-mesh already resolved to the same material.
fn merge_part(
    parts: &mut Vec<Part>,
    material: Option<u32>,
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
) {
    if indices.is_empty() {
        return;
    }
    match parts.iter_mut().find(|(m, _, _)| *m == material) {
        Some((_, p, i)) => {
            let offset = p.len() as u32;
            p.extend_from_slice(&positions);
            i.extend(indices.iter().map(|&x| x + offset));
        }
        None => parts.push((material, positions, indices)),
    }
}

/// Flatten an element's parts into a single position/index pair, for the
/// footprint and storey-assignment maths that treat the element as a whole.
fn combine_parts(parts: &[Part]) -> (Vec<[f32; 3]>, Vec<u32>) {
    if parts.len() == 1 {
        return (parts[0].1.clone(), parts[0].2.clone());
    }
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (_, p, i) in parts {
        let offset = positions.len() as u32;
        positions.extend_from_slice(p);
        indices.extend(i.iter().map(|&x| x + offset));
    }
    (positions, indices)
}

/// Area-weighted smooth vertex normals from welded positions + indices.
fn smooth_normals(pos: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; pos.len()];
    for tri in indices.as_chunks::<3>().0 {
        let (a, b, c) = (
            pos[tri[0] as usize],
            pos[tri[1] as usize],
            pos[tri[2] as usize],
        );
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let fn_ = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &vi in tri {
            let m = &mut n[vi as usize];
            m[0] += fn_[0];
            m[1] += fn_[1];
            m[2] += fn_[2];
        }
    }
    for m in n.iter_mut() {
        let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
        if len > 0.0 {
            m[0] /= len;
            m[1] /= len;
            m[2] /= len;
        } else {
            *m = [0.0, 0.0, 1.0];
        }
    }
    n
}

fn span() -> serde_json::Value {
    serde_json::json!({
        "start": {"line": 0, "column": 0, "byte_offset": 0},
        "end": {"line": 0, "column": 0, "byte_offset": 0}
    })
}

/// Index of the storey whose elevation is the highest at or below `z`.
fn storey_for_z(storeys: &[(u32, String, f64)], z: f32) -> usize {
    let mut best = 0;
    for (i, (_, _, elev)) in storeys.iter().enumerate() {
        if (*elev as f32) <= z + 0.5 {
            best = i;
        }
    }
    best
}

pub fn run(input: PathBuf) -> miette::Result<()> {
    let content = std::fs::read(&input)
        .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;
    let (content, index) = repair(content);

    // One cheap scan: products to mesh, plus storeys + containment for the 2D model.
    let mut ids: Vec<(u32, String)> = Vec::new();
    let mut storey_ids: Vec<u32> = Vec::new();
    let mut rel_ids: Vec<u32> = Vec::new();
    let mut voids_ids: Vec<u32> = Vec::new();
    let mut fills_ids: Vec<u32> = Vec::new();
    let mut project = None;
    let mut scanner = EntityScanner::new(&content);
    while let Some((id, type_name, _start, _end)) = scanner.next_entity() {
        if is_rendered(type_name) {
            ids.push((id, type_name.to_string()));
        } else if type_name == "IFCBUILDINGSTOREY" {
            storey_ids.push(id);
        } else if type_name == "IFCRELCONTAINEDINSPATIALSTRUCTURE" {
            rel_ids.push(id);
        } else if type_name == "IFCRELVOIDSELEMENT" {
            voids_ids.push(id);
        } else if type_name == "IFCRELFILLSELEMENT" {
            fills_ids.push(id);
        } else if type_name == "IFCPROJECT" {
            project.get_or_insert(id);
        }
    }

    let mut decoder = EntityDecoder::with_index(&content, index);
    let scale = length_unit_scale(project, &mut decoder) as f32; // file length unit -> metres
    let router = GeometryRouter::new();
    let mut styles = StyleIndex::build(&content, &mut decoder);
    let rendered: std::collections::HashSet<u32> = ids.iter().map(|(id, _)| *id).collect();
    let mut props = PropertyIndex::build(&content, &mut decoder, &rendered);

    // Storeys (name + elevation), sorted bottom-up, and element -> storey index.
    let mut storeys: Vec<(u32, String, f64)> = Vec::new();
    for sid in &storey_ids {
        if let Ok(e) = decoder.decode_by_id(*sid) {
            let name = e
                .get(2)
                .and_then(|a| a.as_string())
                .unwrap_or("Storey")
                .to_string();
            let elev = e.get(9).and_then(|a| a.as_float()).unwrap_or(0.0) * scale as f64;
            storeys.push((*sid, name, elev));
        }
    }
    storeys.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    let storey_idx: HashMap<u32, usize> = storeys
        .iter()
        .enumerate()
        .map(|(i, (id, _, _))| (*id, i))
        .collect();

    let mut elem_storey: HashMap<u32, usize> = HashMap::new();
    for rid in &rel_ids {
        if let Ok(r) = decoder.decode_by_id(*rid) {
            let structure = r.get(5).and_then(|a| a.as_entity_ref());
            if let Some(si) = structure.and_then(|s| storey_idx.get(&s)).copied()
                && let Some(list) = r.get(4).and_then(|a| a.as_list())
            {
                for it in list {
                    if let Some(eid) = it.as_entity_ref() {
                        elem_storey.insert(eid, si);
                    }
                }
            }
        }
    }

    let nstoreys = storeys.len().max(1);
    let mut storey_elems: Vec<Vec<serde_json::Value>> = vec![Vec::new(); nstoreys];

    // Walls are collected separately so doors/windows can nest under them.
    struct WallEntry {
        id: u32,
        storey: usize,
        from: [f32; 2],
        to: [f32; 2],
        thickness: f32,
        height: f32,
        via: Option<[f32; 2]>,
        children: Vec<serde_json::Value>,
    }
    struct Filler {
        id: u32,
        etype: &'static str,
        xy: Vec<[f32; 2]>,
        zmin: f32,
        zmax: f32,
    }
    let mut walls: Vec<WallEntry> = Vec::new();
    let mut wall_by_id: HashMap<u32, usize> = HashMap::new();
    let mut fillers: Vec<Filler> = Vec::new();

    let mut nodes = Vec::new();
    for (id, type_name) in &ids {
        let element = match decoder.decode_by_id(*id) {
            Ok(e) => e,
            Err(_) => continue,
        };
        // ifc-lite emits near-unwelded geometry; weld by quantised position
        // (0.1 mm grid) and recompute smooth normals so the JSON stays small.
        //
        // Per-item sub-meshes keep separately styled parts (a window's frame and
        // its glazing) apart so each can take its own material. Elements the
        // sub-mesh channel cannot handle fall back to the combined mesh.
        let mut parts: Vec<Part> = Vec::new();
        match router.process_element_with_submeshes(&element, &mut decoder) {
            Ok(subs) if !subs.is_empty() => {
                for sub in subs.iter() {
                    let (p, i) = weld(
                        &sub.mesh.positions,
                        &sub.mesh.indices,
                        scale,
                        sub.mesh.origin,
                    );
                    let material = styles
                        .for_item(sub.geometry_id)
                        .or_else(|| styles.for_product(*id));
                    merge_part(&mut parts, material, p, i);
                }
            }
            _ => match router.process_element(&element, &mut decoder) {
                Ok(m) if !m.indices.is_empty() => {
                    let (p, i) = weld(&m.positions, &m.indices, scale, m.origin);
                    merge_part(&mut parts, styles.for_product(*id), p, i);
                }
                _ => continue,
            },
        }
        if parts.is_empty() {
            continue;
        }
        // Only wholly unstyled elements take the palette: an unstyled frame next
        // to styled glazing must not turn into glass. The palette look is no
        // surface style of the file, so it stays out of the inspector row.
        let mut fallback = None;
        if parts.iter().all(|p| p.0.is_none())
            && let Some(look) = fallback_look(type_name, styles.material_names(*id))
        {
            let m = appearance::intern(&mut styles.materials, look);
            parts.iter_mut().for_each(|p| p.0 = Some(m));
            fallback = Some(m);
        }

        // The floorplan and storey assignment work on the element as a whole.
        let (positions, indices) = combine_parts(&parts);

        // 2D footprint for the floorplan, assigned to its storey.
        let zmin = positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let zmax = positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        let zc = (zmin + zmax) * 0.5;
        let si = elem_storey
            .get(id)
            .copied()
            .unwrap_or_else(|| storey_for_z(&storeys, zc))
            .min(nstoreys - 1);
        match base_type(type_name) {
            "IFCWALL" => {
                let xy: Vec<[f32; 2]> = positions.iter().map(|p| [p[0], p[1]]).collect();
                if let Some((from, to, thickness, via)) = wall_axis(&xy) {
                    wall_by_id.insert(*id, walls.len());
                    walls.push(WallEntry {
                        id: *id,
                        storey: si,
                        from,
                        to,
                        thickness,
                        height: zmax - zmin,
                        via,
                        children: Vec::new(),
                    });
                }
            }
            "IFCDOOR" | "IFCWINDOW" => {
                let xy = positions.iter().map(|p| [p[0], p[1]]).collect();
                fillers.push(Filler {
                    id: *id,
                    etype: if base_type(type_name) == "IFCDOOR" {
                        "Door"
                    } else {
                        "Window"
                    },
                    xy,
                    zmin,
                    zmax,
                });
            }
            _ => {
                let boundary = outline(&positions, &indices);
                if boundary.len() >= 3 {
                    storey_elems[si].push(serde_json::json!({
                        "element_type": "Slab", "id": id.to_string(),
                        "properties": [
                            serde_json::json!({"key":"boundary","value":{"Polygon": boundary.iter().map(|p| serde_json::json!([p[0],p[1]])).collect::<Vec<_>>()},"span":span()}),
                            serde_json::json!({"key":"thickness","value":{"Number":(zmax - zmin).max(0.0)},"span":span()}),
                        ],
                        "children": [], "span": span(),
                    }));
                }
            }
        }

        // IfcRoot(GlobalId, OwnerHistory, Name, Description); elements add
        // ObjectType at 4.
        let short = type_name.strip_prefix("IFC").unwrap_or(type_name);
        let name = element.get(2).and_then(|a| a.as_string()).unwrap_or("");
        let label = if name.is_empty() {
            format!("{} #{}", short, id)
        } else {
            format!("{} - {}", short, name)
        };
        let mut properties: Vec<(String, String)> = vec![("IFC Type".into(), type_name.clone())];
        if !name.is_empty() {
            properties.push(("Name".into(), name.to_string()));
        }
        if let Some(guid) = element.get(0).and_then(|a| a.as_string()) {
            properties.push(("GlobalId".into(), guid.to_string()));
        }
        let object_type = element.get(4).and_then(|a| a.as_string()).unwrap_or("");
        if !object_type.is_empty() {
            properties.push(("Object Type".into(), object_type.to_string()));
        }
        if let Some(mats) = styles.material_names(*id) {
            properties.push(("Material".into(), mats.to_string()));
        }
        // Largest part first, so the element's own mesh is the dominant one and
        // the rest become children sharing its id — which keeps picking and
        // highlighting whole-element.
        parts.sort_by_key(|p| std::cmp::Reverse(p.2.len()));
        let mut surface_names: Vec<&str> = Vec::new();
        for (m, _, _) in parts.iter().filter(|p| p.0 != fallback) {
            if let Some(n) = m.and_then(|m| styles.get(m)).map(|m| m.name.as_str())
                && !n.is_empty()
                && !surface_names.contains(&n)
            {
                surface_names.push(n);
            }
        }
        if !surface_names.is_empty() {
            properties.push((
                "Surface Style".into(),
                surface_names.join(super::ifc_styles::NAME_SEP),
            ));
        }
        // Storey drives the viewer's per-storey visibility filter as well as
        // being useful on its own.
        if let Some((_, storey_name, _)) = storeys.get(si) {
            properties.push(("Storey".into(), storey_name.clone()));
        }
        if let Some(rows) = props.take(*id) {
            properties.extend(rows);
        }

        let mut part_nodes: Vec<SceneNode> = parts
            .into_iter()
            .enumerate()
            .map(|(part_index, (material, positions, indices))| {
                let normals = smooth_normals(&positions, &indices);
                let vertices: Vec<Vertex> = positions
                    .iter()
                    .zip(normals.iter())
                    .map(|(&position, &normal)| Vertex { position, normal })
                    .collect();
                let mesh_color = material
                    .and_then(|m| styles.get(m))
                    .map(|m| m.color)
                    .unwrap_or(DEFAULT_COLOR);
                SceneNode {
                    id: id.to_string(),
                    element_type: short.to_string(),
                    label: label.clone(),
                    mesh: SceneMesh {
                        vertices,
                        indices,
                        color: mesh_color,
                    },
                    children: Vec::new(),
                    // Property sets run to dozens of rows per element; carrying
                    // them once and letting the loader inherit them keeps the
                    // extra parts from multiplying the payload.
                    properties: if part_index == 0 {
                        std::mem::take(&mut properties)
                    } else {
                        Vec::new()
                    },
                    material,
                }
            })
            .collect();
        let mut node = part_nodes.remove(0);
        node.children = part_nodes;
        nodes.push(node);
    }

    use serde_json::json;

    // Nest doors/windows under their host wall: filler -> opening (fills) ->
    // wall (voids). Offset/width come from projecting the footprint onto the axis.
    let mut opening_wall: HashMap<u32, u32> = HashMap::new();
    for vid in &voids_ids {
        if let Ok(r) = decoder.decode_by_id(*vid)
            && let (Some(w), Some(o)) = (
                r.get(4).and_then(|a| a.as_entity_ref()),
                r.get(5).and_then(|a| a.as_entity_ref()),
            )
        {
            opening_wall.insert(o, w);
        }
    }
    let mut filler_wall: HashMap<u32, u32> = HashMap::new();
    for fid in &fills_ids {
        if let Ok(r) = decoder.decode_by_id(*fid)
            && let (Some(o), Some(f)) = (
                r.get(4).and_then(|a| a.as_entity_ref()),
                r.get(5).and_then(|a| a.as_entity_ref()),
            )
            && let Some(&w) = opening_wall.get(&o)
        {
            filler_wall.insert(f, w);
        }
    }
    for f in &fillers {
        let widx = match filler_wall.get(&f.id).and_then(|w| wall_by_id.get(w)) {
            Some(&i) => i,
            None => continue,
        };
        let (wf, wt) = (walls[widx].from, walls[widx].to);
        let dir = [wt[0] - wf[0], wt[1] - wf[1]];
        let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt().max(1e-6);
        let d = [dir[0] / len, dir[1] / len];
        let (mut tmin, mut tmax) = (f32::MAX, f32::MIN);
        for p in &f.xy {
            let t = (p[0] - wf[0]) * d[0] + (p[1] - wf[1]) * d[1];
            tmin = tmin.min(t);
            tmax = tmax.max(t);
        }
        let offset = tmin.max(0.0);
        let width = (tmax - tmin).max(0.05);
        let height = (f.zmax - f.zmin).max(0.05);
        let mut props = vec![
            json!({"key":"offset","value":{"Number":offset},"span":span()}),
            json!({"key":"width","value":{"Number":width},"span":span()}),
            json!({"key":"height","value":{"Number":height},"span":span()}),
        ];
        if f.etype == "Window" {
            // Mesh z is absolute; BML sill height is relative to the storey.
            let elev = storeys.get(walls[widx].storey).map_or(0.0, |s| s.2 as f32);
            let sill = f.zmin - elev;
            props.push(json!({"key":"sill_height","value":{"Number":sill},"span":span()}));
        }
        walls[widx].children.push(json!({
            "element_type": f.etype, "id": f.id.to_string(),
            "properties": props, "children": [], "span": span(),
        }));
    }

    // Emit walls (with nested fillers) into their storeys.
    for w in walls {
        let mut axis = json!({"from":[w.from[0],w.from[1]],"to":[w.to[0],w.to[1]],"via":null});
        if let Some(v) = w.via {
            axis["via"] = json!([v[0], v[1]]);
        }
        storey_elems[w.storey].push(json!({
            "element_type": "Wall", "id": w.id.to_string(),
            "properties": [
                json!({"key":"from","value":{"Axis":axis},"span":span()}),
                json!({"key":"thickness","value":{"Number":w.thickness},"span":span()}),
                json!({"key":"height","value":{"Number":w.height},"span":span()}),
            ],
            "children": w.children, "span": span(),
        }));
    }

    // Build the SemanticModel JSON the floorplan importer consumes.
    let mut storeys_json: Vec<serde_json::Value> = Vec::new();
    if storeys.is_empty() {
        storeys_json.push(json!({
            "name": "Storey",
            "properties": [
                json!({"key":"elevation","value":{"Number":0.0},"span":span()}),
                json!({"key":"height","value":{"Number":3.0},"span":span()}),
            ],
            "spaces": [], "elements": std::mem::take(&mut storey_elems[0]), "span": span(),
        }));
    } else {
        for (i, (_, name, elev)) in storeys.iter().enumerate() {
            storeys_json.push(json!({
                "name": name,
                "properties": [
                    json!({"key":"elevation","value":{"Number":elev},"span":span()}),
                    json!({"key":"height","value":{"Number":3.0},"span":span()}),
                ],
                "spaces": [], "elements": std::mem::take(&mut storey_elems[i]), "span": span(),
            }));
        }
    }
    let model = json!({
        "sites": [{
            "name": "Site", "properties": [],
            "buildings": [{
                "name": "Building", "properties": [],
                "storeys": storeys_json, "span": span(),
            }],
            "span": span(),
        }],
        "materials": [], "asset_libraries": [],
    });

    let scene = SceneGraph {
        nodes,
        materials: styles.materials,
    };
    // Stream straight to stdout: a `Value` tree of a large scene costs
    // several times the size of the JSON it prints.
    #[derive(serde::Serialize)]
    struct Out<'a> {
        scene: &'a SceneGraph,
        model: serde_json::Value,
    }
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    serde_json::to_writer(
        &mut stdout,
        &Out {
            scene: &scene,
            model,
        },
    )
    .map_err(|e| miette::miette!("serialization error: {}", e))?;
    writeln!(stdout)
        .and_then(|_| stdout.flush())
        .map_err(|e| miette::miette!("write error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangling_references_become_unset() {
        let ifc = b"DATA;\n#1= IFCDIRECTION((0.,0.,1.));\n#2= IFCAXIS2PLACEMENT3D(#1,#19,'#19 ''x');\nENDSEC;";
        let index = build_entity_index(&ifc[..]);
        let repaired = null_dangling_refs(ifc, &index).expect("#19 dangles");
        assert_eq!(
            std::str::from_utf8(&repaired).unwrap(),
            "DATA;\n#1= IFCDIRECTION((0.,0.,1.));\n#2= IFCAXIS2PLACEMENT3D(#1,$,'#19 ''x');\nENDSEC;"
        );
        let clean =
            b"DATA;\n#1= IFCDIRECTION((0.,0.,1.));\n#2= IFCAXIS2PLACEMENT3D(#1,$,$);\nENDSEC;";
        assert!(null_dangling_refs(clean, &build_entity_index(&clean[..])).is_none());
    }

    #[test]
    fn product_classes_are_rendered_except_containers_and_voids() {
        assert_eq!(base_type("IFCSLABSTANDARDCASE"), "IFCSLAB");
        assert_eq!(base_type("IFCWALLELEMENTEDCASE"), "IFCWALL");
        assert_eq!(base_type("IFCWINDOW"), "IFCWINDOW");
        for t in [
            "IFCWALLSTANDARDCASE",
            "IFCDOORSTANDARDCASE",
            "IFCPLATESTANDARDCASE",
        ] {
            assert!(is_rendered(t), "{t}");
        }
        for t in [
            "IFCPIPESEGMENT",
            "IFCDUCTFITTING",
            "IFCFLOWSEGMENT",
            "IFCSANITARYTERMINAL",
        ] {
            assert!(is_rendered(t), "{t}");
        }
        for t in [
            "IFCSPACE",
            "IFCOPENINGSTANDARDCASE",
            "IFCBUILDINGSTOREY",
            "IFCDISTRIBUTIONPORT",
        ] {
            assert!(!is_rendered(t), "{t}");
        }
        assert_eq!(element_type("IFCDOORSTANDARDCASE"), Some(ElementType::Door));
        assert_eq!(
            element_type("IFCFURNISHINGELEMENT"),
            Some(ElementType::Furniture)
        );
    }

    #[test]
    fn unstyled_elements_take_the_bml_palette() {
        let name = |t, m| fallback_look(t, m).map(|l| l.name);
        // First material (the one decompile keeps) first, element type second.
        assert_eq!(
            name("IFCWALL", Some("Brick, Common · Air")).as_deref(),
            Some("Brick")
        );
        assert_eq!(
            name("IFCWALL", Some("Air · Brick, Common")).as_deref(),
            Some("Plaster")
        );
        assert_eq!(
            name("IFCWALL", Some("Clad - White")).as_deref(),
            Some("Plaster")
        );
        assert_eq!(
            name("IFCSLABSTANDARDCASE", None).as_deref(),
            Some("Concrete")
        );
        assert_eq!(name("IFCFLOWTERMINAL", None), None);
    }

    /// Points of the `IfcIndexedPolyCurve` `id` in `ifc`, padded to 3D.
    fn polycurve_points(ifc: &[u8], id: u32) -> Vec<[f64; 3]> {
        let mut decoder = EntityDecoder::new(ifc);
        let curve = decoder.decode_by_id(id).unwrap();
        assert_eq!(curve.ifc_type, ifc_lite_core::IfcType::IfcIndexedPolyCurve);
        let list = decoder.resolve_ref(curve.get(0).unwrap()).unwrap().unwrap();
        list.get(0)
            .and_then(|a| a.as_list())
            .unwrap()
            .iter()
            .map(|p| {
                let c: Vec<f64> = p
                    .as_list()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_float())
                    .collect();
                [c[0], c[1], c.get(2).copied().unwrap_or(0.0)]
            })
            .collect()
    }

    #[test]
    fn bsplines_become_indexed_polycurves() {
        // Rational quarter circle of radius 1, bare and trimmed: whole, and its
        // first half run backwards.
        let ifc = b"DATA;\n#1=IFCCARTESIANPOINT((1.,0.));\n#2=IFCCARTESIANPOINT((1.,1.));\n#3=IFCCARTESIANPOINT((0.,1.));\n\
#4=IFCRATIONALBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.,(1.,0.7071067811865476,1.));\n\
#5=IFCTRIMMEDCURVE(#4,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(1.)),.T.,.PARAMETER.);\n\
#6=IFCTRIMMEDCURVE(#4,(IFCPARAMETERVALUE(0.5)),(IFCPARAMETERVALUE(0.)),.F.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;\n";
        let out = bsplines_to_polycurves(ifc, &Arc::new(build_entity_index(&ifc[..])))
            .expect("rewritten");
        let text = std::str::from_utf8(&out).unwrap();
        assert!(
            !text.contains("IFCBSPLINE") && !text.contains("IFCRATIONALBSPLINE"),
            "{text}"
        );
        assert!(!text.contains("IFCTRIMMEDCURVE"), "{text}");
        assert!(text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"), "{text}");
        let on_circle = |pts: &[[f64; 3]]| {
            assert!(pts.len() > 3);
            assert!(max_sagitta(pts, 1.0) <= default_curve_tolerance());
        };
        for id in [4, 5] {
            let pts = polycurve_points(&out, id);
            on_circle(&pts);
            assert!((pts[0][0] - 1.0).abs() < 1e-9 && pts[0][1].abs() < 1e-9);
            let last = pts.last().unwrap();
            assert!(last[0].abs() < 1e-9 && (last[1] - 1.0).abs() < 1e-9);
        }
        let half = polycurve_points(&out, 6);
        on_circle(&half);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (half[0][0] - h).abs() < 1e-9 && (half[0][1] - h).abs() < 1e-9,
            "{half:?}"
        );
        let last = half.last().unwrap();
        assert!((last[0] - 1.0).abs() < 1e-9 && last[1].abs() < 1e-9);

        let clean = b"DATA;\n#1=IFCCARTESIANPOINT((1.,0.));\n#2=IFCPOLYLINE((#1,#1));\nENDSEC;\n";
        assert!(bsplines_to_polycurves(clean, &Arc::new(build_entity_index(&clean[..]))).is_none());
    }

    #[test]
    fn unclamped_bsplines_and_reversed_trims() {
        // An unclamped spline is sampled; a .T. trim from 0.5 to 0 runs from 0 to 0.5.
        let ifc = b"DATA;\n#1=IFCCARTESIANPOINT((1.,0.));\n#2=IFCCARTESIANPOINT((1.,1.));\n#3=IFCCARTESIANPOINT((0.,1.));\n\
#4=IFCRATIONALBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.,(1.,0.7071067811865476,1.));\n\
#5=IFCTRIMMEDCURVE(#4,(IFCPARAMETERVALUE(0.5)),(IFCPARAMETERVALUE(0.)),.T.,.PARAMETER.);\n\
#6=IFCCARTESIANPOINT((2.,0.));\n\
#7=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3,#6),.UNSPECIFIED.,.F.,.F.,(1,1,1,1,1,1,1),(0.,1.,2.,3.,4.,5.,6.),.UNSPECIFIED.);\nENDSEC;\n";
        let out = bsplines_to_polycurves(ifc, &Arc::new(build_entity_index(&ifc[..])))
            .expect("rewritten");
        let half = polycurve_points(&out, 5);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let last = half.last().unwrap();
        assert!(
            (half[0][0] - 1.0).abs() < 1e-9 && half[0][1].abs() < 1e-9,
            "{half:?}"
        );
        assert!(
            (last[0] - h).abs() < 1e-9 && (last[1] - h).abs() < 1e-9,
            "{half:?}"
        );
        assert_eq!(polycurve_points(&out, 7).len(), 16 * 2 + 1);

        // A trim over an unclamped spline keeps both as they are.
        let trimmed = b"DATA;\n#1=IFCCARTESIANPOINT((1.,0.));\n#2=IFCCARTESIANPOINT((1.,1.));\n#3=IFCCARTESIANPOINT((0.,1.));\n\
#6=IFCCARTESIANPOINT((2.,0.));\n\
#7=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3,#6),.UNSPECIFIED.,.F.,.F.,(1,1,1,1,1,1,1),(0.,1.,2.,3.,4.,5.,6.),.UNSPECIFIED.);\n\
#8=IFCTRIMMEDCURVE(#7,(IFCPARAMETERVALUE(2.5)),(IFCPARAMETERVALUE(3.)),.T.,.PARAMETER.);\nENDSEC;\n";
        assert!(
            bsplines_to_polycurves(trimmed, &Arc::new(build_entity_index(&trimmed[..]))).is_none()
        );
    }

    /// Largest distance between the circle of radius `r` around the origin and
    /// the chords of `pts`, which must lie on it.
    fn max_sagitta(pts: &[[f64; 3]], r: f64) -> f64 {
        let radius = |p: [f64; 3]| (p[0] * p[0] + p[1] * p[1]).sqrt();
        for &p in pts {
            assert!((radius(p) - r).abs() < 1e-9 * r, "{p:?}");
        }
        pts.windows(2)
            .map(|w| r - radius(std::array::from_fn(|i| (w[0][i] + w[1][i]) / 2.0)))
            .fold(0.0, f64::max)
    }

    #[test]
    fn bspline_tolerance_follows_the_length_unit() {
        // The same quarter circle in millimetres: 5 mm chord error is 5.0 file units.
        let ifc = b"DATA;\n#10=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#11=IFCUNITASSIGNMENT((#10));\n\
#12=IFCPROJECT('0Proj',$,'P',$,$,$,$,$,#11);\n#1=IFCCARTESIANPOINT((1000.,0.));\n#2=IFCCARTESIANPOINT((1000.,1000.));\n\
#3=IFCCARTESIANPOINT((0.,1000.));\n\
#4=IFCRATIONALBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.,(1.,0.7071067811865476,1.));\n\
ENDSEC ;\nEND-ISO-10303-21;\n";
        let out = bsplines_to_polycurves(ifc, &Arc::new(build_entity_index(&ifc[..])))
            .expect("rewritten");
        let text = std::str::from_utf8(&out).unwrap();
        assert!(text.ends_with("ENDSEC ;\nEND-ISO-10303-21;\n"), "{text}");
        let sagitta = max_sagitta(&polycurve_points(&out, 4), 1000.0);
        assert!(sagitta > 0.5 && sagitta <= 5.0, "{sagitta}");

        // Without an ENDSEC there is nowhere to put the point lists.
        let cut = &ifc[..ifc.len() - "ENDSEC ;\nEND-ISO-10303-21;\n".len()];
        assert!(bsplines_to_polycurves(cut, &Arc::new(build_entity_index(cut))).is_none());
    }

    #[test]
    fn curves_fixture_meshes() {
        use bml_core::{codegen, parser::cst_to_ast, semantic};
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/curves.bml"
        );
        let ast = cst_to_ast::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = codegen::generate_ifc_with_warnings(&model)
            .unwrap()
            .ifc_string;
        assert!(ifc.contains("IFCBSPLINECURVEWITHKNOTS"));

        let (content, index) = repair(ifc.into_bytes());
        assert!(!std::str::from_utf8(&content).unwrap().contains("BSPLINE"));
        let mut products = Vec::new();
        let mut scanner = EntityScanner::new(&content);
        while let Some((id, type_name, _, _)) = scanner.next_entity() {
            if is_rendered(type_name) {
                products.push((id, type_name.to_string()));
            }
        }
        let mut decoder = EntityDecoder::with_index(&content, index);
        let router = GeometryRouter::new();
        let empty: Vec<_> = products
            .iter()
            .filter(|(id, _)| {
                let element = decoder.decode_by_id(*id).unwrap();
                // A curtain wall's panels and mullions carry its geometry.
                !element.get(6).is_some_and(|a| a.is_null())
                    && router
                        .process_element(&element, &mut decoder)
                        .map_or(true, |m| m.indices.is_empty())
            })
            .collect();
        assert!(products.len() > 20);
        assert!(empty.is_empty(), "no mesh: {empty:?}");
    }
}
