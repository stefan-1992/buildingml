//! Plan axis of a wall or other linear element as a polyline: from `path:`
//! (exact curve when written with `smooth` / `nurbs`), a `from` / `via` / `to`
//! arc, or a straight `from` / `to`. Every wall consumer measures openings,
//! outlines and cells along this one polyline.

use super::curve::{Circle, Curve2, CurveError, PathItem, Point, Segment, build, dist, norm};
use crate::semantic::model::{ElementCurve, ResolvedElement, ResolvedValue};

/// Two points closer than this are the same point.
const SAME: f64 = 1e-9;

/// Least spacing of the points the exact wall faces are fitted through.
pub const FACE_SPACING: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct WallAxis {
    pub points: Vec<[f64; 2]>,
    /// Distance along the axis of each point.
    cum: Vec<f64>,
    /// Whether each point is a sharp corner (ends and junctions of curve
    /// segments, every vertex of a plain polyline) rather than a sample of a curve.
    corner: Vec<bool>,
    /// Unit directions at the start and end, exact for curves.
    ends: [[f64; 2]; 2],
}

fn unit(v: [f64; 2]) -> [f64; 2] {
    let n = norm(v);
    if n < SAME {
        [1.0, 0.0]
    } else {
        [v[0] / n, v[1] / n]
    }
}

impl WallAxis {
    /// Axis of `elem`: `path:` first, else `from` / `via` / `to`, else `from` / `to`.
    /// Arcs are tessellated to chord error `tol`.
    pub fn of(elem: &ResolvedElement, tol: f64) -> Option<Self> {
        let find = |key: &str| elem.properties.iter().find(|p| p.key == key);
        if let Some(ResolvedValue::Polygon(pts)) = find("path").map(|p| &p.value) {
            let pts: Vec<[f64; 2]> = pts.iter().map(|p| [p.0, p.1]).collect();
            return match elem.curve("path") {
                Some(ElementCurve::Path2(c)) => Self::on_curve(pts, c),
                _ => Self::polyline(pts),
            };
        }
        let ResolvedValue::Axis { from, to, via, .. } = &find("from")?.value else {
            return None;
        };
        let (from, to) = ([from.0, from.1], [to.0, to.1]);
        if let Some(via) = via.map(|v| [v.0, v.1])
            && Circle::through(from, via, to).is_some()
        {
            let c = Curve2 {
                segments: vec![Segment::Arc { from, via, to }],
                closed: false,
            };
            return Self::on_curve(c.tessellate(tol), &c);
        }
        Self::polyline(vec![from, to])
    }

    /// First and last point of the axis of `elem` when it is straight
    /// ([`is_straight`](Self::is_straight)); a plain `from` / `to` skips
    /// building the axis.
    pub fn straight_ends(elem: &ResolvedElement, tol: f64) -> Option<[[f64; 2]; 2]> {
        let find = |key: &str| {
            elem.properties
                .iter()
                .find(|p| p.key == key)
                .map(|p| &p.value)
        };
        if !matches!(find("path"), Some(ResolvedValue::Polygon(_)))
            && let Some(ResolvedValue::Axis {
                from,
                to,
                via: None,
                ..
            }) = find("from")
        {
            let (a, b) = ([from.0, from.1], [to.0, to.1]);
            // A zero-length axis keeps its first point at both ends.
            return Some(if dist(a, b) < SAME { [a, a] } else { [a, b] });
        }
        let axis = Self::of(elem, tol).filter(Self::is_straight)?;
        Some([axis.points[0], *axis.points.last().unwrap()])
    }

    /// Plain polyline: every vertex a corner.
    pub fn polyline(points: Vec<[f64; 2]>) -> Option<Self> {
        let n = points.len();
        Self::build(points, vec![true; n], None)
    }

    /// Polyline `pts` sampled from `curve`: only the segment junctions are corners.
    fn on_curve(pts: Vec<[f64; 2]>, curve: &Curve2) -> Option<Self> {
        let joints: Vec<[f64; 2]> = curve.segments.iter().map(Segment::start).collect();
        let n = pts.len();
        let corner = pts
            .iter()
            .enumerate()
            .map(|(i, p)| i == 0 || i + 1 == n || joints.iter().any(|j| dist(*p, *j) < SAME))
            .collect();
        let lengths: Vec<f64> = curve.segments.iter().map(Segment::length).collect();
        let ends = [
            curve.tangent_at(0.0),
            curve.tangent_at(lengths.iter().sum()),
        ];
        let mut axis = Self::build(pts, corner, Some(ends))?;
        // Measure along the curve, not its chords, so that lengths and offsets
        // match the exact curve (the chords are a few mm shorter).
        let corners: Vec<usize> = (0..axis.points.len()).filter(|&i| axis.corner[i]).collect();
        if corners.len() == curve.segments.len() + 1 {
            let chords = axis.cum.clone();
            for (w, len) in corners.windows(2).zip(&lengths) {
                let (a, b) = (w[0], w[1]);
                let chord = chords[b] - chords[a];
                let k = if chord > SAME { len / chord } else { 1.0 };
                for i in a + 1..=b {
                    axis.cum[i] = axis.cum[a] + (chords[i] - chords[a]) * k;
                }
            }
        }
        Some(axis)
    }

    fn build(
        points: Vec<[f64; 2]>,
        corner: Vec<bool>,
        ends: Option<[[f64; 2]; 2]>,
    ) -> Option<Self> {
        let (mut pts, mut flags) = (Vec::new(), Vec::new());
        for (p, c) in points.into_iter().zip(corner) {
            match pts.last() {
                Some(&q) if dist(p, q) < SAME => {
                    let last = flags.len() - 1;
                    flags[last] |= c;
                }
                _ => {
                    pts.push(p);
                    flags.push(c);
                }
            }
        }
        let first = *pts.first()?;
        if pts.len() == 1 {
            // Zero-length axis: keep both ends so consumers still see a wall.
            pts.push(first);
            flags.push(true);
        }
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum.last().unwrap() + dist(w[0], w[1]));
        }
        let n = pts.len();
        let ends = ends.unwrap_or([unit(pts[1].sub(pts[0])), unit(pts[n - 1].sub(pts[n - 2]))]);
        Some(Self {
            points: pts,
            cum,
            corner: flags,
            ends,
        })
    }

    pub fn length(&self) -> f64 {
        *self.cum.last().unwrap()
    }

    /// Index `i` of the chord `points[i]..points[i + 1]` holding `s`; a vertex
    /// belongs to the chord after it, the end to the last chord.
    fn chord(&self, s: f64) -> usize {
        let n = self.points.len() - 1;
        (0..n).find(|&i| s < self.cum[i + 1]).unwrap_or(n - 1)
    }

    /// Point `s` along the axis (clamped to its ends).
    pub fn point_at(&self, s: f64) -> [f64; 2] {
        let s = s.clamp(0.0, self.length());
        let i = self.chord(s);
        let (a, b) = (self.points[i], self.points[i + 1]);
        let l = self.cum[i + 1] - self.cum[i];
        let t = if l > 0.0 { (s - self.cum[i]) / l } else { 0.0 };
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }

    /// Unit direction of the chord holding `s`.
    pub fn tangent_at(&self, s: f64) -> [f64; 2] {
        let i = self.chord(s.clamp(0.0, self.length()));
        unit(self.points[i + 1].sub(self.points[i]))
    }

    /// Distance along the axis of the point on it closest to `p`.
    pub fn project(&self, p: [f64; 2]) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for (i, w) in self.points.windows(2).enumerate() {
            let (d, v) = (w[1].sub(w[0]), p.sub(w[0]));
            let l2 = d[0] * d[0] + d[1] * d[1];
            let t = if l2 > 0.0 {
                ((v[0] * d[0] + v[1] * d[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let gap = norm(v.sub(d.scale(t)));
            if gap < best.0 {
                best = (gap, self.cum[i] + t * (self.cum[i + 1] - self.cum[i]));
            }
        }
        best.1
    }

    /// Origin and angle of a frame along the tangent at `s` whose local x = `s`
    /// lands on `point_at(s)`. For a straight axis: `from` and the axis angle.
    pub fn frame(&self, s: f64) -> ((f64, f64), f64) {
        let (p, t) = (self.point_at(s), self.tangent_at(s));
        ((p[0] - t[0] * s, p[1] - t[1] * s), t[1].atan2(t[0]))
    }

    /// Open and every point on the line from the first to the last point, in
    /// order (an axis that doubles back is not straight).
    pub fn is_straight(&self) -> bool {
        let (a, b) = (self.points[0], *self.points.last().unwrap());
        let d = unit(b.sub(a));
        let on_line = self.points.iter().all(|p| {
            let v = p.sub(a);
            (v[0] * d[1] - v[1] * d[0]).abs() < SAME
        });
        on_line
            && self.points.windows(2).all(|w| {
                let v = w[1].sub(w[0]);
                v[0] * d[0] + v[1] * d[1] > -SAME
            })
    }

    /// Direction a cut at `s` runs across: the end directions at the ends, the
    /// bisector of the two chords at a vertex, the chord inside one.
    fn cut_dir(&self, s: f64) -> [f64; 2] {
        let len = self.length();
        if s <= SAME {
            return self.ends[0];
        }
        if s >= len - SAME {
            return self.ends[1];
        }
        let i = self.chord(s);
        if (s - self.cum[i]).abs() < SAME && i > 0 {
            let (d0, d1) = (
                unit(self.points[i].sub(self.points[i - 1])),
                unit(self.points[i + 1].sub(self.points[i])),
            );
            return unit([d0[0] + d1[0], d0[1] + d1[1]]);
        }
        unit(self.points[i + 1].sub(self.points[i]))
    }

    /// Plan outline of a wall `thickness` thick centred on the axis: right
    /// offset forward, left offset back (counterclockwise), mitred at vertices.
    /// For two points this is the rectangle of a straight wall.
    pub fn outline(&self, thickness: f64) -> Vec<[f64; 2]> {
        let h = thickness / 2.0;
        let n = self.points.len();
        let left = |d: [f64; 2]| [-d[1], d[0]];
        // Left offset vector at each point.
        let offsets: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                if i == 0 || i + 1 == n {
                    let l = left(self.ends[if i == 0 { 0 } else { 1 }]);
                    return [l[0] * h, l[1] * h];
                }
                let d0 = unit(self.points[i].sub(self.points[i - 1]));
                let d1 = unit(self.points[i + 1].sub(self.points[i]));
                let m = left(unit([d0[0] + d1[0], d0[1] + d1[1]]));
                let n0 = left(d0);
                let k = h / (m[0] * n0[0] + m[1] * n0[1]).max(0.1);
                [m[0] * k, m[1] * k]
            })
            .collect();
        let mut ring: Vec<[f64; 2]> = self
            .points
            .iter()
            .zip(&offsets)
            .map(|(p, o)| [p[0] - o[0], p[1] - o[1]])
            .collect();
        ring.extend(
            self.points
                .iter()
                .zip(&offsets)
                .rev()
                .map(|(p, o)| [p[0] + o[0], p[1] + o[1]]),
        );
        ring
    }

    /// Closed exact outline of a curved wall `thickness` thick: smooth
    /// curves through the [`outline`](Self::outline) offsets of each face,
    /// sharp at the axis corners, joined by the straight end caps. Samples
    /// closer than [`FACE_SPACING`] to the last kept one, or than half of it
    /// to the next corner, are skipped.
    pub fn faces(&self, thickness: f64) -> Result<Curve2, CurveError> {
        let ring = self.outline(thickness);
        let n = self.points.len();
        // Right face forward, then the left face back.
        let corner = |i: usize| self.corner[if i < n { i } else { 2 * n - 1 - i }];
        // The ring ends on the start corner.
        let mut next_corner = ring.clone();
        for i in (0..ring.len() - 1).rev() {
            if !corner(i) {
                next_corner[i] = next_corner[i + 1];
            }
        }
        let mut items: Vec<PathItem<[f64; 2]>> = Vec::new();
        let mut last = ring[0];
        for (i, &p) in ring.iter().enumerate() {
            let d = p.sub(last);
            if corner(i) {
                items.push(PathItem::Corner(p));
            } else if norm(d) >= FACE_SPACING && dist(p, next_corner[i]) >= FACE_SPACING / 2.0 {
                items.push(PathItem::Smooth(p));
            } else {
                continue;
            }
            last = p;
        }
        build(&items, true)
    }

    /// The part of the axis from `s0` to `s1` (clamped, `s0 <= s1`). Its ends
    /// are cut along the same lines as the neighbouring parts, so they meet.
    pub fn sub(&self, s0: f64, s1: f64) -> WallAxis {
        let len = self.length();
        let (s0, s1) = (s0.clamp(0.0, len), s1.clamp(0.0, len));
        let (s0, s1) = (s0.min(s1), s0.max(s1));
        let mut points = vec![self.point_at(s0)];
        let mut corner = vec![true];
        for i in 0..self.points.len() {
            if self.cum[i] > s0 + SAME && self.cum[i] < s1 - SAME {
                points.push(self.points[i]);
                corner.push(self.corner[i]);
            }
        }
        points.push(self.point_at(s1));
        corner.push(true);
        let mut cum = vec![0.0];
        for w in points.windows(2) {
            cum.push(cum.last().unwrap() + dist(w[0], w[1]));
        }
        WallAxis {
            points,
            cum,
            corner,
            ends: [self.cut_dir(s0), self.cut_dir(s1)],
        }
    }

    /// Smallest bend radius along curve samples (circumradius of each
    /// non-corner vertex and its neighbours); infinite for straight chords.
    pub fn min_radius(&self) -> f64 {
        let p = &self.points;
        (1..p.len().saturating_sub(1))
            .filter(|&i| !self.corner[i])
            .map(|i| {
                let (a, b, c) = (
                    p[i - 1].sub(p[i]),
                    p[i + 1].sub(p[i]),
                    p[i + 1].sub(p[i - 1]),
                );
                let cross = (a[0] * b[1] - a[1] * b[0]).abs();
                if cross < 1e-15 {
                    f64::INFINITY
                } else {
                    norm(a) * norm(b) * norm(c) / (2.0 * cross)
                }
            })
            .fold(f64::INFINITY, f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
    }

    #[test]
    fn doubling_back_is_not_straight() {
        let back = WallAxis::polyline(vec![[0.0, 0.0], [5.0, 0.0], [2.0, 0.0]]).unwrap();
        assert!(!back.is_straight());
        let on = WallAxis::polyline(vec![[0.0, 0.0], [2.0, 0.0], [5.0, 0.0]]).unwrap();
        assert!(on.is_straight());
    }

    #[test]
    fn axis_point_and_tangent() {
        let a = WallAxis::polyline(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]]).unwrap();
        assert!((a.length() - 7.0).abs() < 1e-12);
        assert!(near(a.point_at(2.0), [2.0, 0.0]));
        assert!(near(a.point_at(4.0), [4.0, 0.0]));
        assert!(near(a.point_at(5.5), [4.0, 1.5]));
        assert!(near(a.tangent_at(2.0), [1.0, 0.0]));
        assert!(near(a.tangent_at(5.5), [0.0, 1.0]));
        assert!(!a.is_straight());
        assert_eq!(a.min_radius(), f64::INFINITY);
    }

    #[test]
    fn straight_outline_is_the_wall_rectangle() {
        let a = WallAxis::polyline(vec![[0.0, 0.0], [5.0, 0.0]]).unwrap();
        assert!(a.is_straight());
        let o = a.outline(0.2);
        let want = [[0.0, -0.1], [5.0, -0.1], [5.0, 0.1], [0.0, 0.1]];
        assert!(o.iter().zip(want).all(|(p, q)| near(*p, q)), "{o:?}");
    }

    #[test]
    fn l_outline_is_mitred() {
        let a = WallAxis::polyline(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]]).unwrap();
        let o = a.outline(0.2);
        assert!(near(o[1], [4.1, -0.1]), "{o:?}");
        assert!(near(o[4], [3.9, 0.1]), "{o:?}");
    }

    #[test]
    fn sub_parts_meet() {
        let a = WallAxis::polyline(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]]).unwrap();
        let (l, r) = (a.sub(1.0, 4.0), a.sub(4.0, 6.0));
        assert!((l.length() - 3.0).abs() < 1e-12 && (r.length() - 2.0).abs() < 1e-12);
        let (lo, ro) = (l.outline(0.2), r.outline(0.2));
        // The end of the left part and the start of the right part share the mitre line.
        assert!(near(lo[1], ro[0]) && near(lo[2], ro[3]), "{lo:?} {ro:?}");
    }

    #[test]
    fn arc_radius() {
        let c = Curve2 {
            segments: vec![Segment::Arc {
                from: [0.0, 0.0],
                via: [5.0, 5.0],
                to: [10.0, 0.0],
            }],
            closed: false,
        };
        let a = WallAxis::on_curve(c.tessellate(0.005), &c).unwrap();
        assert!((a.min_radius() - 5.0).abs() < 1e-6);
        assert!((a.length() - 5.0 * std::f64::consts::PI).abs() < 0.01);
        assert!(!a.is_straight());
        // Clockwise semicircle of radius 5 about (5, 0), over the top.
        let top = a.point_at(a.length() / 2.0);
        assert!(
            (top[0] - 5.0).abs() < 0.005 && (top[1] - 5.0).abs() < 0.005,
            "{top:?}"
        );
        assert!(near(a.point_at(a.length()), [10.0, 0.0]));
    }

    #[test]
    fn faces_skip_a_sample_next_to_a_corner() {
        let c = Curve2 {
            segments: vec![Segment::Arc {
                from: [0.0, 0.0],
                via: [5.0, 5.0],
                to: [10.0, 0.0],
            }],
            closed: false,
        };
        // Samples 0.6 m apart, the last one 1 mm before the end.
        let len = c.length();
        let mut s: Vec<f64> = (0..)
            .map(|i| i as f64 * 0.6)
            .take_while(|&s| s < len - 0.6)
            .collect();
        s.extend([len - 0.001, len]);
        let a = WallAxis::on_curve(s.iter().map(|&s| c.point_at(s)).collect(), &c).unwrap();
        let worst = a
            .faces(0.2)
            .unwrap()
            .tessellate(0.001)
            .iter()
            .map(|p| {
                let r = (p[0] - 5.0).hypot(p[1]);
                (r - 4.9).min(0.0).abs().max((r - 5.1).max(0.0))
            })
            .fold(0.0, f64::max);
        assert!(worst < 0.005, "{worst}");
    }
}
