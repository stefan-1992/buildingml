//! Wall joins in plan. Two straight walls ending at the same point meet in a
//! mitre; a wall ending on another wall, or where a straight run of two walls
//! meets, is cut back to that wall's face. The IFC writer and the scene graph
//! both draw walls with these ends (`Specs_V0_3.md` §19).

use std::collections::HashMap;

use super::geometry::wall_length as dist;
use super::ifc_writer::no_body;
use crate::config::default_curve_tolerance;
use crate::geometry::axis::WallAxis;
use crate::scene::graph::get_polygon;
use crate::semantic::model::{ElementType, ResolvedElement, number};

type P = (f64, f64);

/// Wall ends closer than this meet.
const EPS: f64 = 0.01;
/// Walls closer to parallel than 20° are not joined (sin 20°).
const MIN_SIN: f64 = 0.342;

/// Local x (along the axis from `from`) where each face of a straight wall
/// stops: `[start right, start left, end right, end left]`, left being local +y.
pub type WallEnds = [f64; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JoinEnd {
    Start,
    End,
    Path,
}

impl JoinEnd {
    fn of(end: usize) -> Self {
        if end == 0 {
            JoinEnd::Start
        } else {
            JoinEnd::End
        }
    }
}

/// Two joined walls; `related` ends on `relating`.
#[derive(Debug, Clone, PartialEq)]
pub struct Join {
    pub relating: String,
    pub relating_end: JoinEnd,
    pub related: String,
    pub related_end: JoinEnd,
}

#[derive(Debug, Default)]
pub struct Joins {
    /// Ends of the walls a join changed, by element id.
    pub ends: HashMap<String, WallEnds>,
    pub joins: Vec<Join>,
}

struct Wall<'a> {
    id: &'a str,
    from: P,
    u: P,
    len: f64,
    t: f64,
    /// Base (`z_offset`) and `height` (None: the storey height).
    z: (f64, Option<f64>),
}

impl Wall<'_> {
    fn of(e: &ResolvedElement) -> Option<Wall<'_>> {
        let p = &e.properties;
        if e.element_type != ElementType::Wall || no_body(e) || get_polygon(p, "boundary").is_some()
        {
            return None;
        }
        // Curved walls are not joined.
        let [from, to] =
            WallAxis::straight_ends(e, default_curve_tolerance())?.map(|q| (q[0], q[1]));
        let len = dist(from, to);
        (len > EPS).then(|| Wall {
            id: &e.id,
            from,
            u: ((to.0 - from.0) / len, (to.1 - from.1) / len),
            len,
            t: number(p, "thickness").unwrap_or(0.2),
            z: (number(p, "z_offset").unwrap_or(0.0), number(p, "height")),
        })
    }

    /// Whether this wall spans at least the height of `o`.
    fn covers(&self, o: &Wall) -> bool {
        match (self.z, o.z) {
            ((a, Some(h)), (b, Some(g))) => a <= b + EPS && a + h >= b + g - EPS,
            ((a, h), (b, g)) => (a - b).abs() < EPS && h == g,
        }
    }

    fn point(&self, end: usize) -> P {
        let s = if end == 0 { 0.0 } else { self.len };
        (self.from.0 + self.u.0 * s, self.from.1 + self.u.1 * s)
    }

    /// Unit direction from `end` into the wall.
    fn inward(&self, end: usize) -> P {
        if end == 0 {
            self.u
        } else {
            (-self.u.0, -self.u.1)
        }
    }

    /// Local +y.
    fn normal(&self) -> P {
        (-self.u.1, self.u.0)
    }

    /// The face on local side `side` (±1) as a point and direction.
    fn face(&self, side: f64) -> (P, P) {
        let (n, h) = (self.normal(), side * self.t / 2.0);
        ((self.from.0 + n.0 * h, self.from.1 + n.1 * h), self.u)
    }

    /// Local x where face `side` crosses `line`; None if parallel.
    fn cut(&self, side: f64, (q, e): (P, P)) -> Option<f64> {
        let (p, d) = self.face(side);
        let den = cross(d, e);
        (den.abs() > 1e-9).then(|| cross((q.0 - p.0, q.1 - p.1), e) / den)
    }

    /// Whether `p` lies inside the wall away from its ends.
    fn passes(&self, p: P) -> bool {
        let r = (p.0 - self.from.0, p.1 - self.from.1);
        let (s, d) = (dot(r, self.u), dot(r, self.normal()));
        d.abs() <= self.t / 2.0 + EPS && s > EPS && s < self.len - EPS
    }
}

/// Index into `WallEnds` of the face `side` at `end`.
fn slot(end: usize, side: f64) -> usize {
    2 * end + usize::from(side > 0.0)
}

/// Join the straight walls among `elements` (one storey, building or site).
pub fn join_walls<'a>(elements: impl IntoIterator<Item = &'a ResolvedElement>) -> Joins {
    let walls: Vec<Wall> = elements.into_iter().filter_map(Wall::of).collect();
    let mut out = Joins::default();
    let mut set = |w: &Wall, slot: usize, x: f64| {
        out.ends
            .entry(w.id.to_string())
            .or_insert([0.0, 0.0, w.len, w.len])[slot] = x;
    };
    let mut joins = Vec::new();
    let mut seen = vec![[false; 2]; walls.len()];
    let parallel = |a: P, b: P| cross(a, b).abs() < MIN_SIN;

    for i in 0..walls.len() {
        for e in 0..2 {
            if seen[i][e] {
                continue;
            }
            let p = walls[i].point(e);
            let meet: Vec<(usize, usize)> = (0..walls.len())
                .flat_map(|j| [(j, 0), (j, 1)])
                .filter(|&(j, f)| dist(walls[j].point(f), p) < EPS)
                .collect();
            for &(j, f) in &meet {
                seen[j][f] = true;
            }

            // A wall running through the point, else a straight run of two walls ending there.
            let through = (0..walls.len())
                .find(|&k| {
                    walls[k].passes(p)
                        && meet
                            .iter()
                            .all(|&(j, _)| j != k && !parallel(walls[j].u, walls[k].u))
                })
                .map(|k| (k, JoinEnd::Path))
                .or_else(|| {
                    meet.iter().enumerate().find_map(|(n, &(a, ea))| {
                        meet[n + 1..]
                            .iter()
                            .any(|&(b, eb)| dot(walls[a].inward(ea), walls[b].inward(eb)) < -0.99)
                            .then_some((a, JoinEnd::of(ea)))
                    })
                });

            if let Some((k, end_k)) = through {
                let wk = &walls[k];
                for &(j, f) in &meet {
                    let inward = walls[j].inward(f);
                    if j == k || parallel(inward, wk.u) || !wk.covers(&walls[j]) {
                        continue;
                    }
                    let face = wk.face(dot(inward, wk.normal()).signum());
                    for side in [-1.0, 1.0] {
                        if let Some(x) = walls[j].cut(side, face) {
                            set(&walls[j], slot(f, side), x);
                        }
                    }
                    joins.push((k, end_k, j, JoinEnd::of(f)));
                }
            } else if let [(a, ea), (b, eb)] = meet[..]
                && !parallel(walls[a].u, walls[b].u)
                && walls[a].covers(&walls[b])
                && walls[b].covers(&walls[a])
            {
                // Left of each wall seen from the corner: local +y at the start, -y at the end.
                let left = |end: usize| if end == 0 { 1.0 } else { -1.0 };
                let (la, lb) = (left(ea), left(eb));
                for (sa, sb) in [(la, -lb), (-la, lb)] {
                    if let (Some(xa), Some(xb)) = (
                        walls[a].cut(sa, walls[b].face(sb)),
                        walls[b].cut(sb, walls[a].face(sa)),
                    ) {
                        set(&walls[a], slot(ea, sa), xa);
                        set(&walls[b], slot(eb, sb), xb);
                    }
                }
                joins.push((a, JoinEnd::of(ea), b, JoinEnd::of(eb)));
            }
        }
    }

    // A mitre longer than a short wall would turn its outline inside out.
    out.ends
        .retain(|_, x| x[0] < x[2] - EPS && x[1] < x[3] - EPS);
    out.joins = joins
        .into_iter()
        .map(|(a, ea, b, eb)| Join {
            relating: walls[a].id.to_string(),
            relating_end: ea,
            related: walls[b].id.to_string(),
            related_end: eb,
        })
        .collect();
    out
}

fn dot(a: P, b: P) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: P, b: P) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    /// Joins of walls `(id, from, to, thickness)` in one storey.
    fn joins(walls: &[(&str, P, P, f64)]) -> Joins {
        let mut src = String::from(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  height: 3 m\n",
        );
        for (id, a, b, t) in walls {
            src += &format!(
                "\n  Wall: {id}\n    from: ({}, {}) to: ({}, {})\n    height: 3 m\n    thickness: {t} m\n",
                a.0, a.1, b.0, b.1
            );
        }
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        join_walls(model.elements())
    }

    fn close(a: WallEnds, b: WallEnds) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn l_corner_is_mitred() {
        // A runs +x from the corner, B arrives at it from +y.
        let j = joins(&[
            ("A", (0.0, 0.0), (5.0, 0.0), 0.2),
            ("B", (0.0, 4.0), (0.0, 0.0), 0.3),
        ]);
        // A's inner (+y) face stops at B's inner face x = 0.15, the outer one at -0.15.
        assert!(
            close(j.ends["A"], [-0.15, 0.15, 5.0, 5.0]),
            "{:?}",
            j.ends["A"]
        );
        // B points -y, so its left (+y local) face is at +x: inner, stops at y = 0.1.
        assert!(
            close(j.ends["B"], [0.0, 0.0, 4.1, 3.9]),
            "{:?}",
            j.ends["B"]
        );
        assert_eq!(j.joins.len(), 1);
        assert_eq!(
            (j.joins[0].relating_end, j.joins[0].related_end),
            (JoinEnd::Start, JoinEnd::End)
        );
    }

    #[test]
    fn t_end_is_cut_to_face() {
        let j = joins(&[
            ("B", (0.0, 0.0), (6.0, 0.0), 0.3),
            ("A", (3.0, 4.0), (3.0, 0.0), 0.2),
        ]);
        assert!(!j.ends.contains_key("B"));
        assert!(
            close(j.ends["A"], [0.0, 0.0, 3.85, 3.85]),
            "{:?}",
            j.ends["A"]
        );
        assert_eq!(j.joins[0].relating, "B");
        assert_eq!(j.joins[0].relating_end, JoinEnd::Path);
    }

    #[test]
    fn straight_run_with_branch() {
        // B1 + B2 form a straight run; A ends where they meet.
        let j = joins(&[
            ("B1", (0.0, 0.0), (3.0, 0.0), 0.3),
            ("B2", (3.0, 0.0), (6.0, 0.0), 0.3),
            ("A", (3.0, 0.0), (3.0, -4.0), 0.2),
        ]);
        assert_eq!(j.ends.len(), 1);
        assert!(
            close(j.ends["A"], [0.15, 0.15, 4.0, 4.0]),
            "{:?}",
            j.ends["A"]
        );
    }

    #[test]
    fn short_wall_at_acute_corner_keeps_square_ends() {
        let j = joins(&[
            ("A", (0.0, 0.0), (5.0, 0.0), 0.3),
            ("B", (0.26, 0.15), (0.0, 0.0), 0.3),
        ]);
        assert!(!j.ends.contains_key("B"), "{:?}", j.ends);
    }

    #[test]
    fn walls_of_other_heights_stay() {
        let mut src = String::from(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  height: 3 m\n",
        );
        src += "\n  Wall: A\n    from: (0, 0) to: (5, 0)\n    height: 3 m\n";
        src += "\n  Wall: P\n    from: (0, 4) to: (0, 0)\n    height: 1.1 m\n";
        src += "\n  Wall: T\n    from: (2, 4) to: (2, 0)\n    height: 3 m\n    z_offset: 0.5 m\n";
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        let j = join_walls(model.elements());
        assert!(j.ends.is_empty() && j.joins.is_empty(), "{:?}", j.joins);
    }

    #[test]
    fn parallel_and_apart_walls_stay() {
        let j = joins(&[
            ("A", (0.0, 0.0), (3.0, 0.0), 0.2),
            ("B", (3.0, 0.0), (6.0, 0.0), 0.2),
            ("C", (0.0, 2.0), (3.0, 2.0), 0.2),
        ]);
        assert!(j.ends.is_empty() && j.joins.is_empty());
    }
}
