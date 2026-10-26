use std::collections::HashMap;

use crate::geometry::curve::{Curve, Curve2, Curve3, Nurbs};

// ============================================================================
// Geometry description
// ============================================================================

/// A right-handed placement: origin plus local Z axis and local X reference direction.
/// All element frames are relative to the storey placement.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) origin: [f64; 3],
    pub(crate) z: [f64; 3],
    pub(crate) x: [f64; 3],
}

impl Frame {
    /// Upright frame at `origin`, rotated `angle` radians about Z.
    pub(crate) fn upright(origin: [f64; 3], angle: f64) -> Self {
        Frame {
            origin,
            z: [0.0, 0.0, 1.0],
            x: [angle.cos(), angle.sin(), 0.0],
        }
    }

    /// Frame of a linear element from `p0` along `d`: local Z along the axis,
    /// local X horizontal (world X for a vertical axis), turned `rotation`
    /// radians about the axis toward local Y.
    pub(crate) fn along(p0: [f64; 3], d: [f64; 3], rotation: f64) -> Self {
        let z = normalize(d);
        let h = cross([0.0, 0.0, 1.0], z);
        let x = if dot(h, h).sqrt() < 1e-9 {
            [1.0, 0.0, 0.0]
        } else {
            normalize(h)
        };
        let y = cross(z, x);
        let (c, s) = (rotation.cos(), rotation.sin());
        Frame {
            origin: p0,
            z,
            x: std::array::from_fn(|i| x[i] * c + y[i] * s),
        }
    }

    /// Point given in this frame, in the parent system.
    pub(crate) fn to_parent(self, p: [f64; 3]) -> [f64; 3] {
        let y = self.y();
        std::array::from_fn(|i| self.origin[i] + self.x[i] * p[0] + y[i] * p[1] + self.z[i] * p[2])
    }

    pub(crate) fn identity() -> Self {
        Self::upright([0.0; 3], 0.0)
    }

    pub(crate) fn y(&self) -> [f64; 3] {
        cross(self.z, self.x)
    }

    /// Express a point given in the parent (storey) system in this frame.
    pub(crate) fn to_local(self, p: [f64; 3]) -> [f64; 3] {
        self.dir_to_local([
            p[0] - self.origin[0],
            p[1] - self.origin[1],
            p[2] - self.origin[2],
        ])
    }

    /// Express a direction given in the parent system in this frame.
    pub(crate) fn dir_to_local(self, d: [f64; 3]) -> [f64; 3] {
        [dot(d, self.x), dot(d, self.y()), dot(d, self.z)]
    }

    /// `other` (in the parent system) expressed in this frame.
    pub(crate) fn relative(self, other: &Frame) -> Frame {
        Frame {
            origin: self.to_local(other.origin),
            z: self.dir_to_local(other.z),
            x: self.dir_to_local(other.x),
        }
    }

    /// Plan point (storey XY at height `z`) to this frame's local XY.
    pub(crate) fn plan_to_local(&self, p: (f64, f64), z: f64) -> (f64, f64) {
        let l = self.to_local([p.0, p.1, z]);
        (l[0], l[1])
    }
}

/// Swept-area profile, in the XY plane of the solid's position.
#[derive(Debug, Clone)]
pub(crate) enum Profile {
    /// Rectangle centred at (cx, cy), `w` along local X and `d` along local Y.
    Rect {
        cx: f64,
        cy: f64,
        w: f64,
        d: f64,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
    },
    Poly {
        outer: Vec<(f64, f64)>,
        holes: Vec<Vec<(f64, f64)>>,
    },
    /// Closed exact curves, written as B-splines.
    Curve {
        outer: Curve2,
        holes: Vec<Curve2>,
    },
}

/// A profile extruded along the local Z of `position` by `depth`, or revolved
/// about an axis in its plane.
#[derive(Debug, Clone)]
pub(crate) struct Solid {
    pub(crate) profile: Profile,
    /// Placement of the solid inside the element's frame.
    pub(crate) position: Frame,
    pub(crate) depth: f64,
    /// When set, the profile is revolved instead of extruded (`depth` unused).
    pub(crate) revolve: Option<Revolve>,
    /// When set, the profile is swept along this curve instead (`depth`
    /// unused): profile X stays toward +Z, profile Y to the right of the
    /// curve. A circle centred on the origin becomes a swept disk.
    pub(crate) directrix: Option<Curve3>,
}

/// Axis in the profile plane (`origin`, `dir`) and the angle in radians the
/// profile turns about it (right-handed).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Revolve {
    pub(crate) origin: (f64, f64),
    pub(crate) dir: (f64, f64),
    pub(crate) angle: f64,
}

/// Geometry of one element: its frame (relative to the storey) and solids in that frame.
pub(crate) struct Shape {
    pub(crate) frame: Frame,
    pub(crate) solids: Vec<Solid>,
}

impl Solid {
    /// Profile in the element's XY plane, extruded upward from `z0` by `depth`
    /// (a negative depth extrudes downward).
    pub(crate) fn vertical(profile: Profile, z0: f64, depth: f64) -> Self {
        let (z0, depth) = if depth < 0.0 {
            (z0 + depth, -depth)
        } else {
            (z0, depth)
        };
        Solid {
            profile,
            position: Frame::upright([0.0, 0.0, z0], 0.0),
            depth,
            revolve: None,
            directrix: None,
        }
    }

    /// `profile` swept along `path` (in the element frame).
    pub(crate) fn swept(profile: Profile, path: Curve3) -> Self {
        Solid {
            profile,
            position: Frame::identity(),
            depth: 0.0,
            revolve: None,
            directrix: Some(path),
        }
    }
}

// ============================================================================
// STEP writer
// ============================================================================

/// Emits STEP entity lines with auto-incrementing ids. Points, directions and
/// placements are deduplicated by their text.
pub(crate) struct StepWriter {
    next_id: u64,
    pub(crate) lines: String,
    shared: HashMap<String, u64>,
}

impl StepWriter {
    pub(crate) fn new() -> Self {
        Self {
            next_id: 1,
            lines: String::new(),
            shared: HashMap::new(),
        }
    }

    /// Emit `#id=<body>;` and return the id.
    pub(crate) fn add(&mut self, body: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.lines.push_str(&format!("#{}={};\n", id, body));
        id
    }

    /// Like `add`, but reuses an identical earlier entity.
    fn add_shared(&mut self, body: String) -> u64 {
        if let Some(&id) = self.shared.get(&body) {
            return id;
        }
        let id = self.add(&body);
        self.shared.insert(body, id);
        id
    }

    pub(crate) fn point2(&mut self, p: (f64, f64)) -> u64 {
        self.point([p.0, p.1])
    }

    pub(crate) fn dir3(&mut self, d: [f64; 3]) -> u64 {
        self.add_shared(format!(
            "IFCDIRECTION(({},{},{}))",
            sf(d[0]),
            sf(d[1]),
            sf(d[2])
        ))
    }

    fn dir2(&mut self, d: (f64, f64)) -> u64 {
        self.add_shared(format!("IFCDIRECTION(({},{}))", sf(d.0), sf(d.1)))
    }

    pub(crate) fn placement3(&mut self, f: &Frame) -> u64 {
        let o = self.point(f.origin);
        let z = self.dir3(f.z);
        let x = self.dir3(f.x);
        self.add_shared(format!("IFCAXIS2PLACEMENT3D(#{},#{},#{})", o, z, x))
    }

    /// IfcLocalPlacement of `frame` relative to `parent` (or absolute).
    pub(crate) fn local_placement(&mut self, parent: Option<u64>, frame: &Frame) -> u64 {
        let p = self.placement3(frame);
        let rel = parent.map_or("$".to_string(), |id| format!("#{}", id));
        self.add(&format!("IFCLOCALPLACEMENT({},#{})", rel, p))
    }

    fn placement2(&mut self, c: (f64, f64)) -> u64 {
        let o = self.point2(c);
        let x = self.dir2((1.0, 0.0));
        self.add_shared(format!("IFCAXIS2PLACEMENT2D(#{},#{})", o, x))
    }

    fn polyline(&mut self, pts: &[(f64, f64)]) -> u64 {
        let mut ids: Vec<u64> = pts.iter().map(|p| self.point2(*p)).collect();
        ids.push(ids[0]);
        self.add(&format!("IFCPOLYLINE(({}))", refs(&ids)))
    }

    fn point<const N: usize>(&mut self, p: [f64; N]) -> u64 {
        let c: Vec<String> = p.iter().map(|v| sf(*v)).collect();
        self.add_shared(format!("IFCCARTESIANPOINT(({}))", c.join(",")))
    }

    /// A curve of straight lines is one IfcPolyline. Otherwise an
    /// IfcCompositeCurve of polylines (runs of lines) and trimmed B-splines
    /// (spline and NURBS segments, arcs as exact rational quadratics). IFC wants
    /// the last segment of an open composite curve `.DISCONTINUOUS.`.
    fn curve<const N: usize>(&mut self, c: &Curve<[f64; N]>) -> u64 {
        let mut parts = Vec::new();
        let mut line: Vec<u64> = Vec::new();
        for seg in &c.segments {
            match seg.to_nurbs() {
                Some(n) => {
                    if line.len() > 1 {
                        parts.push(self.add(&format!("IFCPOLYLINE(({}))", refs(&line))));
                    }
                    line.clear();
                    // IfcOpenShell only reads a B-spline segment wrapped in a
                    // trimmed curve over its whole knot range, written as 0..1.
                    let b = self.bspline(&n);
                    parts.push(self.add(&format!(
                        "IFCTRIMMEDCURVE(#{b},(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(1.)),.T.,.PARAMETER.)"
                    )));
                }
                None => {
                    for p in [seg.start(), seg.end()] {
                        let id = self.point(p);
                        if line.last() != Some(&id) {
                            line.push(id);
                        }
                    }
                }
            }
        }
        let only_lines = parts.is_empty();
        if only_lines && line.len() == 1 {
            // All points within the file's 0.01 mm: keep a (zero-length) line.
            line.push(line[0]);
        }
        if line.len() > 1 {
            parts.push(self.add(&format!("IFCPOLYLINE(({}))", refs(&line))));
        }
        if only_lines {
            return parts[0];
        }
        let n = parts.len();
        let segs: Vec<u64> = parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let t = if i + 1 == n && !c.closed {
                    ".DISCONTINUOUS."
                } else {
                    ".CONTINUOUS."
                };
                self.add(&format!("IFCCOMPOSITECURVESEGMENT({t},.T.,#{p})"))
            })
            .collect();
        self.add(&format!("IFCCOMPOSITECURVE(({}),.F.)", refs(&segs)))
    }

    /// IfcBSplineCurveWithKnots, rational when a weight is not 1. Knots are
    /// written as distinct values with multiplicities, normalized to 0..1.
    fn bspline<const N: usize>(&mut self, n: &Nurbs<[f64; N]>) -> u64 {
        let pts: Vec<u64> = n.points.iter().map(|p| self.point(*p)).collect();
        let (mut mults, mut knots) = (Vec::<String>::new(), Vec::<f64>::new());
        for k in n.normalized_knots() {
            if knots.last().is_some_and(|l| (k - l).abs() < 1e-12) {
                let m = mults.last_mut().unwrap();
                *m = (m.parse::<usize>().unwrap() + 1).to_string();
            } else {
                knots.push(k);
                mults.push("1".to_string());
            }
        }
        let closed = if pts.first() == pts.last() {
            ".T."
        } else {
            ".F."
        };
        let knots: Vec<String> = knots.iter().map(|k| fine(*k)).collect();
        let body = format!(
            "{},({}),.UNSPECIFIED.,{closed},.F.,({}),({}),.UNSPECIFIED.",
            n.degree,
            refs(&pts),
            mults.join(","),
            knots.join(",")
        );
        if n.weights.iter().all(|w| (w - 1.0).abs() < 1e-12) {
            self.add(&format!("IFCBSPLINECURVEWITHKNOTS({body})"))
        } else {
            let w: Vec<String> = n.weights.iter().map(|w| fine(*w)).collect();
            self.add(&format!(
                "IFCRATIONALBSPLINECURVEWITHKNOTS({body},({}))",
                w.join(",")
            ))
        }
    }

    pub(crate) fn polyline2(&mut self, a: (f64, f64), b: (f64, f64)) -> u64 {
        let (a, b) = (self.point2(a), self.point2(b));
        self.add(&format!("IFCPOLYLINE((#{},#{}))", a, b))
    }

    fn profile(&mut self, p: &Profile) -> u64 {
        match p {
            Profile::Rect { cx, cy, w, d } => {
                let pl = self.placement2((*cx, *cy));
                self.add(&format!(
                    "IFCRECTANGLEPROFILEDEF(.AREA.,$,#{},{},{})",
                    pl,
                    sf(w.max(MIN_SIZE)),
                    sf(d.max(MIN_SIZE))
                ))
            }
            Profile::Circle { cx, cy, r } => {
                let pl = self.placement2((*cx, *cy));
                self.add(&format!(
                    "IFCCIRCLEPROFILEDEF(.AREA.,$,#{},{})",
                    pl,
                    sf(r.max(MIN_SIZE))
                ))
            }
            Profile::Poly { outer, holes } => {
                let outer = clean_ring(outer);
                let o = self.polyline(&outer);
                let holes: Vec<u64> = holes
                    .iter()
                    .map(|h| clean_ring(h))
                    .filter(|h| h.len() >= 3)
                    .map(|h| self.polyline(&h))
                    .collect();
                self.arbitrary_profile(o, &holes)
            }
            Profile::Curve { outer, holes } => {
                let o = self.curve(outer);
                let holes: Vec<u64> = holes.iter().map(|h| self.curve(h)).collect();
                self.arbitrary_profile(o, &holes)
            }
        }
    }

    fn arbitrary_profile(&mut self, outer: u64, holes: &[u64]) -> u64 {
        if holes.is_empty() {
            self.add(&format!(
                "IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{})",
                outer
            ))
        } else {
            self.add(&format!(
                "IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#{},({}))",
                outer,
                refs(holes)
            ))
        }
    }

    fn solid(&mut self, s: &Solid) -> u64 {
        if let Some(path) = &s.directrix {
            let d = self.curve(path);
            if let Profile::Circle { cx, cy, r } = s.profile
                && cx.abs() + cy.abs() < 1e-9
            {
                return self.add(&format!("IFCSWEPTDISKSOLID(#{},{},$,$,$)", d, sf(r)));
            }
            let prof = self.profile(&s.profile);
            let pos = self.placement3(&s.position);
            let up = self.dir3([0.0, 0.0, 1.0]);
            return self.add(&format!(
                "IFCFIXEDREFERENCESWEPTAREASOLID(#{},#{},#{},$,$,#{})",
                prof, pos, d, up
            ));
        }
        let prof = self.profile(&s.profile);
        let pos = self.placement3(&s.position);
        if let Some(r) = s.revolve {
            let o = self.point([r.origin.0, r.origin.1, 0.0]);
            let d = self.dir3([r.dir.0, r.dir.1, 0.0]);
            let axis = self.add(&format!("IFCAXIS1PLACEMENT(#{},#{})", o, d));
            return self.add(&format!(
                "IFCREVOLVEDAREASOLID(#{},#{},#{},{})",
                prof,
                pos,
                axis,
                sf(r.angle)
            ));
        }
        let dir = self.dir3([0.0, 0.0, 1.0]);
        self.add(&format!(
            "IFCEXTRUDEDAREASOLID(#{},#{},#{},{})",
            prof,
            pos,
            dir,
            sf(s.depth.max(MIN_SIZE))
        ))
    }

    /// IfcProductDefinitionShape with one 'Body' representation.
    pub(crate) fn body(&mut self, context: u64, solids: &[Solid]) -> u64 {
        let rep = self.body_rep(context, solids);
        self.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{}))", rep))
    }

    /// Like `body`, with an 'Axis' Curve2D representation of `axis` first.
    pub(crate) fn body_with_axis(
        &mut self,
        context: u64,
        axis_context: u64,
        solids: &[Solid],
        axis: &Curve2,
    ) -> u64 {
        let curve = self.curve(axis);
        let axis = self.add(&format!(
            "IFCSHAPEREPRESENTATION(#{},'Axis','Curve2D',(#{}))",
            axis_context, curve
        ));
        let rep = self.body_rep(context, solids);
        self.add(&format!(
            "IFCPRODUCTDEFINITIONSHAPE($,$,(#{},#{}))",
            axis, rep
        ))
    }

    /// 'SweptSolid' (or 'AdvancedSweptSolid' for curve sweeps) representation.
    fn body_rep(&mut self, context: u64, solids: &[Solid]) -> u64 {
        let items: Vec<u64> = solids.iter().map(|s| self.solid(s)).collect();
        let kind = if solids.iter().any(|s| s.directrix.is_some()) {
            "AdvancedSweptSolid"
        } else {
            "SweptSolid"
        };
        self.add(&format!(
            "IFCSHAPEREPRESENTATION(#{},'Body','{}',({}))",
            context,
            kind,
            refs(&items)
        ))
    }
}

/// Smallest length written for profile sizes and extrusion depths
/// (IFC requires positive lengths).
const MIN_SIZE: f64 = 0.001;

/// Drop a repeated closing point and consecutive duplicates.
fn clean_ring(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for p in pts {
        if out
            .last()
            .is_none_or(|q| (q.0 - p.0).abs() + (q.1 - p.1).abs() > 1e-6)
        {
            out.push(*p);
        }
    }
    if out.len() > 1 {
        let (f, l) = (out[0], out[out.len() - 1]);
        if (f.0 - l.0).abs() + (f.1 - l.1).abs() <= 1e-6 {
            out.pop();
        }
    }
    out
}

// ============================================================================
// Helper functions
// ============================================================================

pub(crate) fn refs(ids: &[u64]) -> String {
    ids.iter()
        .map(|i| format!("#{}", i))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

/// `a + b * s`.
pub(crate) fn add(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i] * s)
}

pub(crate) fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// `v` turned by `angle` about the unit `axis` (Rodrigues).
pub(crate) fn rotate(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let (s, c) = angle.sin_cos();
    add(
        add(v.map(|x| x * c), cross(axis, v), s),
        axis,
        dot(axis, v) * (1.0 - c),
    )
}

pub(crate) fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = dot(a, a).sqrt();
    if n < 1e-12 {
        a
    } else {
        [a[0] / n, a[1] / n, a[2] / n]
    }
}

/// Format a float for STEP output (always includes decimal point).
pub(crate) fn sf(v: f64) -> String {
    // Round to 0.01 mm so float noise does not leak into the file.
    let v = (v * 1e5).round() / 1e5;
    if v == 0.0 {
        "0.".to_string()
    } else if v.fract().abs() < 1e-10 {
        format!("{}.", v.round() as i64)
    } else {
        format!("{}", v)
    }
}

/// A knot or weight for STEP output: unitless, so kept to 1e-12 where `sf`
/// would merge distinct knots.
fn fine(v: f64) -> String {
    let s = format!("{}", (v * 1e12).round() / 1e12);
    if s.contains('.') { s } else { s + "." }
}

/// Encode a string as a STEP string literal, quotes included. Quotes are doubled,
/// backslashes escaped and non-ASCII characters written as `\X2\hhhh\X0\`.
pub(crate) fn step_str(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                out.push_str("\\X2\\");
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("{:04X}", u));
                }
                out.push_str("\\X0\\");
            }
        }
    }
    out.push('\'');
    out
}

/// Generate a unique IFC GUID (22 base64 characters).
///
/// The IFC GUID encodes a 128-bit number into 22 base64 characters using the
/// IFC-specific alphabet `0-9 A-Z a-z _ $`. Since 22 × 6 = 132 bits, the first
/// character can only encode 4 bits (values 0–3) to stay within 128 bits.
///
/// Uses an atomic counter as the primary entropy source, mixed with a timestamp
/// via SplitMix64 to guarantee uniqueness even when multiple GUIDs are generated
/// within the same nanosecond.
pub(crate) fn generate_ifc_guid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);

    // SplitMix64 mixing function for good bit distribution
    let mix = |mut z: u64| -> u64 {
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    };

    // Generate two 64-bit halves from different inputs
    let hi = mix(timestamp.wrapping_add(count.wrapping_mul(2654435761)));
    let lo = mix(count.wrapping_add(timestamp.wrapping_mul(2246822519)));

    // Combine into 128-bit value
    let val128: u128 = ((hi as u128) << 64) | (lo as u128);

    // Encode as 22 base64 characters, most significant first
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let mut guid = String::with_capacity(22);
    let mut remaining = val128;

    // Extract 22 digits in base-64, but first digit is base-4 (2 bits)
    // Process from most significant to least significant
    // Total bits: 2 + 21*6 = 128
    let first = (remaining >> 126) as usize; // Top 2 bits
    guid.push(chars[first] as char);
    remaining &= (1u128 << 126) - 1; // Clear top 2 bits

    for _ in 0..21 {
        let digit = (remaining >> 120) as usize; // Top 6 bits of remaining
        guid.push(chars[digit & 63] as char);
        remaining <<= 6;
    }

    guid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::curve::Segment;

    #[test]
    fn close_knots_and_weights_stay_distinct() {
        let n = Nurbs::new(
            2,
            vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0], [4.0, 0.0]],
            Some(vec![1.0, 0.7000004, 1.0, 1.0, 1.0]),
            Some(vec![0.0, 0.0, 0.0, 0.5, 0.500004, 1.0, 1.0, 1.0]),
        )
        .unwrap();
        let mut w = StepWriter::new();
        w.bspline(&n);
        assert!(w.lines.contains("(0.,0.5,0.500004,1.)"), "{}", w.lines);
        assert!(w.lines.contains("0.7000004"), "{}", w.lines);
    }

    #[test]
    fn curve_shorter_than_the_file_resolution_is_written() {
        let c = Curve {
            segments: vec![Segment::Line([0.0, 0.0], [1e-7, 0.0])],
            closed: false,
        };
        let mut w = StepWriter::new();
        let id = w.curve(&c);
        assert!(
            w.lines.contains(&format!("#{id}=IFCPOLYLINE(")),
            "{}",
            w.lines
        );
    }
}
