use crate::geometry::axis::WallAxis;
use crate::semantic::model::{ResolvedProperty, number};

/// Compute the length of a wall axis from two 2D points.
pub fn wall_length(from: (f64, f64), to: (f64, f64)) -> f64 {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    (dx * dx + dy * dy).sqrt()
}

/// Compute the rotation angle (in radians) of a wall axis from two 2D points.
/// The angle is measured counter-clockwise from the positive X axis.
pub fn wall_angle(from: (f64, f64), to: (f64, f64)) -> f64 {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    dy.atan2(dx)
}

/// Infer a space boundary polygon from the walls that enclose the space.
///
/// Collects all wall segments (from their axis properties), finds their
/// intersections, and attempts to form a closed polygon. This is used
/// when a space has no explicit `boundary:` property.
///
/// Each wall is represented as a line segment from `from` to `to`.
/// We build a graph of intersection points and walk the boundary.
pub type WallSegment = ((f64, f64), (f64, f64));

pub fn infer_boundary_from_walls(walls: &[WallSegment]) -> Option<Vec<(f64, f64)>> {
    if walls.len() < 3 {
        return None;
    }

    // Collect all wall endpoints and intersection points
    let eps = 0.01; // proximity threshold (1cm)

    // Collect unique points from wall endpoints
    let mut points: Vec<(f64, f64)> = Vec::new();
    for &(from, to) in walls {
        add_unique_point(&mut points, from, eps);
        add_unique_point(&mut points, to, eps);
    }

    // Also add intersection points between wall segments
    for i in 0..walls.len() {
        for j in (i + 1)..walls.len() {
            if let Some(pt) = segment_intersection(walls[i].0, walls[i].1, walls[j].0, walls[j].1) {
                add_unique_point(&mut points, pt, eps);
            }
        }
    }

    if points.len() < 3 {
        return None;
    }

    // Sort points by angle from centroid to form a convex-hull-like boundary
    let cx: f64 = points.iter().map(|p| p.0).sum::<f64>() / points.len() as f64;
    let cy: f64 = points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64;

    points.sort_by(|a, b| {
        let angle_a = (a.1 - cy).atan2(a.0 - cx);
        let angle_b = (b.1 - cy).atan2(b.0 - cx);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Remove duplicate adjacent points
    points.dedup_by(|a, b| (a.0 - b.0).abs() < eps && (a.1 - b.1).abs() < eps);

    if points.len() >= 3 {
        Some(points)
    } else {
        None
    }
}

/// Add a point to the list if no existing point is within `eps` distance.
fn add_unique_point(points: &mut Vec<(f64, f64)>, p: (f64, f64), eps: f64) {
    for existing in points.iter() {
        if (existing.0 - p.0).abs() < eps && (existing.1 - p.1).abs() < eps {
            return;
        }
    }
    points.push(p);
}

/// Compute the intersection point of two line segments, if they intersect.
/// Returns None if segments are parallel or don't intersect within their extent.
fn segment_intersection(
    a1: (f64, f64),
    a2: (f64, f64),
    b1: (f64, f64),
    b2: (f64, f64),
) -> Option<(f64, f64)> {
    let d1x = a2.0 - a1.0;
    let d1y = a2.1 - a1.1;
    let d2x = b2.0 - b1.0;
    let d2y = b2.1 - b1.1;

    let denom = d1x * d2y - d1y * d2x;
    if denom.abs() < 1e-12 {
        return None; // Parallel
    }

    let t = ((b1.0 - a1.0) * d2y - (b1.1 - a1.1) * d2x) / denom;
    let u = ((b1.0 - a1.0) * d1y - (b1.1 - a1.1) * d1x) / denom;

    // Extend slightly beyond segment endpoints to catch near-misses
    let margin = 0.01;
    if t >= -margin && t <= 1.0 + margin && u >= -margin && u <= 1.0 + margin {
        Some((a1.0 + t * d1x, a1.1 + t * d1y))
    } else {
        None
    }
}

/// Even-odd test of a point against a polygon ring.
pub(crate) fn point_in_polygon(px: f64, py: f64, poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Unsigned polygon area (shoelace).
pub(crate) fn polygon_area(poly: &[(f64, f64)]) -> f64 {
    signed_area(poly).abs()
}

/// Signed polygon area, positive counter-clockwise.
pub(crate) fn signed_area(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        / 2.0
}

/// A local outline rotated by `rotation` degrees and moved to `(x, y)`.
pub(crate) fn placed(local: Vec<(f64, f64)>, (x, y): (f64, f64), rotation: f64) -> Vec<(f64, f64)> {
    let (sin, cos) = rotation.to_radians().sin_cos();
    local
        .into_iter()
        .map(|(u, v)| (x + u * cos - v * sin, y + u * sin + v * cos))
        .collect()
}

/// A straight stair flight (`Specs_V0_3.md` §23): `risers` steps of
/// `riser` x `tread` across `width`, the top tread flush with the level it
/// arrives at.
pub(crate) struct Flight {
    pub(crate) width: f64,
    pub(crate) risers: usize,
    pub(crate) riser: f64,
    pub(crate) tread: f64,
    /// Thickness under the steps, perpendicular to the pitch; 0 is solid to the base.
    pub(crate) waist: f64,
}

impl Flight {
    /// The flight of a stair; the risers divide `storey_height` by default.
    pub(crate) fn of(p: &[ResolvedProperty], storey_height: f64) -> Self {
        let risers = number(p, "riser_count").unwrap_or(18.0).round().max(1.0);
        Flight {
            width: number(p, "width").unwrap_or(1.2),
            risers: risers as usize,
            riser: number(p, "riser_height").unwrap_or(storey_height / risers),
            tread: number(p, "tread_depth").unwrap_or(0.27),
            waist: number(p, "waist_thickness").unwrap_or(0.0),
        }
    }

    /// Side profile, counter-clockwise: `u` along the walking direction, `v`
    /// up from the flight base.
    pub(crate) fn profile(&self) -> Vec<(f64, f64)> {
        let (t, r) = (self.tread, self.riser);
        let n = self.risers as f64;
        let (run, rise) = (n * t, n * r);
        // The underside runs parallel to the pitch line through the inner corners.
        let drop = self.waist * r.hypot(t) / t;
        let mut pts = if self.waist > 0.0 && drop < rise {
            vec![(0.0, 0.0), (drop * t / r, 0.0), (run, rise - drop)]
        } else {
            vec![(0.0, 0.0), (run, 0.0)]
        };
        for k in (1..=self.risers).rev() {
            let k = k as f64;
            pts.push((k * t, k * r));
            pts.push(((k - 1.0) * t, k * r));
        }
        pts
    }
}

/// Default thickness of a `Covering` in metres.
pub(crate) const COVERING_THICKNESS: f64 = 0.02;

/// Plan outline of a wall `thickness` thick centred on its axis, as plan tuples.
pub(crate) fn wall_outline(axis: &WallAxis, thickness: f64) -> Vec<(f64, f64)> {
    axis.outline(thickness)
        .into_iter()
        .map(|p| (p[0], p[1]))
        .collect()
}

/// Compute the arc length for a curved wall defined by from, via, to points.
pub fn arc_length(from: (f64, f64), via: (f64, f64), to: (f64, f64)) -> f64 {
    // Compute the circle through the three points
    let ax = from.0;
    let ay = from.1;
    let bx = via.0;
    let by = via.1;
    let cx = to.0;
    let cy = to.1;

    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-10 {
        // Collinear — return straight-line distance
        return wall_length(from, to);
    }

    let ux = ((ax * ax + ay * ay) * (by - cy)
        + (bx * bx + by * by) * (cy - ay)
        + (cx * cx + cy * cy) * (ay - by))
        / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx)
        + (bx * bx + by * by) * (ax - cx)
        + (cx * cx + cy * cy) * (bx - ax))
        / d;

    let radius = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();

    // Compute angles
    let start_angle = (from.1 - uy).atan2(from.0 - ux);
    let end_angle = (to.1 - uy).atan2(to.0 - ux);
    let via_angle = (via.1 - uy).atan2(via.0 - ux);

    let mut sweep = end_angle - start_angle;
    let via_offset = normalize_angle_geo(via_angle - start_angle);
    let end_offset = normalize_angle_geo(sweep);

    if (via_offset > 0.0) != (end_offset > 0.0) || via_offset.abs() > end_offset.abs() {
        if sweep > 0.0 {
            sweep -= 2.0 * std::f64::consts::PI;
        } else {
            sweep += 2.0 * std::f64::consts::PI;
        }
    }

    radius * sweep.abs()
}

fn normalize_angle_geo(angle: f64) -> f64 {
    let mut a = angle;
    while a > std::f64::consts::PI {
        a -= 2.0 * std::f64::consts::PI;
    }
    while a < -std::f64::consts::PI {
        a += 2.0 * std::f64::consts::PI;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wall_along_x_axis() {
        let length = wall_length((0.0, 0.0), (5.0, 0.0));
        assert!((length - 5.0).abs() < 1e-10);

        let angle = wall_angle((0.0, 0.0), (5.0, 0.0));
        assert!((angle - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_wall_along_y_axis() {
        let length = wall_length((0.0, 0.0), (0.0, 5.0));
        assert!((length - 5.0).abs() < 1e-10);

        let angle = wall_angle((0.0, 0.0), (0.0, 5.0));
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    }

    #[test]
    fn test_wall_diagonal() {
        let length = wall_length((0.0, 0.0), (3.0, 4.0));
        assert!((length - 5.0).abs() < 1e-10);
    }

    // --- Phase 7c geometry tests ---

    #[test]
    fn test_infer_boundary_from_rectangular_walls() {
        let walls = vec![
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 8.0)),
            ((10.0, 8.0), (0.0, 8.0)),
            ((0.0, 8.0), (0.0, 0.0)),
        ];
        let boundary = infer_boundary_from_walls(&walls).expect("should infer boundary");
        assert!(
            boundary.len() >= 4,
            "rectangular room should infer at least 4 boundary points, got {}",
            boundary.len()
        );
    }

    #[test]
    fn test_infer_boundary_from_l_shaped_walls() {
        let walls = vec![
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 5.0)),
            ((10.0, 5.0), (5.0, 5.0)),
            ((5.0, 5.0), (5.0, 8.0)),
            ((5.0, 8.0), (0.0, 8.0)),
            ((0.0, 8.0), (0.0, 0.0)),
        ];
        let boundary = infer_boundary_from_walls(&walls).expect("should infer boundary");
        assert!(
            boundary.len() >= 6,
            "L-shaped room should infer at least 6 boundary points, got {}",
            boundary.len()
        );
    }

    #[test]
    fn test_arc_length_semicircle() {
        let from = (-1.0, 0.0);
        let via = (0.0, 1.0);
        let to = (1.0, 0.0);
        let len = arc_length(from, via, to);
        // Semicircle with radius 1 has arc length pi
        assert!(
            (len - std::f64::consts::PI).abs() < 0.1,
            "semicircle arc length should be ~pi, got {}",
            len
        );
    }

    #[test]
    fn test_segment_intersection() {
        // Perpendicular segments that intersect at (1, 1)
        let p = segment_intersection((0.0, 1.0), (2.0, 1.0), (1.0, 0.0), (1.0, 2.0));
        assert!(p.is_some(), "perpendicular segments should intersect");
        let (x, y) = p.unwrap();
        assert!((x - 1.0).abs() < 1e-10);
        assert!((y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_segment_intersection_parallel() {
        // Parallel segments
        let p = segment_intersection((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0));
        assert!(p.is_none(), "parallel segments should not intersect");
    }
}
