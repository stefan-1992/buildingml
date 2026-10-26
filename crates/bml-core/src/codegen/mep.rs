//! Geometry and IFC classes of building services (`Specs_V0_3.md` §24):
//! duct, pipe and cable tray segments and fittings, terminals and equipment,
//! and accessories such as hangers (§26).
//! The IFC writer and the scene graph both build their bodies from `shape`.

use crate::scene::graph::get_point;
use crate::semantic::model::{
    ElementCurve, ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, number,
    string_or_ident,
};

use super::step_writer::{
    Frame, Profile, Revolve, Shape, Solid, add, cross, dot, length, normalize, rotate, sub,
};

/// Duct, pipe or cable tray.
pub(crate) fn is_run(et: ElementType) -> bool {
    matches!(
        et,
        ElementType::Duct | ElementType::Pipe | ElementType::CableTray
    )
}

/// Cross-section of a duct, pipe or cable tray. Local X of the run's frame
/// is across (`width`), local Y up (`height`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Section {
    Round(f64),
    Rect(f64, f64),
    /// Open-top U channel of a cable tray.
    Tray(f64, f64),
}

/// Sides of the polygon outline of a round sleeve.
const SLEEVE_SIDES: usize = 16;

/// Default gap between a run and its opening (`clearance`), per side.
pub(crate) const CLEARANCE: f64 = 0.025;

/// Wall thickness of a cable tray channel.
const TRAY_WALL: f64 = 0.003;

/// Side of the square strut channel of a trapeze hanger.
pub(crate) const STRUT: f64 = 0.041;
/// Threaded rod diameter of a trapeze hanger.
pub(crate) const ROD: f64 = 0.012;
/// How far the strut reaches past each rod.
const STRUT_OVERHANG: f64 = 0.03;

impl Section {
    /// Section from `diameter`, else `width` and `height`, keys prefixed by
    /// `prefix` (`branch_` for the branch of a tee).
    fn with(et: ElementType, p: &[ResolvedProperty], prefix: &str, default: Section) -> Self {
        let key = |k: &str| number(p, &format!("{prefix}{k}"));
        if let Some(d) = key("diameter") {
            return Section::Round(d);
        }
        let (w0, h0) = match default {
            Section::Round(d) => (d, d),
            Section::Rect(w, h) | Section::Tray(w, h) => (w, h),
        };
        match (key("width"), key("height")) {
            (None, None) if prefix.is_empty() || matches!(default, Section::Round(_)) => default,
            (w, h) if et == ElementType::CableTray => {
                Section::Tray(w.unwrap_or(w0), h.unwrap_or(h0))
            }
            (w, h) => Section::Rect(w.unwrap_or(w0), h.unwrap_or(h0)),
        }
    }

    pub(crate) fn of(et: ElementType, p: &[ResolvedProperty]) -> Self {
        let default = match et {
            ElementType::Pipe => Section::Round(0.05),
            ElementType::CableTray => Section::Tray(0.3, 0.06),
            _ => Section::Rect(0.4, 0.25),
        };
        Self::with(et, p, "", default)
    }

    /// Section of a tee's branch, by default the run's own.
    pub(crate) fn branch(et: ElementType, p: &[ResolvedProperty]) -> Self {
        Self::with(et, p, "branch_", Self::of(et, p))
    }

    pub(crate) fn profile(self) -> Profile {
        match self {
            Section::Round(d) => Profile::Circle {
                cx: 0.0,
                cy: 0.0,
                r: d / 2.0,
            },
            Section::Rect(w, d) => Profile::Rect {
                cx: 0.0,
                cy: 0.0,
                w,
                d,
            },
            Section::Tray(w, h) => {
                let (x, y, t) = (w / 2.0, h / 2.0, TRAY_WALL.min(w / 4.0).min(h / 2.0));
                Profile::Poly {
                    outer: vec![
                        (-x, -y),
                        (x, -y),
                        (x, y),
                        (x - t, y),
                        (x - t, t - y),
                        (t - x, t - y),
                        (t - x, y),
                        (-x, y),
                    ],
                    holes: vec![],
                }
            }
        }
    }

    /// Outline of the section grown by `by` on every side, counter-clockwise.
    /// A round one is a polygon inside the circle, or around it (`around`).
    fn outline(self, by: f64, around: bool) -> Vec<(f64, f64)> {
        match self {
            Section::Round(d) => (0..SLEEVE_SIDES)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / SLEEVE_SIDES as f64;
                    let grow = if around {
                        1.0 / (std::f64::consts::PI / SLEEVE_SIDES as f64).cos()
                    } else {
                        1.0
                    };
                    let r = (d / 2.0 + by) * grow;
                    (r * a.cos(), r * a.sin())
                })
                .collect(),
            Section::Rect(w, h) | Section::Tray(w, h) => {
                let (x, y) = (w / 2.0 + by, h / 2.0 + by);
                vec![(-x, -y), (x, -y), (x, y), (-x, y)]
            }
        }
    }

    /// Half the section's extent along `dir`, the section lying in the XY
    /// plane of `frame`.
    pub(crate) fn half_extent(self, frame: &Frame, dir: [f64; 3]) -> f64 {
        match self {
            Section::Round(d) => d / 2.0,
            Section::Rect(w, h) | Section::Tray(w, h) => {
                (dot(dir, frame.x).abs() * w + dot(dir, frame.y()).abs() * h) / 2.0
            }
        }
    }

    /// Default centreline bend radius: 1.5 diameters for pipes, else the
    /// larger side.
    pub(crate) fn bend_radius(self, et: ElementType) -> f64 {
        match self {
            Section::Round(d) if et == ElementType::Pipe => 1.5 * d,
            Section::Round(d) => d,
            Section::Rect(w, h) | Section::Tray(w, h) => w.max(h),
        }
    }
}

/// Kind of a run element by its properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fitting {
    Segment,
    Bend,
    Tee,
    /// Sleeve lining the opening a run passes through.
    Sleeve,
}

pub(crate) fn fitting(p: &[ResolvedProperty]) -> Fitting {
    match string_or_ident(p, "fitting") {
        Some("bend") => Fitting::Bend,
        Some("tee") => Fitting::Tee,
        Some("sleeve") => Fitting::Sleeve,
        _ => Fitting::Segment,
    }
}

/// `from` and `to` of a 3D axis, z above the storey (`z_offset` added).
pub(crate) fn axis3(p: &[ResolvedProperty]) -> Option<([f64; 3], [f64; 3])> {
    let base = number(p, "z_offset").unwrap_or(0.0);
    match p.iter().find(|q| q.key == "from").map(|q| &q.value)? {
        ResolvedValue::Axis { from, to, z, .. } => {
            let (z0, z1) = z.unwrap_or((0.0, 0.0));
            Some(([from.0, from.1, base + z0], [to.0, to.1, base + z1]))
        }
        _ => None,
    }
}

/// Points of `path`, z above the storey (`z_offset` added).
pub(crate) fn path(p: &[ResolvedProperty]) -> Option<Vec<[f64; 3]>> {
    let base = number(p, "z_offset").unwrap_or(0.0);
    match p.iter().find(|q| q.key == "path").map(|q| &q.value)? {
        ResolvedValue::Path3D(pts) => Some(pts.iter().map(|&(x, y, z)| [x, y, base + z]).collect()),
        _ => None,
    }
}

/// `branch` end of a tee, z above the storey.
pub(crate) fn branch_point(p: &[ResolvedProperty]) -> Option<[f64; 3]> {
    let base = number(p, "z_offset").unwrap_or(0.0);
    match p.iter().find(|q| q.key == "branch").map(|q| &q.value)? {
        ResolvedValue::Point3D(x, y, z) => Some([*x, *y, base + z]),
        _ => None,
    }
}

pub(crate) fn degrees(p: &[ResolvedProperty], key: &str) -> f64 {
    number(p, key).unwrap_or(0.0).to_radians()
}

/// Bend from `p0` through the corner `v` toward `p1`: the section of the
/// incoming run (turned `rotation` about it) revolved about the bend centre.
/// Its radius fits the shorter leg; the longer one keeps a straight piece
/// (legs differ where a bend meets a smooth run). `None` for a straight or
/// reversing corner.
pub(crate) fn bend(
    section: Section,
    p0: [f64; 3],
    v: [f64; 3],
    p1: [f64; 3],
    rotation: f64,
) -> Option<Shape> {
    let (t0, t1) = (length(sub(v, p0)), length(sub(p1, v)));
    let a = normalize(sub(v, p0));
    let b = normalize(sub(p1, v));
    let cos = dot(a, b).clamp(-1.0, 1.0);
    let angle = cos.acos();
    let t = t0.min(t1);
    if t < 1e-9 || !(1e-6..=std::f64::consts::PI - 1e-3).contains(&angle) {
        return None;
    }
    let radius = t / (angle / 2.0).tan();
    let start = add(v, a, -t);
    let frame = Frame::along(start, a, rotation);
    let u = normalize(add(b, a, -cos));
    let centre = frame.to_local(add(start, u, radius));
    let n = normalize(cross(a, b));
    let mut solids = Vec::new();
    if t0 - t > 1e-9 {
        solids.push(Solid::vertical(section.profile(), t - t0, t0 - t));
    }
    solids.push(Solid {
        profile: section.profile(),
        position: Frame::identity(),
        depth: 0.0,
        revolve: Some(Revolve {
            origin: (centre[0], centre[1]),
            dir: (dot(n, frame.x), dot(n, frame.y())),
            angle,
        }),
        directrix: None,
    });
    if t1 - t > 1e-9 {
        // Straight on in the section turned through the bend.
        let mut tail = Solid::vertical(section.profile(), 0.0, t1 - t);
        tail.position = Frame {
            origin: frame.to_local(add(v, b, t)),
            z: frame.dir_to_local(b),
            x: frame.dir_to_local(rotate(frame.x, n, angle)),
        };
        solids.push(tail);
    }
    Some(Shape { frame, solids })
}

/// Straight run from `p0` to `p1`.
fn straight(profile: Profile, p0: [f64; 3], p1: [f64; 3], rotation: f64) -> Option<Shape> {
    let d = sub(p1, p0);
    let len = length(d);
    (len > 1e-9).then(|| Shape {
        frame: Frame::along(p0, d, rotation),
        solids: vec![Solid::vertical(profile, 0.0, len)],
    })
}

/// Curved run along polyline `pts`: one straight piece per edge, in the
/// frame of the first.
fn chain(profile: Profile, pts: &[[f64; 3]], rotation: f64) -> Option<Shape> {
    let mut pieces = pts
        .windows(2)
        .filter_map(|w| straight(profile.clone(), w[0], w[1], rotation));
    let first = pieces.next()?;
    let f = first.frame;
    let mut solids = first.solids;
    for piece in pieces {
        solids.extend(piece.solids.into_iter().map(|mut solid| {
            solid.position = f.relative(&piece.frame);
            solid
        }));
    }
    Some(Shape { frame: f, solids })
}

/// Body of a duct, pipe, cable tray, terminal, equipment or accessory,
/// `None` when its geometry is missing or degenerate.
pub(crate) fn shape(elem: &ResolvedElement) -> Option<Shape> {
    let p = &elem.properties;
    let et = elem.element_type;
    let rotation = degrees(p, "rotation");
    if !is_run(et) {
        let (x, y) = get_point(p, "position")?;
        let base = number(p, "z_offset").unwrap_or(0.0);
        let [w, d, h] = size(elem);
        let frame = Frame::upright([x, y, base], rotation);
        if kind(elem).is_some_and(|k| k.name == "hanger") {
            return Some(Shape {
                frame,
                solids: hanger(w, h),
            });
        }
        let section = number(p, "diameter").map_or(Section::Rect(w, d), Section::Round);
        return Some(Shape {
            frame,
            solids: vec![Solid::vertical(section.profile(), 0.0, h)],
        });
    }
    let section = Section::of(et, p);
    match fitting(p) {
        Fitting::Segment => match axis3(p) {
            Some((p0, p1)) => straight(section.profile(), p0, p1, rotation),
            None => match (section, elem.curve("path")) {
                // A round run along a smooth path is one swept disk.
                // Its curve is in storey coordinates already (parts own no `z_offset`).
                (Section::Round(_), Some(ElementCurve::Path3(c))) => Some(Shape {
                    frame: Frame::identity(),
                    solids: vec![Solid::swept(section.profile(), c.clone())],
                }),
                _ => chain(section.profile(), &path(p)?, rotation),
            },
        },
        Fitting::Bend => match path(p)?[..] {
            [p0, v, p1] => bend(section, p0, v, p1, rotation),
            _ => None,
        },
        // A tube between the run (half the clearance away) and its opening;
        // the hole's polygon lies around the run so it does not cut into it.
        Fitting::Sleeve => {
            let (p0, p1) = axis3(p)?;
            let c = number(p, "clearance").unwrap_or(CLEARANCE);
            let ring = Profile::Poly {
                outer: section.outline(c, false),
                holes: vec![section.outline(c / 2.0, true)],
            };
            straight(ring, p0, p1, rotation)
        }
        Fitting::Tee => {
            let (p0, p1) = axis3(p)?;
            let e = branch_point(p)?;
            let main = straight(section.profile(), p0, p1, rotation)?;
            let a = normalize(sub(p1, p0));
            let foot = add(p0, a, dot(sub(e, p0), a));
            let stub = straight(
                Section::branch(et, p).profile(),
                foot,
                e,
                degrees(p, "branch_rotation"),
            )?;
            let mut solid = stub.solids.into_iter().next()?;
            solid.position = main.frame.relative(&stub.frame);
            let mut solids = main.solids;
            solids.push(solid);
            Some(Shape {
                frame: main.frame,
                solids,
            })
        }
    }
}

/// Offset of a trapeze hanger's rods from the run axis and half length of
/// its strut, for a run `width` wide.
pub(crate) fn hanger_span(width: f64) -> (f64, f64) {
    let x = width / 2.0 + ROD / 2.0;
    (x, x + STRUT_OVERHANG)
}

/// Trapeze hanger: a strut across local X carrying a run `width` wide on its
/// top (z = 0), and a rod on each side reaching `height` up.
fn hanger(width: f64, height: f64) -> Vec<Solid> {
    let (x, strut_half) = hanger_span(width);
    let strut = Profile::Rect {
        cx: 0.0,
        cy: 0.0,
        w: 2.0 * strut_half,
        d: STRUT,
    };
    let rod = |cx| Profile::Circle {
        cx,
        cy: 0.0,
        r: ROD / 2.0,
    };
    vec![
        Solid::vertical(strut, -STRUT, STRUT),
        Solid::vertical(rod(-x), -STRUT, height + STRUT),
        Solid::vertical(rod(x), -STRUT, height + STRUT),
    ]
}

/// Connection points of a run element, storey coordinates.
pub(crate) fn ports(elem: &ResolvedElement) -> Vec<[f64; 3]> {
    let p = &elem.properties;
    if !is_run(elem.element_type) {
        return vec![];
    }
    match fitting(p) {
        Fitting::Segment => match (axis3(p), path(p)) {
            (Some((a, b)), _) => vec![a, b],
            (None, Some(pts)) if pts.len() >= 2 => vec![pts[0], pts[pts.len() - 1]],
            _ => vec![],
        },
        Fitting::Bend => match path(p).as_deref() {
            Some(&[a, _, b]) => vec![a, b],
            _ => vec![],
        },
        Fitting::Tee => match (axis3(p), branch_point(p)) {
            (Some((a, b)), Some(e)) => vec![a, b, e],
            _ => vec![],
        },
        Fitting::Sleeve => vec![],
    }
}

/// Plan size `[width, depth, height]` of a terminal, equipment or accessory: its
/// properties, else the default of its `type`.
fn size(elem: &ResolvedElement) -> [f64; 3] {
    let p = &elem.properties;
    let [w, d, h] = kind(elem).map_or([0.5, 0.5, 0.5], |k| k.size);
    let dia = number(p, "diameter");
    [
        number(p, "width").or(dia).unwrap_or(w),
        number(p, "depth").or(dia).unwrap_or(d),
        number(p, "height").unwrap_or(h),
    ]
}

/// Storey-coordinate bounding box of a terminal, equipment or accessory body.
pub(crate) fn body_box(elem: &ResolvedElement) -> Option<([f64; 3], [f64; 3])> {
    kinds(elem.element_type)?;
    let shape = shape(elem)?;
    let [w, d, h] = size(elem);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for i in 0..8 {
        let corner = [
            if i & 1 == 0 { -w / 2.0 } else { w / 2.0 },
            if i & 2 == 0 { -d / 2.0 } else { d / 2.0 },
            if i & 4 == 0 { 0.0 } else { h },
        ];
        let q = shape.frame.to_parent(corner);
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    Some((lo, hi))
}

/// A terminal, equipment or accessory `type`: IFC class, PredefinedType and
/// default size. A USERDEFINED one writes its name as ObjectType.
pub(crate) struct Kind {
    pub(crate) name: &'static str,
    pub(crate) class: &'static str,
    pub(crate) predefined: &'static str,
    size: [f64; 3],
}

const fn k(
    name: &'static str,
    class: &'static str,
    predefined: &'static str,
    size: [f64; 3],
) -> Kind {
    Kind {
        name,
        class,
        predefined,
        size,
    }
}

static TERMINAL_KINDS: &[Kind] = &[
    k("air_supply", "IFCAIRTERMINAL", "DIFFUSER", [0.6, 0.6, 0.05]),
    k("air_extract", "IFCAIRTERMINAL", "GRILLE", [0.6, 0.6, 0.05]),
    k(
        "sprinkler",
        "IFCFIRESUPPRESSIONTERMINAL",
        "SPRINKLER",
        [0.05, 0.05, 0.06],
    ),
    k(
        "hose_reel",
        "IFCFIRESUPPRESSIONTERMINAL",
        "HOSEREEL",
        [0.7, 0.25, 0.7],
    ),
    k("wc", "IFCSANITARYTERMINAL", "TOILETPAN", [0.4, 0.55, 0.4]),
    k(
        "washbasin",
        "IFCSANITARYTERMINAL",
        "WASHHANDBASIN",
        [0.55, 0.45, 0.15],
    ),
    k("sink", "IFCSANITARYTERMINAL", "SINK", [0.8, 0.5, 0.2]),
    k("urinal", "IFCSANITARYTERMINAL", "URINAL", [0.35, 0.3, 0.6]),
    k("shower", "IFCSANITARYTERMINAL", "SHOWER", [0.9, 0.9, 0.05]),
    k("light", "IFCLIGHTFIXTURE", "POINTSOURCE", [0.6, 0.6, 0.08]),
    k("outlet", "IFCOUTLET", "POWEROUTLET", [0.08, 0.05, 0.08]),
    k("data_outlet", "IFCOUTLET", "DATAOUTLET", [0.08, 0.05, 0.08]),
    k(
        "floor_drain",
        "IFCWASTETERMINAL",
        "FLOORTRAP",
        [0.15, 0.15, 0.1],
    ),
    k("radiator", "IFCSPACEHEATER", "RADIATOR", [1.0, 0.1, 0.6]),
    k(
        "switch",
        "IFCSWITCHINGDEVICE",
        "TOGGLESWITCH",
        [0.08, 0.05, 0.08],
    ),
    k(
        "smoke_detector",
        "IFCSENSOR",
        "SMOKESENSOR",
        [0.1, 0.1, 0.05],
    ),
    k("alarm", "IFCALARM", "MANUALPULLBOX", [0.1, 0.05, 0.1]),
];

static EQUIPMENT_KINDS: &[Kind] = &[
    k(
        "air_handler",
        "IFCUNITARYEQUIPMENT",
        "AIRHANDLER",
        [4.0, 2.0, 2.0],
    ),
    k("fan", "IFCFAN", "TUBEAXIAL", [1.0, 1.0, 1.0]),
    k("pump", "IFCPUMP", "CIRCULATOR", [0.5, 0.4, 0.6]),
    k("tank", "IFCTANK", "STORAGE", [1.0, 1.0, 1.5]),
    k("boiler", "IFCBOILER", "WATER", [1.2, 0.8, 1.5]),
    k("chiller", "IFCCHILLER", "AIRCOOLED", [3.0, 1.5, 1.8]),
    k(
        "heat_exchanger",
        "IFCHEATEXCHANGER",
        "PLATE",
        [0.6, 0.4, 1.0],
    ),
    k(
        "distribution_board",
        "IFCELECTRICDISTRIBUTIONBOARD",
        "DISTRIBUTIONBOARD",
        [0.8, 0.3, 2.0],
    ),
    k("transformer", "IFCTRANSFORMER", "VOLTAGE", [2.0, 1.5, 2.0]),
    k("damper", "IFCDAMPER", "CONTROLDAMPER", [0.4, 0.3, 0.3]),
    k("fire_damper", "IFCDAMPER", "FIREDAMPER", [0.4, 0.3, 0.3]),
    k("valve", "IFCVALVE", "ISOLATING", [0.15, 0.1, 0.15]),
];

/// Default bracket size: width, depth, height.
pub(crate) const BRACKET: [f64; 3] = [0.15, 0.1, 0.15];

static ACCESSORY_KINDS: &[Kind] = &[
    k(
        "hanger",
        "IFCDISCRETEACCESSORY",
        "USERDEFINED",
        [0.3, STRUT, 0.5],
    ),
    k("bracket", "IFCDISCRETEACCESSORY", "BRACKET", BRACKET),
    k(
        "base_plate",
        "IFCDISCRETEACCESSORY",
        "ANCHORPLATE",
        [0.4, 0.4, 0.02],
    ),
    k("shoe", "IFCDISCRETEACCESSORY", "SHOE", [0.2, 0.2, 0.1]),
    k(
        "anchor_bolt",
        "IFCMECHANICALFASTENER",
        "ANCHORBOLT",
        [0.02, 0.02, 0.3],
    ),
];

/// Kinds a terminal, equipment or accessory `type` can name.
pub(crate) fn kinds(et: ElementType) -> Option<&'static [Kind]> {
    match et {
        ElementType::Terminal => Some(TERMINAL_KINDS),
        ElementType::Equipment => Some(EQUIPMENT_KINDS),
        ElementType::Accessory => Some(ACCESSORY_KINDS),
        _ => None,
    }
}

/// The `type` of a terminal, equipment or accessory.
fn kind(elem: &ResolvedElement) -> Option<&'static Kind> {
    let t = string_or_ident(&elem.properties, "type")?;
    kinds(elem.element_type)?.iter().find(|k| k.name == t)
}

/// IFC class and PredefinedType of a services element without `ifc_class:`.
pub(crate) fn class(elem: &ResolvedElement) -> Option<(&'static str, &'static str)> {
    if let Some(k) = kind(elem) {
        return Some((k.class, k.predefined));
    }
    let et = elem.element_type;
    let tray = et == ElementType::CableTray;
    let fitting = fitting(&elem.properties);
    let class = |segment, other| match fitting {
        Fitting::Segment | Fitting::Sleeve => segment,
        _ => other,
    };
    let predefined = match fitting {
        Fitting::Segment if tray => "CABLETRAYSEGMENT",
        Fitting::Segment => "RIGIDSEGMENT",
        Fitting::Bend => "BEND",
        Fitting::Tee if tray => "TEE",
        Fitting::Tee => "JUNCTION",
        Fitting::Sleeve => "NOTDEFINED",
    };
    Some(match et {
        ElementType::Duct => (class("IFCDUCTSEGMENT", "IFCDUCTFITTING"), predefined),
        ElementType::Pipe => (class("IFCPIPESEGMENT", "IFCPIPEFITTING"), predefined),
        ElementType::CableTray => (
            class("IFCCABLECARRIERSEGMENT", "IFCCABLECARRIERFITTING"),
            predefined,
        ),
        _ => return None,
    })
}

/// ObjectType of a services element whose `type` is USERDEFINED in IFC.
pub(crate) fn object_type(elem: &ResolvedElement) -> Option<String> {
    kind(elem)
        .filter(|k| k.predefined == "USERDEFINED")
        .map(|k| k.name.to_ascii_uppercase())
}

/// IfcDistributionPortTypeEnum value of a run element's ports.
pub(crate) fn port_type(et: ElementType) -> &'static str {
    match et {
        ElementType::Duct => "DUCT",
        ElementType::Pipe => "PIPE",
        ElementType::CableTray => "CABLECARRIER",
        _ => "NOTDEFINED",
    }
}
