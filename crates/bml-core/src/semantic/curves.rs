//! Curved point lists (`smooth` points, `nurbs` segments): build the exact
//! curve, tessellate it into the property's polyline and report bad curves.

use std::collections::BTreeMap;

use crate::ast::nodes::{CurveItem, Property, Value};
use crate::geometry::curve::{self, Curve, CurveError, Nurbs, PathItem};

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::{ElementCurve, ResolvedProperty, ResolvedValue};

/// Points of a curve item list as written (corners, smooth points, nurbs
/// control points); the value of a curved property until it is tessellated.
fn raw_points(items: &[CurveItem]) -> Vec<&[f64]> {
    items
        .iter()
        .flat_map(|it| match it {
            CurveItem::Corner(p) | CurveItem::Smooth(p) => vec![p.as_slice()],
            CurveItem::Nurbs { points, .. } => points.iter().map(Vec::as_slice).collect(),
        })
        .collect()
}

fn raw_ring(items: &[CurveItem]) -> Vec<(f64, f64)> {
    raw_points(items).iter().map(|p| (p[0], p[1])).collect()
}

pub(crate) fn raw_path(items: &[CurveItem], dim: u8) -> ResolvedValue {
    if dim == 3 {
        ResolvedValue::Path3D(
            raw_points(items)
                .iter()
                .map(|p| (p[0], p[1], p[2]))
                .collect(),
        )
    } else {
        ResolvedValue::Polygon(raw_ring(items))
    }
}

pub(crate) fn raw_region(outer: &[CurveItem], holes: &[Vec<CurveItem>]) -> ResolvedValue {
    ResolvedValue::PolygonWithHoles {
        outer: raw_ring(outer),
        holes: holes.iter().map(|h| raw_ring(h)).collect(),
    }
}

/// Replace each curved property of `props` (resolved from `ast`) by its
/// tessellation and return the exact curves. `owner` names the element or
/// space in diagnostics.
pub(crate) fn resolve(
    ast: &[Property],
    props: &mut [ResolvedProperty],
    owner: &str,
    tol: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, ElementCurve> {
    let mut curves = BTreeMap::new();
    for prop in ast {
        let with_value = |c: ElementCurve| (c.tessellated(tol), c);
        let built = match &prop.value {
            Value::CurvePath { items, dim: 3 } => {
                build::<3>(items, false).map(|c| with_value(ElementCurve::Path3(c)))
            }
            Value::CurvePath { items, .. } if prop.key == "path" => {
                build::<2>(items, false).map(|c| with_value(ElementCurve::Path2(c)))
            }
            Value::CurvePath { items, .. } => build::<2>(items, true).map(|outer| {
                with_value(ElementCurve::Region {
                    outer,
                    holes: Vec::new(),
                })
            }),
            Value::CurveRegion { outer, holes } => build::<2>(outer, true).and_then(|outer| {
                let holes = holes
                    .iter()
                    .map(|h| build::<2>(h, true))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(with_value(ElementCurve::Region { outer, holes }))
            }),
            _ => continue,
        };
        match built {
            Ok((value, curve)) => {
                if crosses_itself(&value, !matches!(curve, ElementCurve::Path2(_))) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::E310,
                        format!("`{}` of `{owner}` crosses itself", prop.key),
                        prop.span,
                    ));
                }
                if let Some(p) = props
                    .iter_mut()
                    .find(|p| p.key == prop.key && p.span == prop.span)
                {
                    p.value = value;
                }
                curves.insert(prop.key.clone(), curve);
            }
            Err(e) => diagnostics.push(Diagnostic::error(
                e.code,
                format!("`{}` of `{owner}`: {}", prop.key, e.message),
                prop.span,
            )),
        }
    }
    curves
}

/// A curve error with the code it reports under.
struct BuildError {
    code: DiagnosticCode,
    message: String,
}

fn nurbs_error(message: impl ToString) -> BuildError {
    BuildError {
        code: DiagnosticCode::E309,
        message: message.to_string(),
    }
}

fn build<const N: usize>(items: &[CurveItem], closed: bool) -> Result<Curve<[f64; N]>, BuildError> {
    let pt = |p: &[f64]| -> [f64; N] { std::array::from_fn(|i| p[i]) };
    let items = items
        .iter()
        .map(|it| match it {
            CurveItem::Corner(p) => Ok(PathItem::Corner(pt(p))),
            CurveItem::Smooth(p) => Ok(PathItem::Smooth(pt(p))),
            CurveItem::Nurbs {
                degree,
                points,
                weights,
                knots,
            } => {
                if degree.fract() != 0.0 {
                    return Err(nurbs_error(format!(
                        "nurbs degree must be a whole number, got {degree}"
                    )));
                }
                if *degree < 1.0 {
                    return Err(nurbs_error(CurveError::BadDegree));
                }
                let points = points.iter().map(|p| pt(p)).collect();
                Nurbs::new(*degree as usize, points, weights.clone(), knots.clone())
                    .map(PathItem::Nurbs)
                    .map_err(nurbs_error)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    curve::build(&items, closed).map_err(|e| match e {
        CurveError::TooFewPoints | CurveError::RepeatedPoint(_) => BuildError {
            code: DiagnosticCode::E308,
            message: e.to_string(),
        },
        e => nurbs_error(e),
    })
}

fn flat(points: &[[f64; 2]]) -> Vec<(f64, f64)> {
    points.iter().map(|p| (p[0], p[1])).collect()
}

impl ElementCurve {
    /// The polyline property value of the curve, chord error at most `tol`:
    /// a 3D path, a 2D path or outline, or an outline with holes.
    pub(crate) fn tessellated(&self, tol: f64) -> ResolvedValue {
        let ring = |c: &Curve<[f64; 2]>| flat(&c.tessellate(tol));
        match self {
            Self::Path3(c) => ResolvedValue::Path3D(
                c.tessellate(tol)
                    .iter()
                    .map(|p| (p[0], p[1], p[2]))
                    .collect(),
            ),
            Self::Path2(c) => ResolvedValue::Polygon(ring(c)),
            Self::Region { outer, holes } if holes.is_empty() => {
                ResolvedValue::Polygon(ring(outer))
            }
            Self::Region { outer, holes } => ResolvedValue::PolygonWithHoles {
                outer: ring(outer),
                holes: holes.iter().map(ring).collect(),
            },
        }
    }
}

/// Does a 2D path, an outline ring or a hole of `value` cross itself?
/// `closed`: a polygon is a ring (with the edge from last to first point).
fn crosses_itself(value: &ResolvedValue, closed: bool) -> bool {
    match value {
        ResolvedValue::Polygon(points) => line_crosses_itself(points, closed),
        ResolvedValue::PolygonWithHoles { outer, holes } => {
            line_crosses_itself(outer, true) || holes.iter().any(|h| line_crosses_itself(h, true))
        }
        _ => false,
    }
}

/// Two non-adjacent edges of the polyline (closed: ring) intersect.
fn line_crosses_itself(points: &[(f64, f64)], closed: bool) -> bool {
    let n = points.len();
    let edges = if closed { n } else { n.saturating_sub(1) };
    let edge = |i: usize| (points[i], points[(i + 1) % n]);
    (0..edges).any(|i| {
        let (a, b) = edge(i);
        (i + 2..edges)
            .filter(|&j| !(closed && i == 0 && j == n - 1))
            .any(|j| {
                let (c, d) = edge(j);
                segments_intersect(a, b, c, d)
            })
    })
}

fn segments_intersect(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    if a.0.max(b.0) < c.0.min(d.0)
        || c.0.max(d.0) < a.0.min(b.0)
        || a.1.max(b.1) < c.1.min(d.1)
        || c.1.max(d.1) < a.1.min(b.1)
    {
        return false;
    }
    let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
        (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
    };
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}
