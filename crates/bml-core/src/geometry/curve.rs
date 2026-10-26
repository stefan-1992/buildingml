//! Exact plan curves (`Curve2` / `Curve3`): straight lines, circle arcs and NURBS
//! segments, with through-points spline fitting, evaluation, arc length and
//! tessellation to a chord error.
//!
//! A path is a list of [`PathItem`]s. Plain points are corners; a run of
//! `Smooth` points becomes one cubic spline through those points, ending at the
//! corners either side (wrapping over the seam of a closed path). A closed path
//! of only smooth points is one closed spline, C1 across the seam.

use std::fmt;

/// Point arithmetic the curve code needs, for `[f64; 2]` and `[f64; 3]`.
pub trait Point: Copy + PartialEq + fmt::Debug {
    fn add(self, o: Self) -> Self;
    fn sub(self, o: Self) -> Self;
    fn scale(self, k: f64) -> Self;
    fn dot(self, o: Self) -> f64;
}

impl<const N: usize> Point for [f64; N] {
    fn add(self, o: Self) -> Self {
        std::array::from_fn(|i| self[i] + o[i])
    }
    fn sub(self, o: Self) -> Self {
        std::array::from_fn(|i| self[i] - o[i])
    }
    fn scale(self, k: f64) -> Self {
        self.map(|v| v * k)
    }
    fn dot(self, o: Self) -> f64 {
        (0..N).map(|i| self[i] * o[i]).sum()
    }
}

pub(crate) fn norm<P: Point>(p: P) -> f64 {
    p.dot(p).sqrt()
}

pub(crate) fn dist<P: Point>(a: P, b: P) -> f64 {
    norm(b.sub(a))
}

pub(crate) fn lerp<P: Point>(a: P, b: P, t: f64) -> P {
    a.add(b.sub(a).scale(t))
}

/// Two points closer than this are the same point.
const SAME: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq)]
pub enum CurveError {
    TooFewPoints,
    /// Item index of a point that repeats its predecessor inside a smooth run.
    RepeatedPoint(usize),
    BadDegree,
    BadKnots {
        expected: usize,
        got: usize,
    },
    /// Index of a weight that is not positive (or missing).
    BadWeight(usize),
    /// Index of a knot smaller than the one before it.
    DecreasingKnots(usize),
    /// First and last knot must each repeat `degree + 1` times.
    UnclampedKnots,
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewPoints => write!(f, "curve has too few points"),
            Self::RepeatedPoint(i) => {
                write!(f, "point {i} repeats the previous point of a smooth run")
            }
            Self::BadDegree => write!(f, "nurbs degree must be at least 1"),
            Self::BadKnots { expected, got } => {
                write!(
                    f,
                    "nurbs needs {expected} knots (points + degree + 1), got {got}"
                )
            }
            Self::BadWeight(i) => write!(f, "nurbs weight {i} must be a positive number"),
            Self::DecreasingKnots(i) => {
                write!(f, "nurbs knot {i} is smaller than the knot before it")
            }
            Self::UnclampedKnots => write!(
                f,
                "nurbs knots must be clamped: first and last knot repeated degree + 1 times"
            ),
        }
    }
}

impl std::error::Error for CurveError {}

/// One item of a point list.
#[derive(Debug, Clone, PartialEq)]
pub enum PathItem<P> {
    Corner(P),
    Smooth(P),
    /// Exact segment from its first to its last control point.
    Nurbs(Nurbs<P>),
}

/// Clamped NURBS curve; it starts at `points[0]` and ends at the last point.
#[derive(Debug, Clone, PartialEq)]
pub struct Nurbs<P> {
    pub degree: usize,
    pub points: Vec<P>,
    pub weights: Vec<f64>,
    pub knots: Vec<f64>,
}

impl<P: Point> Nurbs<P> {
    /// Weights default to all 1, knots to a clamped uniform vector.
    pub fn new(
        degree: usize,
        points: Vec<P>,
        weights: Option<Vec<f64>>,
        knots: Option<Vec<f64>>,
    ) -> Result<Self, CurveError> {
        let n = points.len();
        if degree < 1 {
            return Err(CurveError::BadDegree);
        }
        if n <= degree {
            return Err(CurveError::TooFewPoints);
        }
        let weights = weights.unwrap_or_else(|| vec![1.0; n]);
        if let Some(i) =
            (0..n).find(|&i| weights.get(i).is_none_or(|w| !(*w > 0.0 && w.is_finite())))
        {
            return Err(CurveError::BadWeight(i));
        }
        if weights.len() > n {
            return Err(CurveError::BadWeight(n));
        }
        let knots = knots.unwrap_or_else(|| clamped_uniform(degree, n));
        let expected = n + degree + 1;
        if knots.len() != expected {
            return Err(CurveError::BadKnots {
                expected,
                got: knots.len(),
            });
        }
        if let Some(i) = (1..expected).find(|&i| knots[i] < knots[i - 1]) {
            return Err(CurveError::DecreasingKnots(i));
        }
        let (first, last) = (knots[0], knots[expected - 1]);
        let clamped = knots[..=degree].iter().all(|&k| k == first)
            && knots[n..].iter().all(|&k| k == last)
            && knots[degree] < knots[degree + 1]
            && knots[n - 1] < knots[n];
        if !clamped {
            return Err(CurveError::UnclampedKnots);
        }
        Ok(Self {
            degree,
            points,
            weights,
            knots,
        })
    }

    /// First and last parameter of the curve.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.points.len()])
    }

    fn homogeneous(&self) -> Vec<(P, f64)> {
        self.points
            .iter()
            .zip(&self.weights)
            .map(|(&p, &w)| (p.scale(w), w))
            .collect()
    }

    /// Knot span of `u` and the `degree + 1` homogeneous control points it uses.
    fn window(&self, u: f64) -> (usize, Vec<(P, f64)>) {
        let p = self.degree;
        let k = span(p, self.points.len(), &self.knots, u);
        let h = (k - p..=k)
            .map(|i| (self.points[i].scale(self.weights[i]), self.weights[i]))
            .collect();
        (k, h)
    }

    /// Point at parameter `u`.
    pub(crate) fn eval(&self, u: f64) -> P {
        let (k, h) = self.window(u);
        let (a, w) = de_boor(self.degree, k, h, &self.knots, u);
        a.scale(1.0 / w)
    }

    /// First derivative at parameter `u`.
    fn deriv(&self, u: f64) -> P {
        let p = self.degree;
        let (k, h) = self.window(u);
        // The derivative's control points over the same window, in span k - 1
        // of the knots without their ends.
        let dh: Vec<(P, f64)> = (0..p)
            .map(|j| {
                let i = k - p + j;
                let span = self.knots[i + p + 1] - self.knots[i + 1];
                let s = if span > 0.0 { p as f64 / span } else { 0.0 };
                (h[j + 1].0.sub(h[j].0).scale(s), (h[j + 1].1 - h[j].1) * s)
            })
            .collect();
        let (a, w) = de_boor(p, k, h, &self.knots, u);
        let (da, dw) = de_boor(p - 1, k - 1, dh, &self.knots[1..self.knots.len() - 1], u);
        da.sub(a.scale(dw / w)).scale(1.0 / w)
    }

    /// Knots mapped to 0..1.
    pub(crate) fn normalized_knots(&self) -> Vec<f64> {
        let (k0, k1) = (self.knots[0], *self.knots.last().unwrap());
        self.knots.iter().map(|k| (k - k0) / (k1 - k0)).collect()
    }

    /// Distinct knot values inside the domain, ends included.
    fn breaks(&self) -> Vec<f64> {
        let (a, b) = self.domain();
        let mut out: Vec<f64> = vec![a];
        for &k in &self.knots {
            if k > *out.last().unwrap() && k <= b {
                out.push(k);
            }
        }
        out
    }

    /// Insert knot `u` once (Boehm); the curve does not change.
    fn insert_knot(&mut self, u: f64) {
        let p = self.degree;
        let n = self.points.len();
        let k = span(p, n, &self.knots, u);
        let h = self.homogeneous();
        let mut q = Vec::with_capacity(n + 1);
        q.extend_from_slice(&h[..=k - p]);
        for i in k - p + 1..=k {
            let a = (u - self.knots[i]) / (self.knots[i + p] - self.knots[i]);
            q.push((
                lerp(h[i - 1].0, h[i].0, a),
                h[i - 1].1 + a * (h[i].1 - h[i - 1].1),
            ));
        }
        q.extend_from_slice(&h[k..]);
        self.knots.insert(k + 1, u);
        self.points = q.iter().map(|(a, w)| a.scale(1.0 / w)).collect();
        self.weights = q.iter().map(|q| q.1).collect();
    }

    /// Points of a spline whose knots need not be clamped (built without
    /// `new`), 16 per knot span over `knots[degree]..knots[points.len()]`.
    pub fn samples(&self) -> Vec<P> {
        let n = self.points.len();
        let (u0, u1) = (self.knots[self.degree], self.knots[n]);
        let k = 16 * (n - self.degree);
        (0..=k)
            .map(|i| self.eval(u0 + (u1 - u0) * i as f64 / k as f64))
            .collect()
    }

    /// The part between parameters `u0 < u1`, clamped to the domain; the
    /// whole curve when the clamped range is empty.
    pub fn trim(&self, u0: f64, u1: f64) -> Self {
        let (a, b) = self.domain();
        let (u0, u1) = (u0.clamp(a, b), u1.clamp(a, b));
        let mut n = self.clone();
        if u0 >= u1 {
            return n;
        }
        if u1 < b {
            n = n.split(u1).0;
        }
        if u0 > a {
            n = n.split(u0).1;
        }
        n
    }

    /// The two halves either side of parameter `u` (inside the domain).
    fn split(&self, u: f64) -> (Self, Self) {
        let p = self.degree;
        let mut c = self.clone();
        let have = c.knots.iter().filter(|&&k| k == u).count();
        for _ in have..p {
            c.insert_knot(u);
        }
        // Knots below `u`: as many control points end the first half.
        let below = c.knots.iter().filter(|&&k| k < u).count();
        let half = |points: &[P], weights: &[f64], knots: Vec<f64>| Self {
            degree: p,
            points: points.to_vec(),
            weights: weights.to_vec(),
            knots,
        };
        let mut left_knots = c.knots[..below + p].to_vec();
        left_knots.push(u);
        let mut right_knots = vec![u];
        right_knots.extend_from_slice(&c.knots[below..]);
        (
            half(&c.points[..below], &c.weights[..below], left_knots),
            half(&c.points[below - 1..], &c.weights[below - 1..], right_knots),
        )
    }
}

/// Knot vector from distinct `values` each repeated `mults` times.
pub fn expand_knots(values: &[f64], mults: &[usize]) -> Vec<f64> {
    values
        .iter()
        .zip(mults)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m))
        .collect()
}

pub(crate) fn clamped_uniform(degree: usize, n: usize) -> Vec<f64> {
    let inner = n - degree;
    let mut k = vec![0.0; degree + 1];
    k.extend((1..inner).map(|i| i as f64 / inner as f64));
    k.extend(std::iter::repeat_n(1.0, degree + 1));
    k
}

/// Knot span `k` with `knots[k] <= u < knots[k + 1]`, the last span at the domain end.
fn span(p: usize, n: usize, knots: &[f64], u: f64) -> usize {
    (p..n).rev().find(|&i| knots[i] <= u).unwrap_or(p)
}

/// de Boor evaluation of a B-spline in knot span `k`, over the `p + 1`
/// homogeneous control points `d` of that span.
fn de_boor<P: Point>(p: usize, k: usize, mut d: Vec<(P, f64)>, knots: &[f64], u: f64) -> (P, f64) {
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let a = (u - knots[i]) / (knots[i + p + 1 - r] - knots[i]);
            d[j] = (
                lerp(d[j - 1].0, d[j].0, a),
                d[j - 1].1 + a * (d[j].1 - d[j - 1].1),
            );
        }
    }
    d[p]
}

/// Values of the `p + 1` basis functions nonzero at `u` in span `k`.
fn basis(p: usize, k: usize, knots: &[f64], u: f64) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[k + 1 - j];
        right[j] = knots[k + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let t = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * t;
            saved = left[j - r] * t;
        }
        n[j] = saved;
    }
    n
}

/// Solve the dense system `m x = rhs` (Gaussian elimination with partial pivoting).
fn solve<P: Point>(mut m: Vec<Vec<f64>>, mut rhs: Vec<P>) -> Vec<P> {
    let n = rhs.len();
    for c in 0..n {
        let piv = (c..n)
            .max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))
            .unwrap();
        m.swap(c, piv);
        rhs.swap(c, piv);
        for r in c + 1..n {
            let f = m[r][c] / m[c][c];
            if f != 0.0 {
                let pivot_row = m[c].clone();
                for (v, p) in m[r][c..].iter_mut().zip(&pivot_row[c..]) {
                    *v -= f * p;
                }
                rhs[r] = rhs[r].sub(rhs[c].scale(f));
            }
        }
    }
    let mut x = rhs.clone();
    for r in (0..n).rev() {
        let mut v = rhs[r];
        for k in r + 1..n {
            v = v.sub(x[k].scale(m[r][k]));
        }
        x[r] = v.scale(1.0 / m[r][r]);
    }
    x
}

/// Chord-length parameters of `q` in [0, 1].
fn chord_params<P: Point>(q: &[P]) -> Vec<f64> {
    let mut u = vec![0.0];
    for w in q.windows(2) {
        u.push(u.last().unwrap() + dist(w[0], w[1]));
    }
    let total = *u.last().unwrap();
    u.iter().map(|v| v / total).collect()
}

/// Matrix row: basis functions of a curve with `cols` control points at `u`.
fn basis_row(p: usize, cols: usize, knots: &[f64], u: f64) -> Vec<f64> {
    let k = span(p, cols, knots, u);
    let mut row = vec![0.0; cols];
    row[k - p..=k].copy_from_slice(&basis(p, k, knots, u));
    row
}

/// Open spline through `q` (at least 3 points): global interpolation with
/// chord-length parameters and averaged knots, degree 3 (2 for 3 points).
/// The through-points are then inserted as knots so tessellation hits them.
fn interpolate<P: Point>(q: &[P]) -> Nurbs<P> {
    let n = q.len() - 1;
    let p = n.min(3);
    let u = chord_params(q);
    let mut knots = vec![0.0; p + 1];
    knots.extend((1..=n - p).map(|j| u[j..j + p].iter().sum::<f64>() / p as f64));
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    let m = u
        .iter()
        .map(|&uk| basis_row(p, n + 1, &knots, uk))
        .collect();
    let mut points = solve(m, q.to_vec());
    points[0] = q[0];
    points[n] = q[n];
    let mut c = Nurbs {
        degree: p,
        points,
        weights: vec![1.0; n + 1],
        knots,
    };
    for &uk in &u[1..n] {
        if !c.knots.iter().any(|k| (k - uk).abs() < 1e-12) {
            c.insert_knot(uk);
        }
    }
    c
}

/// Closed cubic spline through `q` and back to `q[0]` (at least 3 points):
/// clamped, seam point repeated, equal end derivatives (C1 at the seam).
/// Knots sit at the through-points.
fn interpolate_closed<P: Point>(q: &[P]) -> Nurbs<P> {
    let mut q = q.to_vec();
    q.push(q[0]);
    let n = q.len() - 1;
    let p = 3;
    let u = chord_params(&q);
    let d = q[1].sub(q[n - 1]).scale(1.0 / (u[1] + 1.0 - u[n - 1]));
    let mut knots = vec![0.0; p + 1];
    knots.extend_from_slice(&u[1..n]);
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    let cols = n + 3;
    let mut m: Vec<Vec<f64>> = u.iter().map(|&uk| basis_row(p, cols, &knots, uk)).collect();
    let mut rhs = q.clone();
    // C'(0) = 3 / u_4 (P1 - P0), C'(1) = 3 / (1 - u_{cols-1}) (P_last - P_last-1).
    let mut row = vec![0.0; cols];
    let k0 = p as f64 / knots[p + 1];
    (row[0], row[1]) = (-k0, k0);
    m.push(row);
    let mut row = vec![0.0; cols];
    let k1 = p as f64 / (1.0 - knots[cols - 1]);
    (row[cols - 2], row[cols - 1]) = (-k1, k1);
    m.push(row);
    rhs.extend([d, d]);
    let mut points = solve(m, rhs);
    points[0] = q[0];
    points[cols - 1] = q[0];
    Nurbs {
        degree: p,
        points,
        weights: vec![1.0; cols],
        knots,
    }
}

/// Circle through three points, in their plane: `center + radius (cos a e1 + sin a e2)`
/// for `a` in `0..=sweep` runs from the first through the second to the third point.
pub(crate) struct Circle<P> {
    pub center: P,
    pub radius: f64,
    pub e1: P,
    pub e2: P,
    /// Positive sweep angle in radians.
    pub sweep: f64,
}

impl<P: Point> Circle<P> {
    /// `None` when the three points are collinear.
    pub(crate) fn through(from: P, via: P, to: P) -> Option<Self> {
        let (u, v) = (via.sub(from), to.sub(from));
        let (uu, uv, vv) = (u.dot(u), u.dot(v), v.dot(v));
        // det = |u x v|^2; same threshold as the old 2D test |2 u x v| < 1e-12.
        let det = uu * vv - uv * uv;
        if 2.0 * det.max(0.0).sqrt() < 1e-12 {
            return None;
        }
        let a = (uu * vv - uv * vv) / (2.0 * det);
        let b = (uu * vv - uv * uu) / (2.0 * det);
        let center = from.add(u.scale(a)).add(v.scale(b));
        let r0 = from.sub(center);
        let radius = norm(r0);
        let e1 = r0.scale(1.0 / radius);
        let perp = |p: P| {
            let w = p.sub(center);
            w.sub(e1.scale(w.dot(e1)))
        };
        let (pv, pt) = (perp(via), perp(to));
        let pick = if norm(pv) >= norm(pt) { pv } else { pt };
        let mut e2 = pick.scale(1.0 / norm(pick));
        let angle = |p: P, e2: P| {
            let w = p.sub(center);
            w.dot(e2).atan2(w.dot(e1)).rem_euclid(std::f64::consts::TAU)
        };
        if angle(via, e2) > angle(to, e2) {
            e2 = e2.scale(-1.0);
        }
        let sweep = angle(to, e2);
        Some(Self {
            center,
            radius,
            e1,
            e2,
            sweep,
        })
    }

    /// Point at angle `a` from the first point.
    pub(crate) fn at(&self, r: f64, a: f64) -> P {
        self.center
            .add(self.e1.scale(r * a.cos()))
            .add(self.e2.scale(r * a.sin()))
    }

    fn length(&self) -> f64 {
        self.radius * self.sweep
    }

    /// The arc as an exact rational quadratic B-spline, one piece per at most
    /// a quarter turn (double interior knots).
    pub(crate) fn to_nurbs(&self) -> Nurbs<P> {
        let n = ((self.sweep / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
        let step = self.sweep / n as f64;
        let w = (step / 2.0).cos();
        let mut points = vec![self.at(self.radius, 0.0)];
        let mut weights = vec![1.0];
        let mut knots = vec![0.0; 3];
        for i in 0..n {
            let a = step * i as f64;
            points.push(self.at(self.radius / w, a + step / 2.0));
            points.push(self.at(self.radius, a + step));
            weights.extend([w, 1.0]);
            let k = (i + 1) as f64 / n as f64;
            knots.extend(if i + 1 == n { vec![k; 3] } else { vec![k; 2] });
        }
        Nurbs {
            degree: 2,
            points,
            weights,
            knots,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment<P> {
    Line(P, P),
    /// Circle arc from `from` through `via` to `to` (a line when collinear).
    Arc {
        from: P,
        via: P,
        to: P,
    },
    Nurbs(Nurbs<P>),
}

/// 5-point Gauss-Legendre nodes and weights on [-1, 1].
const GAUSS: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Pieces each knot span is cut into for arc-length integration.
const PIECES: usize = 16;

impl<P: Point> Nurbs<P> {
    /// Arc length between parameters `a` and `b` (one Gauss rule).
    fn length_between(&self, a: f64, b: f64) -> f64 {
        let (m, h) = ((a + b) / 2.0, (b - a) / 2.0);
        GAUSS
            .iter()
            .map(|&(x, w)| w * norm(self.deriv(m + h * x)))
            .sum::<f64>()
            * h
    }

    /// Integration pieces over the domain.
    fn pieces(&self) -> Vec<(f64, f64)> {
        self.breaks()
            .windows(2)
            .flat_map(|w| {
                let (a, b) = (w[0], w[1]);
                (0..PIECES).map(move |i| {
                    let t = |i: usize| a + (b - a) * i as f64 / PIECES as f64;
                    (t(i), t(i + 1))
                })
            })
            .collect()
    }

    /// Parameter `s` along the curve from its start.
    fn param_at(&self, s: f64) -> f64 {
        let mut acc = 0.0;
        let pieces = self.pieces();
        for &(a, b) in &pieces {
            let l = self.length_between(a, b);
            if acc + l >= s {
                let rest = s - acc;
                if l <= 0.0 {
                    return a;
                }
                let mut u = a + (b - a) * (rest / l);
                for _ in 0..20 {
                    let f = self.length_between(a, u) - rest;
                    if f.abs() < 1e-12 {
                        break;
                    }
                    u = (u - f / norm(self.deriv(u))).clamp(a, b);
                }
                return u;
            }
            acc += l;
        }
        self.domain().1
    }
}

impl<P: Point> Segment<P> {
    pub fn start(&self) -> P {
        match self {
            Self::Line(a, _) => *a,
            Self::Arc { from, .. } => *from,
            Self::Nurbs(n) => n.points[0],
        }
    }

    pub fn end(&self) -> P {
        match self {
            Self::Line(_, b) => *b,
            Self::Arc { to, .. } => *to,
            Self::Nurbs(n) => *n.points.last().unwrap(),
        }
    }

    pub(crate) fn circle(&self) -> Option<Circle<P>> {
        match self {
            Self::Arc { from, via, to } => Circle::through(*from, *via, *to),
            _ => None,
        }
    }

    /// The same segment run from end to start.
    pub(crate) fn reversed(self) -> Self {
        match self {
            Self::Line(a, b) => Self::Line(b, a),
            Self::Arc { from, via, to } => Self::Arc {
                from: to,
                via,
                to: from,
            },
            Self::Nurbs(mut n) => {
                let (lo, hi) = (n.knots[0], *n.knots.last().unwrap());
                n.points.reverse();
                n.weights.reverse();
                n.knots = n.knots.iter().rev().map(|k| lo + hi - k).collect();
                Self::Nurbs(n)
            }
        }
    }

    /// The segment as NURBS: an arc as an exact rational quadratic with its
    /// ends snapped to the arc's ends. `None` for lines and collinear arcs.
    pub(crate) fn to_nurbs(&self) -> Option<Nurbs<P>> {
        match self {
            Self::Nurbs(n) => Some(n.clone()),
            _ => self.circle().map(|circle| {
                let mut n = circle.to_nurbs();
                n.points[0] = self.start();
                *n.points.last_mut().unwrap() = self.end();
                n
            }),
        }
    }

    pub fn length(&self) -> f64 {
        match self {
            Self::Nurbs(n) => n
                .pieces()
                .iter()
                .map(|&(a, b)| n.length_between(a, b))
                .sum(),
            _ => self
                .circle()
                .map_or_else(|| dist(self.start(), self.end()), |c| c.length()),
        }
    }

    /// Point and (not normalised) tangent `s` along the segment.
    fn point_and_tangent(&self, s: f64) -> (P, P) {
        match self {
            Self::Nurbs(n) => {
                let u = n.param_at(s);
                (n.eval(u), n.deriv(u))
            }
            _ => match self.circle() {
                Some(c) => {
                    let a = s / c.radius;
                    (
                        c.at(c.radius, a),
                        c.e2.scale(a.cos()).sub(c.e1.scale(a.sin())),
                    )
                }
                None => {
                    let (a, b) = (self.start(), self.end());
                    (lerp(a, b, s / dist(a, b)), b.sub(a))
                }
            },
        }
    }

    /// Points strictly between start and end, chord error at most `tol`.
    fn interior(&self, tol: f64, out: &mut Vec<P>) {
        match self {
            Self::Line(..) => {}
            Self::Arc { .. } => {
                let Some(c) = self.circle() else { return };
                let step = 2.0 * (1.0 - tol / c.radius).clamp(-1.0, 1.0).acos();
                let n = (c.sweep / step).ceil().max(1.0) as usize;
                out.extend((1..n).map(|i| c.at(c.radius, c.sweep * i as f64 / n as f64)));
            }
            Self::Nurbs(n) => {
                let breaks = n.breaks();
                for (i, w) in breaks.windows(2).enumerate() {
                    let (a, b) = (w[0], w[1]);
                    let pa = if i == 0 { n.points[0] } else { n.eval(a) };
                    if i > 0 {
                        out.push(pa);
                    }
                    subdivide(n, a, pa, b, n.eval(b), tol, 0, out);
                }
            }
        }
    }
}

/// Distance from `p` to the line through `a` and `b`, or to the segment
/// `a`-`b` when `clamp`.
fn dist_to_line<P: Point>(p: P, a: P, b: P, clamp: bool) -> f64 {
    let ab = b.sub(a);
    let len2 = ab.dot(ab);
    let mut t = if len2 > 0.0 {
        p.sub(a).dot(ab) / len2
    } else {
        0.0
    };
    if clamp {
        t = t.clamp(0.0, 1.0);
    }
    dist(p, lerp(a, b, t))
}

/// Recursively split `[a, b]` until the curve is within `tol` of the chord,
/// tested at the quarter points; pushes interior points in order.
#[allow(clippy::too_many_arguments)]
fn subdivide<P: Point>(
    n: &Nurbs<P>,
    a: f64,
    pa: P,
    b: f64,
    pb: P,
    tol: f64,
    depth: u32,
    out: &mut Vec<P>,
) {
    let m = (a + b) / 2.0;
    let pm = n.eval(m);
    let err = [0.25, 0.5, 0.75]
        .iter()
        .map(|&t| {
            dist_to_line(
                if t == 0.5 {
                    pm
                } else {
                    n.eval(a + (b - a) * t)
                },
                pa,
                pb,
                true,
            )
        })
        .fold(0.0, f64::max);
    if err <= tol || depth >= 24 {
        return;
    }
    subdivide(n, a, pa, m, pm, tol, depth + 1, out);
    out.push(pm);
    subdivide(n, m, pm, b, pb, tol, depth + 1, out);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Curve<P> {
    pub segments: Vec<Segment<P>>,
    /// The last segment ends where the first starts.
    pub closed: bool,
}

pub type Curve2 = Curve<[f64; 2]>;
pub type Curve3 = Curve<[f64; 3]>;

impl<P: Point> Curve<P> {
    pub fn length(&self) -> f64 {
        self.segments.iter().map(Segment::length).sum()
    }

    /// Segment and distance into it for distance `s` along the curve (clamped).
    fn locate(&self, s: f64) -> (&Segment<P>, f64) {
        let mut rest = s.max(0.0);
        for (i, seg) in self.segments.iter().enumerate() {
            let l = seg.length();
            if rest <= l || i + 1 == self.segments.len() {
                return (seg, rest.min(l));
            }
            rest -= l;
        }
        unreachable!("curve without segments")
    }

    /// Point `s` along the curve from its start.
    pub fn point_at(&self, s: f64) -> P {
        let (seg, s) = self.locate(s);
        seg.point_and_tangent(s).0
    }

    /// Unit tangent `s` along the curve.
    pub fn tangent_at(&self, s: f64) -> P {
        let (seg, s) = self.locate(s);
        let t = seg.point_and_tangent(s).1;
        t.scale(1.0 / norm(t))
    }

    /// Polyline with chord error at most `tol`. Segment ends (corners) are
    /// exact; a closed curve does not repeat its first point.
    pub fn tessellate(&self, tol: f64) -> Vec<P> {
        let mut out = Vec::new();
        for seg in &self.segments {
            out.push(seg.start());
            seg.interior(tol, &mut out);
        }
        if let Some(last) = self.segments.last().filter(|_| !self.closed) {
            out.push(last.end());
        }
        out
    }

    /// The part of an open curve from distance `s0` to `s1` along it.
    pub fn between(&self, s0: f64, s1: f64) -> Self {
        let mut segments = Vec::new();
        let mut at = 0.0;
        for seg in &self.segments {
            let l = seg.length();
            let (a, b) = ((s0 - at).max(0.0), (s1 - at).min(l));
            at += l;
            if b - a <= SAME {
                continue;
            }
            let point = |s: f64| seg.point_and_tangent(s).0;
            segments.push(match seg {
                Segment::Line(..) => Segment::Line(point(a), point(b)),
                Segment::Arc { .. } => Segment::Arc {
                    from: point(a),
                    via: point((a + b) / 2.0),
                    to: point(b),
                },
                Segment::Nurbs(n) => {
                    let (lo, hi) = n.domain();
                    let u0 = if a > SAME { n.param_at(a) } else { lo };
                    let u1 = if b < l - SAME { n.param_at(b) } else { hi };
                    Segment::Nurbs(n.trim(u0, u1))
                }
            });
        }
        Self {
            segments,
            closed: false,
        }
    }

    /// The curve moved by `d`.
    pub fn translate(&self, d: P) -> Self {
        self.map(|p| p.add(d))
    }

    /// The curve with every defining point passed through `f`. Exact for
    /// affine maps that keep circles circles (moves, turns, lifts to 3D).
    pub fn map<Q: Point>(&self, f: impl Fn(P) -> Q) -> Curve<Q> {
        let segments = self
            .segments
            .iter()
            .map(|seg| match seg {
                Segment::Line(a, b) => Segment::Line(f(*a), f(*b)),
                Segment::Arc { from, via, to } => Segment::Arc {
                    from: f(*from),
                    via: f(*via),
                    to: f(*to),
                },
                Segment::Nurbs(n) => Segment::Nurbs(Nurbs {
                    degree: n.degree,
                    points: n.points.iter().map(|p| f(*p)).collect(),
                    weights: n.weights.clone(),
                    knots: n.knots.clone(),
                }),
            })
            .collect();
        Curve {
            segments,
            closed: self.closed,
        }
    }

    /// Open and every point on the straight line from start to end.
    pub fn is_straight(&self) -> bool {
        let (Some(first), Some(last)) = (self.segments.first(), self.segments.last()) else {
            return false;
        };
        let (a, b) = (first.start(), last.end());
        !self.closed
            && self.segments.iter().all(|s| {
                matches!(s, Segment::Line(..))
                    && dist_to_line(s.start(), a, b, false) < SAME
                    && dist_to_line(s.end(), a, b, false) < SAME
            })
    }
}

struct Vertex<P> {
    p: P,
    smooth: bool,
    item: usize,
}

/// Build a curve from a point list. Corners join with lines, each smooth run
/// becomes one interpolating spline between the corners either side. A closed
/// curve with corners starts at its first corner.
pub fn build<P: Point>(items: &[PathItem<P>], closed: bool) -> Result<Curve<P>, CurveError> {
    let mut verts: Vec<Vertex<P>> = Vec::new();
    // nurbs[i]: exact segment from verts[i] to verts[i + 1].
    let mut nurbs: Vec<Option<Nurbs<P>>> = Vec::new();
    let push = |verts: &mut Vec<Vertex<P>>,
                nurbs: &mut Vec<Option<Nurbs<P>>>,
                v: Vertex<P>,
                force: bool| {
        if let Some(last) = verts.last().filter(|l| !force && dist(l.p, v.p) < SAME) {
            if last.smooth || v.smooth {
                return Err(CurveError::RepeatedPoint(v.item));
            }
            return Ok(());
        }
        verts.push(v);
        nurbs.push(None);
        Ok(())
    };
    for (item, it) in items.iter().enumerate() {
        match it {
            PathItem::Corner(p) => push(
                &mut verts,
                &mut nurbs,
                Vertex {
                    p: *p,
                    smooth: false,
                    item,
                },
                false,
            )?,
            PathItem::Smooth(p) => push(
                &mut verts,
                &mut nurbs,
                Vertex {
                    p: *p,
                    smooth: true,
                    item,
                },
                false,
            )?,
            PathItem::Nurbs(n) => {
                push(
                    &mut verts,
                    &mut nurbs,
                    Vertex {
                        p: n.points[0],
                        smooth: false,
                        item,
                    },
                    false,
                )?;
                *nurbs.last_mut().unwrap() = Some(n.clone());
                let end = *n.points.last().unwrap();
                push(
                    &mut verts,
                    &mut nurbs,
                    Vertex {
                        p: end,
                        smooth: false,
                        item,
                    },
                    true,
                )?;
            }
        }
    }
    if !closed {
        if verts.len() < 2 {
            return Err(CurveError::TooFewPoints);
        }
        return Ok(Curve {
            segments: walk(&verts, nurbs),
            closed,
        });
    }
    if verts.len() > 1 && dist(verts[0].p, verts[verts.len() - 1].p) < SAME {
        let last = verts.pop().unwrap();
        nurbs.pop();
        if last.smooth || verts[0].smooth {
            return Err(CurveError::RepeatedPoint(last.item));
        }
    }
    if verts.len() < 3 && nurbs.iter().all(Option::is_none) {
        return Err(CurveError::TooFewPoints);
    }
    let Some(first_corner) = verts.iter().position(|v| !v.smooth) else {
        let q: Vec<P> = verts.iter().map(|v| v.p).collect();
        return Ok(Curve {
            segments: vec![Segment::Nurbs(interpolate_closed(&q))],
            closed,
        });
    };
    verts.rotate_left(first_corner);
    nurbs.rotate_left(first_corner);
    verts.push(Vertex {
        p: verts[0].p,
        smooth: false,
        item: verts[0].item,
    });
    nurbs.push(None);
    Ok(Curve {
        segments: walk(&verts, nurbs),
        closed,
    })
}

/// Segments of an open vertex chain.
fn walk<P: Point>(verts: &[Vertex<P>], mut nurbs: Vec<Option<Nurbs<P>>>) -> Vec<Segment<P>> {
    let k = verts.len();
    let mut segments = Vec::new();
    let mut i = 0;
    while i + 1 < k {
        if let Some(n) = nurbs[i].take() {
            segments.push(Segment::Nurbs(n));
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j + 1 < k && verts[j].smooth {
            j += 1;
        }
        segments.push(if j == i + 1 {
            Segment::Line(verts[i].p, verts[j].p)
        } else {
            let q: Vec<P> = verts[i..=j].iter().map(|v| v.p).collect();
            Segment::Nurbs(interpolate(&q))
        });
        i = j;
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use PathItem::{Corner, Smooth};

    fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    }

    /// Distance from `p` to the polyline `poly` (closed when `closed`).
    fn dist_to_polyline(p: [f64; 2], poly: &[[f64; 2]], closed: bool) -> f64 {
        let n = poly.len();
        let edges = if closed { n } else { n - 1 };
        (0..edges)
            .map(|i| {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let ab = [b[0] - a[0], b[1] - a[1]];
                let len2 = ab[0] * ab[0] + ab[1] * ab[1];
                let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0);
                dist2(p, [a[0] + t * ab[0], a[1] + t * ab[1]])
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn angle_between(a: [f64; 2], b: [f64; 2]) -> f64 {
        (a[0] * b[0] + a[1] * b[1])
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    #[test]
    fn trim_keeps_the_parameter_range() {
        let n = Nurbs::new(
            3,
            vec![[0.0, 0.0], [2.0, 3.0], [5.0, -1.0], [7.0, 2.0], [9.0, 0.0]],
            None,
            None,
        )
        .unwrap();
        let t = n.trim(0.2, 0.7);
        assert_eq!(t.domain(), (0.2, 0.7));
        assert!(dist2(t.eval(0.2), n.eval(0.2)) < 1e-9);
        assert!(dist2(t.eval(0.45), n.eval(0.45)) < 1e-9);
        assert!(dist2(*t.points.last().unwrap(), n.eval(0.7)) < 1e-9);
        // Out-of-domain trims clamp; an empty range keeps the whole curve.
        assert_eq!(n.trim(-1.0, 2.0), n);
        assert_eq!(n.trim(0.5, 0.5), n);
    }

    fn wall_spline() -> Curve2 {
        build(
            &[
                Corner([0.0, 0.0]),
                Smooth([6.0, 2.0]),
                Smooth([12.0, 1.0]),
                Corner([18.0, 5.0]),
            ],
            false,
        )
        .unwrap()
    }

    #[test]
    fn spline_passes_through_points() {
        let poly = wall_spline().tessellate(0.005);
        for p in [[0.0, 0.0], [6.0, 2.0], [12.0, 1.0], [18.0, 5.0]] {
            let best = poly
                .iter()
                .map(|&q| dist2(p, q))
                .fold(f64::INFINITY, f64::min);
            assert!(best < 1e-9, "{p:?} is {best} from the nearest vertex");
        }
    }

    #[test]
    fn corners_stay_sharp() {
        let c = build(
            &[
                Corner([0.0, 0.0]),
                Smooth([10.0, -2.0]),
                Corner([20.0, 0.0]),
                Corner([20.0, 15.0]),
                Smooth([10.0, 18.0]),
                Corner([0.0, 15.0]),
            ],
            true,
        )
        .unwrap();
        assert!(c.closed);
        let corner = c.segments[0].length();
        let before = c.tangent_at(corner - 1e-6);
        let after = c.tangent_at(corner + 1e-6);
        assert!(dist2(c.point_at(corner), [20.0, 0.0]) < 1e-9);
        assert!(angle_between(before, after) > 10.0);
    }

    #[test]
    fn closed_all_smooth_has_no_kink() {
        let pts: Vec<_> = (0..5)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 5.0;
                Smooth([8.0 * a.cos(), 4.0 * a.sin()])
            })
            .collect();
        let c = build(&pts, true).unwrap();
        assert_eq!(c.segments.len(), 1);
        let len = c.length();
        let angle = angle_between(c.tangent_at(len - 1e-7), c.tangent_at(1e-7));
        assert!(angle < 0.5, "seam kink {angle} deg");
        let poly = c.tessellate(0.005);
        assert!(
            dist2(poly[0], *poly.last().unwrap()) > 1e-3,
            "first point repeated"
        );
        for item in &pts {
            let Smooth(p) = item else { unreachable!() };
            let best = poly
                .iter()
                .map(|&q| dist2(*p, q))
                .fold(f64::INFINITY, f64::min);
            assert!(best < 1e-9);
        }
    }

    #[test]
    fn single_smooth_point_curves() {
        let c = build(
            &[Corner([0.0, 0.0]), Smooth([5.0, 2.0]), Corner([10.0, 0.0])],
            false,
        )
        .unwrap();
        assert_eq!(c.segments.len(), 1);
        let Segment::Nurbs(n) = &c.segments[0] else {
            panic!("expected a spline")
        };
        assert_eq!(n.degree, 2);
        let poly = c.tessellate(0.005);
        assert!(poly.iter().any(|&q| dist2(q, [5.0, 2.0]) < 1e-9));
        assert!(!c.is_straight());
    }

    #[test]
    fn run_wraps_over_seam() {
        let c = build(
            &[
                Smooth([0.0, 0.0]),
                Corner([10.0, 0.0]),
                Corner([10.0, 10.0]),
                Smooth([0.0, 10.0]),
            ],
            true,
        )
        .unwrap();
        assert_eq!(c.segments.len(), 2, "{:?}", c.segments);
        assert!(matches!(
            c.segments[0],
            Segment::Line([10.0, 0.0], [10.0, 10.0])
        ));
        let Segment::Nurbs(n) = &c.segments[1] else {
            panic!("expected a spline")
        };
        assert_eq!(n.points[0], [10.0, 10.0]);
        assert_eq!(*n.points.last().unwrap(), [10.0, 0.0]);
        let poly = c.tessellate(0.005);
        for p in [[0.0, 10.0], [0.0, 0.0]] {
            assert!(poly.iter().any(|&q| dist2(q, p) < 1e-9));
        }
    }

    #[test]
    fn length_and_point_at_distance() {
        let line = build(&[Corner([0.0, 0.0]), Corner([10.0, 0.0])], false).unwrap();
        assert!((line.length() - 10.0).abs() < 1e-12);
        assert!(dist2(line.point_at(4.0), [4.0, 0.0]) < 1e-12);
        assert!(line.is_straight());

        let h = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve2 {
            segments: vec![Segment::Arc {
                from: [1.0, 0.0],
                via: [h, h],
                to: [0.0, 1.0],
            }],
            closed: false,
        };
        assert!((arc.length() - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(dist2(arc.point_at(std::f64::consts::FRAC_PI_4), [h, h]) < 1e-9);
        assert!(!arc.is_straight());

        // Spline: point_at(s) is s along the curve (numeric length of the piece up to s).
        let c = wall_spline();
        let s = 7.5;
        let p = c.point_at(s);
        let fine = c.tessellate(1e-6);
        let mut acc = 0.0;
        let mut found = f64::INFINITY;
        for w in fine.windows(2) {
            let d = dist2(w[0], w[1]);
            if acc + d >= s {
                let t = (s - acc) / d;
                found = dist2(
                    p,
                    [
                        w[0][0] + t * (w[1][0] - w[0][0]),
                        w[0][1] + t * (w[1][1] - w[0][1]),
                    ],
                );
                break;
            }
            acc += d;
        }
        assert!(found < 1e-4, "point_at off by {found}");
    }

    #[test]
    fn arc_in_3d_plane() {
        // Quarter circle of radius 2 in the plane x = z, tilted 45 degrees.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let c = Curve3 {
            segments: vec![Segment::Arc {
                from: [2.0 * h, 0.0, 2.0 * h],
                via: [h * h * 2.0, 2.0 * h, h * h * 2.0],
                to: [0.0, 2.0, 0.0],
            }],
            closed: false,
        };
        assert!((c.length() - std::f64::consts::PI).abs() < 1e-9);
        for p in c.tessellate(0.001) {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((r - 2.0).abs() < 1e-9);
            assert!((p[0] - p[2]).abs() < 1e-9);
        }
    }

    #[test]
    fn tessellation_within_tolerance() {
        let c = wall_spline();
        let poly = c.tessellate(0.005);
        let len = c.length();
        for i in 0..1000 {
            let p = c.point_at(len * i as f64 / 999.0);
            let d = dist_to_polyline(p, &poly, false);
            assert!(d <= 0.005 + 1e-9, "sample {i} is {d} from the polyline");
        }
    }

    #[test]
    fn rational_quarter_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let n = Nurbs::new(
            2,
            vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            Some(vec![1.0, w, 1.0]),
            Some(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        )
        .unwrap();
        let c = Curve2 {
            segments: vec![Segment::Nurbs(n)],
            closed: false,
        };
        let len = c.length();
        assert!((len - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        for i in 0..50 {
            let p = c.point_at(len * i as f64 / 49.0);
            assert!((dist2(p, [0.0, 0.0]) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn nurbs_validation() {
        let pts = vec![[0.0, 0.0], [1.0, 1.0], [2.0, 1.0], [3.0, 0.0]];
        assert_eq!(
            Nurbs::new(
                2,
                pts.clone(),
                None,
                Some(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
            )
            .unwrap_err(),
            CurveError::BadKnots {
                expected: 7,
                got: 6
            }
        );
        assert_eq!(
            Nurbs::new(2, pts.clone(), Some(vec![1.0, 0.0, 1.0, 1.0]), None).unwrap_err(),
            CurveError::BadWeight(1)
        );
        assert_eq!(
            Nurbs::new(0, pts.clone(), None, None).unwrap_err(),
            CurveError::BadDegree
        );
        assert_eq!(
            Nurbs::new(
                2,
                pts.clone(),
                None,
                Some(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            )
            .unwrap_err(),
            CurveError::UnclampedKnots
        );
        assert_eq!(
            Nurbs::new(
                2,
                pts.clone(),
                None,
                Some(vec![0.0, 0.0, 0.0, 0.6, 0.4, 1.0, 1.0])
            )
            .unwrap_err(),
            CurveError::DecreasingKnots(4)
        );
        assert_eq!(
            Nurbs::new(3, pts[..3].to_vec(), None, None).unwrap_err(),
            CurveError::TooFewPoints
        );
        let ok = Nurbs::new(2, pts, None, None).unwrap();
        assert_eq!(ok.weights, vec![1.0; 4]);
        assert_eq!(ok.knots, vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn repeated_point_in_run_is_error() {
        let err = build(
            &[
                Corner([0.0, 0.0]),
                Smooth([5.0, 2.0]),
                Smooth([5.0, 2.0]),
                Corner([10.0, 0.0]),
            ],
            false,
        )
        .unwrap_err();
        assert_eq!(err, CurveError::RepeatedPoint(2));
    }

    #[test]
    fn between_keeps_the_curve() {
        let c = wall_spline();
        let len = c.length();
        let part = c.between(2.0, len - 3.0);
        assert!((part.length() - (len - 5.0)).abs() < 1e-6);
        for s in [0.0, 1.0, 5.5, len - 5.0] {
            assert!(
                dist2(part.point_at(s), c.point_at(s + 2.0)) < 1e-6,
                "at {s}"
            );
        }
        let start = match &part.segments[0] {
            Segment::Nurbs(n) => n.points[0],
            s => panic!("{s:?}"),
        };
        assert!(dist2(start, c.point_at(2.0)) < 1e-9);
    }

    #[test]
    fn zero_length_knot_span_has_a_point() {
        // Three equal control points make the first knot span a single point.
        let n = Nurbs::new(
            2,
            vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
            None,
            None,
        )
        .unwrap();
        let c = Curve {
            segments: vec![Segment::Nurbs(n)],
            closed: false,
        };
        let p = c.point_at(0.0);
        assert!(dist2(p, [0.0, 0.0]) < 1e-12, "{p:?}");
    }

    #[test]
    fn nurbs_item_joins_corners() {
        // Spec §1 exact form: the nurbs ends coincide with the corners either side.
        let n = Nurbs::new(
            3,
            vec![[0.0, 0.0], [4.0, -3.0], [16.0, -3.0], [20.0, 0.0]],
            None,
            None,
        )
        .unwrap();
        let c = build(
            &[
                Corner([0.0, 0.0]),
                PathItem::Nurbs(n),
                Corner([20.0, 15.0]),
                Corner([0.0, 15.0]),
            ],
            true,
        )
        .unwrap();
        assert_eq!(c.segments.len(), 4, "{:?}", c.segments);
        assert!(matches!(c.segments[0], Segment::Nurbs(_)));
        let poly = c.tessellate(0.005);
        assert_eq!(poly[0], [0.0, 0.0]);
        assert!(poly.contains(&[20.0, 0.0]) && poly.contains(&[20.0, 15.0]));
    }

    #[test]
    fn arc_as_nurbs_stays_on_circle() {
        // Three quarters of a circle of radius 5 about (1, 2): three pieces.
        let c = Circle::through([6.0, 2.0], [1.0, 7.0], [1.0, -3.0]).unwrap();
        let n = c.to_nurbs();
        assert_eq!(n.points.len(), 7);
        assert_eq!(n.knots.len(), 10);
        assert!(dist2(n.points[0], [6.0, 2.0]) < 1e-9);
        assert!(dist2(*n.points.last().unwrap(), [1.0, -3.0]) < 1e-9);
        for i in 0..=30 {
            let p = n.eval(i as f64 / 30.0);
            let r = (p[0] - 1.0).hypot(p[1] - 2.0);
            assert!((r - 5.0).abs() < 1e-9, "{p:?}");
        }
    }
}
