//! Exact B-spline geometry: profiles, axes and swept-disk directrices whose
//! curves are B-splines read back as `path:` / `boundary:` with their exact
//! curve. Geometry without a B-spline stays with the plain reader.

use std::collections::{BTreeMap, HashMap};

use super::ifc_reader::{
    IfcEntity, parse_coordinate_list, parse_entity_args, parse_ref, parse_ref_list, prop,
    resolve_axis2placement3d_directions, resolve_axis2placement3d_location,
    resolve_cartesian_point, resolve_direction,
};
use super::mesh_fit::extrusion;
use crate::codegen::geometry::signed_area;
use crate::config::default_curve_tolerance;
use crate::geometry::curve::{
    Curve, Curve2, Curve3, CurveError, Nurbs, Point, Segment, dist, expand_knots,
};
use crate::semantic::model::{ElementCurve, ElementType, ResolvedProperty, ResolvedValue};

type Map<'a> = HashMap<u64, &'a IfcEntity>;
type P3 = [f64; 3];
type Exact = (Vec<ResolvedProperty>, BTreeMap<String, ElementCurve>);

/// Geometry properties and curves of a product (`et` is None for a space;
/// its flag is false for an element nested in a host) whose axis, profile or
/// swept-disk directrix holds a B-spline.
pub(super) fn exact_geometry(
    text: &str,
    et: Option<(ElementType, bool)>,
    m: &Map,
    s: f64,
) -> Option<Exact> {
    let args = parse_entity_args(text)?;
    let place = local_placement(parse_ref(args.get(5)?.trim()), m, s)?;
    let (axis, body) = representations(parse_ref(args.get(6)?.trim()), m)?;
    let body_entity = m.get(&body?)?;
    let b = parse_entity_args(&body_entity.raw_text)?;
    let tol = default_curve_tolerance();
    let mut props = Vec::new();
    let mut curves = BTreeMap::new();
    let flat2 = |c: &Curve2| -> Vec<(f64, f64)> {
        c.tessellate(tol).iter().map(|p| (p[0], p[1])).collect()
    };
    match body_entity.entity_type.as_str() {
        "IFCSWEPTDISKSOLID" => {
            // A hollow tube or a sweep over part of the directrix stays with the mesh fit.
            if b.get(2..5)?.iter().any(|a| a.trim() != "$") {
                return None;
            }
            let path = curve(parse_ref(b.first()?.trim()), m, s)?.map(|p| place.apply(p));
            let r = number(b.get(1)?)? * s;
            if !has_spline(&path) {
                return None;
            }
            match et?.0 {
                ElementType::Duct | ElementType::Pipe => {
                    let path = ElementCurve::Path3(path);
                    props.push(prop("path", path.tessellated(tol)));
                    props.push(prop("diameter", ResolvedValue::Number(2.0 * r)));
                    curves.insert("path".to_string(), path);
                }
                ElementType::Beam | ElementType::Member => {
                    let z = path.segments[0].start()[2];
                    if !control_points(&path).all(|p| (p[2] - z).abs() < 1e-6) {
                        return None;
                    }
                    let path = ElementCurve::Path2(path.map(|p| [p[0], p[1]]));
                    props.push(prop("path", path.tessellated(tol)));
                    props.push(profile("circle", vec![r]));
                    props.push(prop("z_offset", ResolvedValue::Number(z)));
                    curves.insert("path".to_string(), path);
                }
                _ => return None,
            }
        }
        "IFCEXTRUDEDAREASOLID" if b.len() >= 4 => {
            let solid = match b[1].trim() {
                "$" => Place::IDENTITY,
                pos => Place::of(parse_ref(pos), m, s)?,
            };
            let dir = resolve_direction(parse_ref(b[2].trim()), m)?;
            if dir.0.abs() > 1e-9 || dir.1.abs() > 1e-9 || dir.2 <= 0.0 {
                return None;
            }
            let depth = number(&b[3])? * s;
            let (outer, holes) = area(parse_ref(b[0].trim()), m, s)?;
            let plan = |c: &Curve3| -> Curve2 {
                c.map(|p| {
                    let q = place.apply(solid.apply(p));
                    [q[0], q[1]]
                })
            };
            let (outer, holes): (Curve2, Vec<Curve2>) =
                (plan(&outer), holes.iter().map(plan).collect());
            let lo = place.origin[2] + solid.origin[2];
            match (et.map(|e| e.0), axis.and_then(|id| curve(id, m, s))) {
                (
                    Some(
                        t @ (ElementType::Wall
                        | ElementType::Railing
                        | ElementType::Beam
                        | ElementType::Member),
                    ),
                    Some(axis),
                ) if has_spline(&axis) => {
                    let axis: Curve2 = axis.map(|p| {
                        let q = place.apply(p);
                        [q[0], q[1]]
                    });
                    let plan_area = signed_area(&flat2(&outer)).abs()
                        - holes
                            .iter()
                            .map(|h| signed_area(&flat2(h)).abs())
                            .sum::<f64>();
                    let width = (plan_area / axis.length() * 1000.0).round() / 1000.0;
                    let axis = ElementCurve::Path2(axis);
                    props.push(prop("path", axis.tessellated(tol)));
                    if matches!(t, ElementType::Wall | ElementType::Railing) {
                        props.push(prop("height", ResolvedValue::Number(depth)));
                        props.push(prop("thickness", ResolvedValue::Number(width)));
                        props.push(prop("z_offset", ResolvedValue::Number(lo)));
                    } else {
                        props.push(profile("rectangle", vec![width, depth]));
                        props.push(prop("z_offset", ResolvedValue::Number(lo + depth / 2.0)));
                    }
                    curves.insert("path".to_string(), axis);
                }
                _ if has_spline(&outer) || holes.iter().any(has_spline) => {
                    let region = ElementCurve::Region { outer, holes };
                    props.push(prop("boundary", region.tessellated(tol)));
                    match et {
                        Some((t, top)) => extrusion(t, &mut props, lo, lo + depth, top),
                        None => {
                            props.push(prop("z_offset", ResolvedValue::Number(lo)));
                            props.push(prop("height", ResolvedValue::Number(depth)));
                        }
                    }
                    curves.insert("boundary".to_string(), region);
                }
                _ => return None,
            }
        }
        _ => return None,
    }
    Some((props, curves))
}

/// Upright placement: origin and plan rotation of its local X axis.
#[derive(Clone, Copy)]
struct Place {
    origin: P3,
    x: [f64; 2],
}

impl Place {
    const IDENTITY: Place = Place {
        origin: [0.0; 3],
        x: [1.0, 0.0],
    };

    /// An IFCAXIS2PLACEMENT3D; None when its Z axis is not up.
    fn of(id: u64, m: &Map, s: f64) -> Option<Self> {
        let (x, y, z) = resolve_axis2placement3d_location(id, m, s)?;
        let (axis, rx) = resolve_axis2placement3d_directions(id, m)?;
        let len = rx.0.hypot(rx.1);
        if axis.0.abs() > 1e-9 || axis.1.abs() > 1e-9 || axis.2 <= 0.0 || len < 1e-12 {
            return None;
        }
        Some(Self {
            origin: [x, y, z],
            x: [rx.0 / len, rx.1 / len],
        })
    }

    fn apply(&self, p: P3) -> P3 {
        let [c, sn] = self.x;
        [
            self.origin[0] + c * p[0] - sn * p[1],
            self.origin[1] + sn * p[0] + c * p[1],
            self.origin[2] + p[2],
        ]
    }
}

/// The product's own IFCLOCALPLACEMENT (relative to its storey).
fn local_placement(id: u64, m: &Map, s: f64) -> Option<Place> {
    let e = m.get(&id)?;
    if e.entity_type != "IFCLOCALPLACEMENT" {
        return None;
    }
    Place::of(
        parse_ref(parse_entity_args(&e.raw_text)?.get(1)?.trim()),
        m,
        s,
    )
}

/// First item of the 'Axis' representation and the single item of 'Body'.
fn representations(id: u64, m: &Map) -> Option<(Option<u64>, Option<u64>)> {
    let e = m.get(&id)?;
    if e.entity_type != "IFCPRODUCTDEFINITIONSHAPE" {
        return None;
    }
    let (mut axis, mut body) = (None, None);
    for rep in parse_ref_list(parse_entity_args(&e.raw_text)?.last()?.trim()) {
        let Some(r) = m
            .get(&rep)
            .filter(|r| r.entity_type == "IFCSHAPEREPRESENTATION")
        else {
            continue;
        };
        let Some(a) = parse_entity_args(&r.raw_text).filter(|a| a.len() >= 4) else {
            continue;
        };
        let items = parse_ref_list(a[3].trim());
        match a[1].trim() {
            "'Axis'" => axis = items.first().copied(),
            "'Body'" if items.len() == 1 => body = Some(items[0]),
            _ => {}
        }
    }
    Some((axis, body))
}

/// Outer curve and holes of an arbitrary profile, closed.
fn area(id: u64, m: &Map, s: f64) -> Option<(Curve3, Vec<Curve3>)> {
    let e = m.get(&id)?;
    let a = parse_entity_args(&e.raw_text)?;
    let closed = |id: u64| {
        let mut c = curve(id, m, s)?;
        let (first, last) = (c.segments.first()?.start(), c.segments.last()?.end());
        if dist(first, last) > 1e-6 {
            return None;
        }
        c.closed = true;
        Some(c)
    };
    let outer = closed(parse_ref(a.get(2)?.trim()))?;
    let holes = match e.entity_type.as_str() {
        "IFCARBITRARYCLOSEDPROFILEDEF" => Vec::new(),
        "IFCARBITRARYPROFILEDEFWITHVOIDS" => parse_ref_list(a.get(3)?.trim())
            .into_iter()
            .map(closed)
            .collect::<Option<_>>()?,
        _ => return None,
    };
    Some((outer, holes))
}

/// A polyline, B-spline, parameter-trimmed B-spline or composite of these,
/// in the coordinates of its representation. Other curves give None.
fn curve(id: u64, m: &Map, s: f64) -> Option<Curve3> {
    let e = m.get(&id)?;
    let a = parse_entity_args(&e.raw_text)?;
    let open = |segments: Vec<Segment<P3>>| Curve {
        segments,
        closed: false,
    };
    match e.entity_type.as_str() {
        "IFCPOLYLINE" => Some(open(lines(&points(a.first()?, m, s)?))),
        "IFCBSPLINECURVEWITHKNOTS" | "IFCRATIONALBSPLINECURVEWITHKNOTS" => {
            bspline(&a, m, s).map(open)
        }
        "IFCTRIMMEDCURVE" if a.len() >= 4 => {
            let basis = curve(parse_ref(a[0].trim()), m, s)?;
            let [Segment::Nurbs(n)] = basis.segments.as_slice() else {
                return None;
            };
            let (u0, u1) = (parameter(&a[1])?, parameter(&a[2])?);
            let seg = Segment::Nurbs(n.trim(u0.min(u1), u0.max(u1)));
            Some(open(vec![match a[3].trim() {
                ".F." => seg.reversed(),
                _ => seg,
            }]))
        }
        "IFCCOMPOSITECURVE" => {
            let mut segments = Vec::new();
            for id in parse_ref_list(a.first()?.trim()) {
                let seg = m
                    .get(&id)
                    .filter(|e| e.entity_type == "IFCCOMPOSITECURVESEGMENT")?;
                let sa = parse_entity_args(&seg.raw_text)?;
                let part = curve(parse_ref(sa.get(2)?.trim()), m, s)?;
                if sa.get(1)?.trim() == ".F." {
                    segments.extend(part.segments.into_iter().rev().map(Segment::reversed));
                } else {
                    segments.extend(part.segments);
                }
            }
            (!segments.is_empty()).then(|| open(segments))
        }
        _ => None,
    }
}

/// B-spline from its IFC arguments: knots expanded from their multiplicities.
/// An unclamped spline comes back as lines through points sampled on it.
fn bspline(a: &[String], m: &Map, s: f64) -> Option<Vec<Segment<P3>>> {
    let degree: usize = a.first()?.trim().parse().ok()?;
    let pts = points(a.get(1)?, m, s)?;
    let mults = parse_coordinate_list(a.get(5)?);
    let values = parse_coordinate_list(a.get(6)?);
    let weights = a.get(8).map(|w| parse_coordinate_list(w));
    if mults.len() != values.len() || mults.len() < 2 {
        return None;
    }
    // An interior knot repeated more than `degree` times breaks the curve.
    if mults[1..mults.len() - 1].iter().any(|&k| k > degree as f64) {
        return None;
    }
    let mults: Vec<usize> = mults.iter().map(|&k| k as usize).collect();
    let knots = expand_knots(&values, &mults);
    match Nurbs::new(degree, pts.clone(), weights.clone(), Some(knots.clone())) {
        Ok(n) => Some(vec![Segment::Nurbs(n)]),
        // `Nurbs::new` checked the weights and knot count before clamping.
        Err(CurveError::UnclampedKnots) => {
            let n = pts.len();
            let mut weights = weights.unwrap_or_default();
            weights.resize(n, 1.0);
            let spline = Nurbs {
                degree,
                points: pts,
                weights,
                knots,
            };
            Some(lines(&spline.samples()))
        }
        Err(_) => None,
    }
}

fn lines(pts: &[P3]) -> Vec<Segment<P3>> {
    pts.windows(2).map(|w| Segment::Line(w[0], w[1])).collect()
}

/// Points of a reference list like `(#1,#2)`.
fn points(list: &str, m: &Map, s: f64) -> Option<Vec<P3>> {
    parse_ref_list(list.trim())
        .into_iter()
        .map(|id| resolve_cartesian_point(id, m, s).map(|(x, y, z)| [x, y, z]))
        .collect()
}

/// `(IFCPARAMETERVALUE(0.5))`; other trims (cartesian points) give None.
fn parameter(arg: &str) -> Option<f64> {
    let inner = arg.split("IFCPARAMETERVALUE(").nth(1)?;
    number(inner.split(')').next()?)
}

fn number(arg: &str) -> Option<f64> {
    arg.trim().trim_end_matches('.').parse().ok()
}

fn has_spline<P: Point>(c: &Curve<P>) -> bool {
    c.segments.iter().any(|s| matches!(s, Segment::Nurbs(_)))
}

fn control_points(c: &Curve3) -> impl Iterator<Item = P3> + '_ {
    c.segments.iter().flat_map(|s| match s {
        Segment::Line(a, b) => vec![*a, *b],
        Segment::Arc { from, via, to } => vec![*from, *via, *to],
        Segment::Nurbs(n) => n.points.clone(),
    })
}

fn profile(kind: &str, params: Vec<f64>) -> ResolvedProperty {
    prop(
        "profile",
        ResolvedValue::Profile {
            profile_type: kind.to_string(),
            params,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompile::ifc_reader::parse_all_entities;

    fn read(step: &str, id: u64) -> Option<Curve3> {
        let entities = parse_all_entities(step);
        let m: Map = entities.iter().map(|e| (e.id, e)).collect();
        curve(id, &m, 1.0)
    }

    const POINTS: &str = "#1=IFCCARTESIANPOINT((0.,0.));
#2=IFCCARTESIANPOINT((1.,2.));
#3=IFCCARTESIANPOINT((3.,2.));
#4=IFCCARTESIANPOINT((4.,0.));
";

    #[test]
    fn clamped_bspline_reads_as_nurbs_and_trims() {
        let step = format!(
            "{POINTS}#5=IFCBSPLINECURVEWITHKNOTS(3,(#1,#2,#3,#4),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);
#6=IFCTRIMMEDCURVE(#5,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(1.)),.F.,.PARAMETER.);"
        );
        let c = read(&step, 6).unwrap();
        let [Segment::Nurbs(n)] = c.segments.as_slice() else {
            panic!("{c:?}")
        };
        assert_eq!(n.points[0], [4.0, 0.0, 0.0], "sense .F. reverses");
    }

    #[test]
    fn trimmed_bspline_ends_at_its_trim_parameters() {
        let step = format!(
            "{POINTS}#5=IFCBSPLINECURVEWITHKNOTS(3,(#1,#2,#3,#4),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);
#6=IFCTRIMMEDCURVE(#5,(IFCPARAMETERVALUE(0.25)),(IFCPARAMETERVALUE(0.75)),.T.,.PARAMETER.);"
        );
        let full = read(&step, 5).unwrap();
        let [Segment::Nurbs(basis)] = full.segments.as_slice() else {
            panic!()
        };
        let trimmed = read(&step, 6).unwrap();
        let [Segment::Nurbs(n)] = trimmed.segments.as_slice() else {
            panic!()
        };
        for (p, u) in [(n.points[0], 0.25), (*n.points.last().unwrap(), 0.75)] {
            assert!(dist(p, basis.eval(u)) < 1e-9, "{p:?} at {u}");
        }
    }

    #[test]
    fn unclamped_bspline_reads_as_corners() {
        let step = format!(
            "{POINTS}#5=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3,#4),.UNSPECIFIED.,.F.,.F.,(1,1,1,1,1,1,1),(0.,1.,2.,3.,4.,5.,6.),.UNSPECIFIED.);"
        );
        let c = read(&step, 5).unwrap();
        assert!(c.segments.iter().all(|s| matches!(s, Segment::Line(..))));
        assert!(c.segments.len() > 4);
    }

    #[test]
    fn interior_knot_above_degree_is_rejected() {
        let step = format!(
            "{POINTS}#5=IFCBSPLINECURVEWITHKNOTS(1,(#1,#2,#3,#4),.UNSPECIFIED.,.F.,.F.,(2,2,2),(0.,0.5,1.),.UNSPECIFIED.);"
        );
        assert!(read(&step, 5).is_none());
    }
}
