//! Triangle meshes of the codegen `Shape`s (extruded, revolved and swept profiles),
//! so the scene shows exactly the body the IFC writer emits.

use super::graph::{Vertex, ear_clip_triangulate, triangulate_polygon_with_holes};
use crate::codegen::geometry::signed_area;
use crate::codegen::step_writer::{
    Profile, Shape, Solid, cross, dot, length, normalize, rotate, sub,
};
use crate::config::default_curve_tolerance;

/// Segments of a full circle.
const CIRCLE: usize = 16;

/// Mesh of `shape`, its frame relative to a storey at `elevation`.
pub(crate) fn shape_mesh(shape: &Shape, elevation: f64) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    for solid in &shape.solids {
        let to_storey = |p: [f64; 3]| {
            let q = shape.frame.to_parent(solid.position.to_parent(p));
            [q[0], q[1], q[2] + elevation]
        };
        sweep(solid, &mut |tri: [[f64; 3]; 3]| {
            let t = tri.map(to_storey);
            let n = normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])));
            for p in t {
                vertices.push(Vertex {
                    position: p.map(|v| v as f32),
                    normal: n.map(|v| v as f32),
                });
            }
        });
    }
    let indices = (0..vertices.len() as u32).collect();
    (vertices, indices)
}

/// Rings of a profile: the outer one counter-clockwise, then the holes
/// clockwise, so every side faces away from the material.
fn rings(profile: &Profile) -> Vec<Vec<(f64, f64)>> {
    let (outer, holes) = match profile {
        Profile::Rect { cx, cy, w, d } => {
            let (x, y) = (w / 2.0, d / 2.0);
            let rect = vec![
                (cx - x, cy - y),
                (cx + x, cy - y),
                (cx + x, cy + y),
                (cx - x, cy + y),
            ];
            (rect, &[][..])
        }
        Profile::Circle { cx, cy, r } => {
            let circle = (0..CIRCLE)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / CIRCLE as f64;
                    (cx + r * a.cos(), cy + r * a.sin())
                })
                .collect();
            (circle, &[][..])
        }
        Profile::Poly { outer, holes } => (outer.clone(), holes.as_slice()),
        Profile::Curve { outer, holes } => {
            let ring = |c: &crate::geometry::curve::Curve2| {
                c.tessellate(default_curve_tolerance())
                    .into_iter()
                    .map(|p| (p[0], p[1]))
                    .collect::<Vec<_>>()
            };
            let mut rings = vec![oriented(ring(outer), true)];
            rings.extend(holes.iter().map(|h| oriented(ring(h), false)));
            return rings;
        }
    };
    let mut rings = vec![oriented(outer, true)];
    rings.extend(holes.iter().map(|h| oriented(h.clone(), false)));
    rings
}

/// Frames along a swept solid's directrix, one per tessellation vertex:
/// origin, profile X (toward +Z) and profile Y (right of the curve).
fn directrix_frames(path: &crate::geometry::curve::Curve3) -> Vec<[[f64; 3]; 3]> {
    let pts = path.tessellate(default_curve_tolerance());
    let n = pts.len();
    (0..n)
        .map(|i| {
            let chord = |a: usize, b: usize| normalize(sub(pts[b], pts[a]));
            let t = match (i.checked_sub(1), (i + 1 < n).then_some(i + 1)) {
                (Some(a), Some(b)) => {
                    normalize(std::array::from_fn(|k| chord(a, i)[k] + chord(i, b)[k]))
                }
                (Some(a), None) => chord(a, i),
                (None, Some(b)) => chord(i, b),
                (None, None) => [1.0, 0.0, 0.0],
            };
            let reference = |r: [f64; 3]| {
                let s = dot(r, t);
                sub(r, t.map(|v| v * s))
            };
            let mut x = reference([0.0, 0.0, 1.0]);
            if length(x) < 1e-6 {
                x = reference([1.0, 0.0, 0.0]);
            }
            let x = normalize(x);
            [pts[i], x, cross(t, x)]
        })
        .collect()
}

/// `pts` wound counter-clockwise (`ccw`) or clockwise.
fn oriented(mut pts: Vec<(f64, f64)>, ccw: bool) -> Vec<(f64, f64)> {
    if (signed_area(&pts) > 0.0) != ccw {
        pts.reverse();
    }
    pts
}

/// Profile point (x, y) at sweep step k, in the solid's frame.
type Sweep = dyn Fn((f64, f64), usize) -> [f64; 3];

/// Emit the triangles of a solid in its position frame, outward-wound.
fn sweep(solid: &Solid, emit: &mut dyn FnMut([[f64; 3]; 3])) {
    let rings = rings(&solid.profile);
    let pts = &rings[0];
    let n = pts.len();
    if n < 3 {
        return;
    }
    let (steps, at): (usize, Box<Sweep>) = match solid.revolve {
        None if solid.directrix.is_some() => {
            let frames = directrix_frames(solid.directrix.as_ref().unwrap());
            let steps = frames.len() - 1;
            let at = move |(x, y): (f64, f64), k: usize| {
                let [o, fx, fy] = frames[k];
                std::array::from_fn(|i| o[i] + fx[i] * x + fy[i] * y)
            };
            (steps, Box::new(at))
        }
        None => {
            let depth = solid.depth;
            (1, Box::new(move |(x, y), k| [x, y, depth * k as f64]))
        }
        Some(r) => {
            let steps =
                ((r.angle.abs() / (std::f64::consts::TAU / CIRCLE as f64)).ceil() as usize).max(2);
            let d = normalize([r.dir.0, r.dir.1, 0.0]);
            let o = [r.origin.0, r.origin.1, 0.0];
            let at = move |(x, y): (f64, f64), k: usize| {
                let v = rotate(
                    [x - o[0], y - o[1], 0.0],
                    d,
                    r.angle * k as f64 / steps as f64,
                );
                std::array::from_fn(|i| o[i] + v[i])
            };
            (steps, Box::new(at))
        }
    };
    // The profile is counter-clockwise about local +Z; sides face outward when
    // the sweep leaves the plane toward +Z.
    let c = pts.iter().fold((0.0, 0.0), |a, p| {
        (a.0 + p.0 / n as f64, a.1 + p.1 / n as f64)
    });
    // A directrix sweep leaves along the profile normal (X x Y = tangent).
    let forward = solid.directrix.is_some() || at(c, 1)[2] - at(c, 0)[2] >= 0.0;
    for k in 0..steps {
        for ring in &rings {
            let n = ring.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let quad = [
                    at(ring[i], k),
                    at(ring[j], k),
                    at(ring[j], k + 1),
                    at(ring[i], k + 1),
                ];
                let [a, b, c, d] = if forward {
                    quad
                } else {
                    [quad[1], quad[0], quad[3], quad[2]]
                };
                emit([a, b, c]);
                emit([a, c, d]);
            }
        }
    }
    let caps = if rings.len() == 1 {
        ear_clip_triangulate(pts)
    } else {
        triangulate_polygon_with_holes(pts, &rings[1..])
    };
    let all: Vec<(f64, f64)> = rings.concat();
    for tri in caps {
        let [a, b, c] = tri.map(|i| all[i]);
        // earcut does not promise a winding; caps need counter-clockwise.
        let [a, b, c] = if signed_area(&[a, b, c]) < 0.0 {
            [a, c, b]
        } else {
            [a, b, c]
        };
        let (start, end) = ([a, c, b].map(|p| at(p, 0)), [a, b, c].map(|p| at(p, steps)));
        if forward {
            emit(start);
            emit(end);
        } else {
            emit([start[0], start[2], start[1]]);
            emit([end[0], end[2], end[1]]);
        }
    }
}
