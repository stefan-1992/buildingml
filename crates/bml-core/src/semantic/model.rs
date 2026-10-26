use std::collections::BTreeMap;

use crate::ast::span::Span;
use crate::geometry::curve::{Curve2, Curve3};

/// Fully resolved semantic model of a BuildingML project.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticModel {
    pub sites: Vec<ResolvedSite>,
    pub materials: Vec<ResolvedMaterial>,
    pub asset_libraries: Vec<ResolvedAssetLibrary>,
}

impl SemanticModel {
    /// The material definition with id `id`.
    pub fn material(&self, id: &str) -> Option<&ResolvedMaterial> {
        self.materials.iter().find(|m| m.id == id)
    }

    /// Every storey, building and site with its own spaces and elements:
    /// storeys first, then their building, then the site.
    pub fn containers(&self) -> impl Iterator<Item = Container<'_>> {
        self.sites.iter().flat_map(|site| {
            let buildings = site.buildings.iter().flat_map(move |b| {
                let storeys = b.storeys.iter().map(move |s| Container {
                    site,
                    building: Some(b),
                    storey: Some(s),
                    spaces: &s.spaces,
                    elements: &s.elements,
                });
                storeys.chain(std::iter::once(Container {
                    site,
                    building: Some(b),
                    storey: None,
                    spaces: &b.spaces,
                    elements: &b.elements,
                }))
            });
            buildings.chain(std::iter::once(Container {
                site,
                building: None,
                storey: None,
                spaces: &site.spaces,
                elements: &site.elements,
            }))
        })
    }

    /// Mutable [`SemanticModel::containers`] as (elevation, spaces, elements).
    pub fn containers_mut(
        &mut self,
    ) -> impl Iterator<Item = (f64, &mut Vec<ResolvedSpace>, &mut Vec<ResolvedElement>)> {
        self.sites.iter_mut().flat_map(|site| {
            let buildings = site.buildings.iter_mut().flat_map(|b| {
                let storeys = b.storeys.iter_mut().map(|s| {
                    let elevation = number(&s.properties, "elevation").unwrap_or(0.0);
                    (elevation, &mut s.spaces, &mut s.elements)
                });
                storeys.chain(std::iter::once((0.0, &mut b.spaces, &mut b.elements)))
            });
            buildings.chain(std::iter::once((0.0, &mut site.spaces, &mut site.elements)))
        })
    }

    /// Every element of a storey, building or site, directly or in a space
    /// (children not included), in [`SemanticModel::containers`] order.
    pub fn elements(&self) -> impl Iterator<Item = &ResolvedElement> {
        self.containers()
            .flat_map(|c| contents(c.spaces, c.elements))
    }

    /// Every space of a storey, building or site.
    pub fn spaces(&self) -> impl Iterator<Item = &ResolvedSpace> {
        self.containers().flat_map(|c| c.spaces)
    }

    /// Mutable [`SemanticModel::elements`].
    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut ResolvedElement> {
        self.containers_mut()
            .flat_map(|(_, spaces, elements)| contents_mut(spaces, elements))
    }

    /// Tag every span with `file_id`, so diagnostics raised on the merged
    /// model of a project point to the file the item came from.
    pub fn set_file_id(&mut self, file_id: u32) {
        fn props(properties: &mut [ResolvedProperty], file_id: u32) {
            for p in properties {
                p.span.file_id = file_id;
            }
        }
        fn elements(list: &mut [ResolvedElement], file_id: u32) {
            for e in list {
                e.span.file_id = file_id;
                props(&mut e.properties, file_id);
                elements(&mut e.children, file_id);
            }
        }
        fn spaces(list: &mut [ResolvedSpace], file_id: u32) {
            for s in list {
                s.span.file_id = file_id;
                props(&mut s.properties, file_id);
                elements(&mut s.elements, file_id);
            }
        }
        for site in &mut self.sites {
            site.span.file_id = file_id;
            props(&mut site.properties, file_id);
            spaces(&mut site.spaces, file_id);
            elements(&mut site.elements, file_id);
            for b in &mut site.buildings {
                b.span.file_id = file_id;
                props(&mut b.properties, file_id);
                spaces(&mut b.spaces, file_id);
                elements(&mut b.elements, file_id);
                for s in &mut b.storeys {
                    s.span.file_id = file_id;
                    props(&mut s.properties, file_id);
                    spaces(&mut s.spaces, file_id);
                    elements(&mut s.elements, file_id);
                }
            }
        }
        for m in &mut self.materials {
            m.span.file_id = file_id;
            props(&mut m.properties, file_id);
        }
        for lib in &mut self.asset_libraries {
            lib.span.file_id = file_id;
            props(&mut lib.properties, file_id);
            for a in &mut lib.assets {
                a.span.file_id = file_id;
                props(&mut a.properties, file_id);
                for part in &mut a.parts {
                    part.span.file_id = file_id;
                }
            }
        }
    }
}

/// A storey, building or site with the spaces and elements it holds directly.
pub struct Container<'a> {
    pub site: &'a ResolvedSite,
    pub building: Option<&'a ResolvedBuilding>,
    pub storey: Option<&'a ResolvedStorey>,
    pub spaces: &'a [ResolvedSpace],
    pub elements: &'a [ResolvedElement],
}

impl Container<'_> {
    /// Storey elevation; 0 for a building or site.
    pub fn elevation(&self) -> f64 {
        self.storey
            .and_then(|s| number(&s.properties, "elevation"))
            .unwrap_or(0.0)
    }

    /// Storey height; 3 m for a building or site or when unset.
    pub fn height(&self) -> f64 {
        self.storey
            .and_then(|s| number(&s.properties, "height"))
            .unwrap_or(3.0)
    }
}

/// The value of a numeric property.
pub(crate) fn number(properties: &[ResolvedProperty], key: &str) -> Option<f64> {
    match properties.iter().find(|p| p.key == key)?.value {
        ResolvedValue::Number(n) => Some(n),
        _ => None,
    }
}

/// The value of a boolean property.
pub(crate) fn boolean(properties: &[ResolvedProperty], key: &str) -> Option<bool> {
    match properties.iter().find(|p| p.key == key)?.value {
        ResolvedValue::Boolean(b) => Some(b),
        _ => None,
    }
}

/// The value of a string or identifier property.
pub(crate) fn string_or_ident<'a>(
    properties: &'a [ResolvedProperty],
    key: &str,
) -> Option<&'a str> {
    match &properties.iter().find(|p| p.key == key)?.value {
        ResolvedValue::String(s) | ResolvedValue::Identifier(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Element ids of a single-id or list property.
pub(crate) fn refs<'a>(properties: &'a [ResolvedProperty], key: &str) -> Vec<&'a str> {
    match properties.iter().find(|p| p.key == key).map(|p| &p.value) {
        Some(ResolvedValue::Identifier(id)) => vec![id.as_str()],
        Some(ResolvedValue::EnumList(ids)) => ids.iter().map(String::as_str).collect(),
        _ => vec![],
    }
}

/// Elements of a container, then those of its spaces.
pub fn contents<'a>(
    spaces: &'a [ResolvedSpace],
    elements: &'a [ResolvedElement],
) -> impl Iterator<Item = &'a ResolvedElement> {
    elements
        .iter()
        .chain(spaces.iter().flat_map(|s| &s.elements))
}

/// Mutable [`contents`].
pub fn contents_mut<'a>(
    spaces: &'a mut [ResolvedSpace],
    elements: &'a mut [ResolvedElement],
) -> impl Iterator<Item = &'a mut ResolvedElement> {
    elements
        .iter_mut()
        .chain(spaces.iter_mut().flat_map(|s| &mut s.elements))
}

/// A resolved site (# Site).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedSite {
    pub name: String,
    /// Created for a building without a `# Site` heading (no heading of its own).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub implicit: bool,
    pub properties: Vec<ResolvedProperty>,
    pub buildings: Vec<ResolvedBuilding>,
    /// Spaces and elements of the site itself, outside any building.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<ResolvedSpace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<ResolvedElement>,
    pub span: Span,
}

/// A resolved building (## Building).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedBuilding {
    pub name: String,
    pub properties: Vec<ResolvedProperty>,
    pub storeys: Vec<ResolvedStorey>,
    /// Spaces and elements of the building itself, outside any storey.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<ResolvedSpace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<ResolvedElement>,
    pub span: Span,
}

/// A resolved storey (### Storey).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedStorey {
    pub name: String,
    pub properties: Vec<ResolvedProperty>,
    pub spaces: Vec<ResolvedSpace>,
    pub elements: Vec<ResolvedElement>,
    pub span: Span,
}

/// A resolved space (#### Space).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedSpace {
    pub name: String,
    pub properties: Vec<ResolvedProperty>,
    pub elements: Vec<ResolvedElement>,
    pub span: Span,
    /// Exact curves of curved properties, as on [`ResolvedElement`]
    #[serde(skip)]
    pub curves: BTreeMap<String, ElementCurve>,
}

impl ResolvedSpace {
    /// Exact curve of property `key`, if it was written with `smooth` or `nurbs`.
    pub fn curve(&self, key: &str) -> Option<&ElementCurve> {
        self.curves.get(key)
    }
}

/// A resolved element with its children (e.g., Wall containing Window).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedElement {
    pub element_type: ElementType,
    pub id: String,
    pub properties: Vec<ResolvedProperty>,
    pub children: Vec<ResolvedElement>,
    pub span: Span,
    /// Exact curves of curved properties, keyed by property name. The property
    /// itself holds the tessellated polyline. Serialized as the list of curved
    /// keys (`curved`), so the 2D editor knows not to rewrite them.
    #[serde(
        rename = "curved",
        skip_serializing_if = "BTreeMap::is_empty",
        serialize_with = "serialize_keys"
    )]
    pub curves: BTreeMap<String, ElementCurve>,
}

fn serialize_keys<S: serde::Serializer, V>(
    map: &BTreeMap<String, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(map.keys())
}

/// Exact curve of a `smooth` / `nurbs` point list.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementCurve {
    /// Open 2D path
    Path2(Curve2),
    /// Open 3D route
    Path3(Curve3),
    /// Closed outline with closed holes
    Region { outer: Curve2, holes: Vec<Curve2> },
}

impl ResolvedElement {
    /// Exact curve of property `key`, if it was written with `smooth` or `nurbs`.
    pub fn curve(&self, key: &str) -> Option<&ElementCurve> {
        self.curves.get(key)
    }
}

/// Known element types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ElementType {
    Wall,
    Slab,
    Column,
    Beam,
    Window,
    Door,
    Roof,
    Stair,
    Railing,
    Ramp,
    Furniture,
    Elevator,
    Proxy,
    Member,
    Plate,
    Opening,
    Grid,
    Covering,
    Footing,
    Duct,
    Pipe,
    CableTray,
    Terminal,
    Equipment,
    Accessory,
    System,
}

impl ElementType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Wall" => Some(Self::Wall),
            "Slab" => Some(Self::Slab),
            "Column" => Some(Self::Column),
            "Beam" => Some(Self::Beam),
            "Window" => Some(Self::Window),
            "Door" => Some(Self::Door),
            "Roof" => Some(Self::Roof),
            "Stair" => Some(Self::Stair),
            "Railing" => Some(Self::Railing),
            "Ramp" => Some(Self::Ramp),
            "Furniture" => Some(Self::Furniture),
            "Elevator" => Some(Self::Elevator),
            "Proxy" => Some(Self::Proxy),
            "Member" => Some(Self::Member),
            "Plate" => Some(Self::Plate),
            "Opening" => Some(Self::Opening),
            "Grid" => Some(Self::Grid),
            "Covering" => Some(Self::Covering),
            "Footing" => Some(Self::Footing),
            "Duct" => Some(Self::Duct),
            "Pipe" => Some(Self::Pipe),
            "CableTray" => Some(Self::CableTray),
            "Terminal" => Some(Self::Terminal),
            "Equipment" => Some(Self::Equipment),
            "Accessory" => Some(Self::Accessory),
            "System" => Some(Self::System),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wall => "Wall",
            Self::Slab => "Slab",
            Self::Column => "Column",
            Self::Beam => "Beam",
            Self::Window => "Window",
            Self::Door => "Door",
            Self::Roof => "Roof",
            Self::Stair => "Stair",
            Self::Railing => "Railing",
            Self::Ramp => "Ramp",
            Self::Furniture => "Furniture",
            Self::Elevator => "Elevator",
            Self::Proxy => "Proxy",
            Self::Member => "Member",
            Self::Plate => "Plate",
            Self::Opening => "Opening",
            Self::Grid => "Grid",
            Self::Covering => "Covering",
            Self::Footing => "Footing",
            Self::Duct => "Duct",
            Self::Pipe => "Pipe",
            Self::CableTray => "CableTray",
            Self::Terminal => "Terminal",
            Self::Equipment => "Equipment",
            Self::Accessory => "Accessory",
            Self::System => "System",
        }
    }

    /// Return all known element type names.
    pub fn all_names() -> Vec<&'static str> {
        vec![
            "Wall",
            "Slab",
            "Column",
            "Beam",
            "Window",
            "Door",
            "Roof",
            "Stair",
            "Railing",
            "Ramp",
            "Furniture",
            "Elevator",
            "Proxy",
            "Member",
            "Plate",
            "Opening",
            "Grid",
            "Covering",
            "Footing",
            "Duct",
            "Pipe",
            "CableTray",
            "Terminal",
            "Equipment",
            "Accessory",
            "System",
        ]
    }
}

/// A resolved property with unit-normalized value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedProperty {
    pub key: String,
    pub value: ResolvedValue,
    pub span: Span,
}

/// A resolved value — units normalized to SI.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ResolvedValue {
    /// Number in SI units (meters, m2, etc.)
    Number(f64),
    /// Quoted string
    String(String),
    /// Boolean
    Boolean(bool),
    /// Reference identifier (material, storey, etc.)
    Identifier(String),
    /// 2D point in meters
    Point(f64, f64),
    /// Polygon (list of points in meters)
    Polygon(Vec<(f64, f64)>),
    /// 3D polyline in meters, z above the storey
    Path3D(Vec<(f64, f64, f64)>),
    /// Polygon with interior holes
    PolygonWithHoles {
        outer: Vec<(f64, f64)>,
        holes: Vec<Vec<(f64, f64)>>,
    },
    /// Profile specification
    Profile {
        profile_type: String,
        params: Vec<f64>,
    },
    /// Axis (from/to points in meters, optionally via a midpoint for arcs)
    Axis {
        from: (f64, f64),
        to: (f64, f64),
        via: Option<(f64, f64)>,
        /// Endpoint heights above the storey (from, to) when written as `(x, y, z)`
        #[serde(skip_serializing_if = "Option::is_none")]
        z: Option<(f64, f64)>,
    },
    /// Enum list
    EnumList(Vec<String>),
    /// Number list in SI units
    NumberList(Vec<f64>),
    /// 3D point (x, y, z) for asset part positions
    Point3D(f64, f64, f64),
    /// Geometry primitive for asset parts
    GeometryPrimitive(GeometryPrimitive),
    /// Indented `- key: value` items (e.g., the layers of a material)
    List(Vec<ResolvedProperty>),
}

/// A resolved material definition — either layered or homogeneous.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedMaterial {
    pub id: String,
    pub material_type: MaterialType,
    pub properties: Vec<ResolvedProperty>,
    pub span: Span,
}

/// The type of material definition.
#[derive(Debug, Clone, serde::Serialize)]
pub enum MaterialType {
    /// A layered material (e.g., wall assemblies with multiple layers).
    Layered { layers: Vec<MaterialLayer> },
    /// A homogeneous material (e.g., concrete, steel).
    Homogeneous {
        category: Option<String>,
        grade: Option<String>,
        density: Option<f64>,
    },
}

/// A single layer in a layered material: a material id and its thickness in metres.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MaterialLayer {
    pub name: String,
    pub thickness: f64,
}

impl ResolvedMaterial {
    /// The layers of a layered material, `None` for a homogeneous one or a
    /// layered one without layers.
    pub fn layers(&self) -> Option<&[MaterialLayer]> {
        match &self.material_type {
            MaterialType::Layered { layers } if !layers.is_empty() => Some(layers),
            _ => None,
        }
    }

    /// `ifc_name` of the material, else its id.
    pub fn ifc_name(&self) -> &str {
        self.properties
            .iter()
            .find_map(|p| match &p.value {
                ResolvedValue::String(s) if p.key == "ifc_name" => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or(&self.id)
    }
}

/// A resolved asset library — collection of reusable furniture asset definitions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedAssetLibrary {
    pub name: String,
    pub properties: Vec<ResolvedProperty>,
    pub assets: Vec<ResolvedAsset>,
    pub span: Span,
}

/// A resolved asset — reusable multi-part geometry assembly.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedAsset {
    pub id: String,
    pub properties: Vec<ResolvedProperty>,
    pub parts: Vec<ResolvedAssetPart>,
    pub span: Span,
}

/// A resolved asset part — geometry primitive with position and material.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedAssetPart {
    pub name: String,
    pub geometry: Option<GeometryPrimitive>,
    pub position: (f64, f64, f64),
    pub rotation: f64,
    pub material: Option<String>,
    pub span: Span,
}

/// Geometry primitive for asset parts.
#[derive(Debug, Clone, serde::Serialize)]
pub enum GeometryPrimitive {
    /// Rectangular cuboid (width, depth, height)
    Box(f64, f64, f64),
    /// Cylinder (radius, height), tessellated to N segments
    Cylinder(f64, f64),
    /// Arbitrary polygon extruded to height
    Extrude(Vec<(f64, f64)>, f64),
    /// External 3D model (path to glTF/GLB file)
    Import(String),
}
