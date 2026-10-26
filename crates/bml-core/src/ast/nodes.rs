use super::span::Span;

/// Root node of a parsed BuildingML file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceFile {
    pub items: Vec<Item>,
    pub span: Span,
}

/// Top-level items in a source file.
#[derive(Debug, Clone, serde::Serialize)]
pub enum Item {
    Heading(Heading),
    Element(ElementDeclaration),
    Import(Import),
    Material(MaterialDefinition),
    Template(TemplateDefinition),
    AssetLibrary(AssetLibraryDefinition),
    CheckSuite(CheckSuiteDeclaration),
    CheckDecl(CheckDeclaration),
    Comment(Comment),
}

/// Spatial hierarchy heading (# Site, ## Building, ### Storey, #### Space).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Heading {
    /// Number of `#` (1=Site, 2=Building, 3=Storey, 4=Space)
    pub level: u8,
    /// The spatial type keyword (Site, Building, Storey, Space)
    pub spatial_type: String,
    /// The name after the colon
    pub name: String,
    /// Indented body items (properties, child elements)
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// Items that appear in indented body blocks.
#[derive(Debug, Clone, serde::Serialize)]
pub enum BodyItem {
    Property(Property),
    Element(ElementDeclaration),
    CheckDecl(CheckDeclaration),
    Comment(Comment),
}

/// Element declaration (e.g., Wall: W-GF-01).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ElementDeclaration {
    pub element_type: String,
    pub id: String,
    /// Indented body (properties, nested child elements like Window inside Wall)
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// Material definition (Material: material_id).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MaterialDefinition {
    pub id: String,
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// Template definition for floor templates.
///
/// A template defines a reusable set of elements and spaces that can be
/// applied to multiple storeys via `apply_template:`.
/// The body contains elements at the indented level; space headings that
/// follow the template are collected separately.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TemplateDefinition {
    pub id: String,
    /// Elements and properties directly in the template body
    pub body: Vec<BodyItem>,
    /// Space headings that belong to this template (collected during parsing)
    pub spaces: Vec<Heading>,
    pub span: Span,
}

/// Asset library definition for furniture asset collections.
///
/// An asset library contains reusable asset definitions that can be placed
/// in buildings via `Furniture:` elements with `asset:` references.
/// Uses heading hierarchy: `# AssetLibrary:`, `## Asset:`, `### Part:`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetLibraryDefinition {
    pub name: String,
    pub properties: Vec<Property>,
    pub assets: Vec<AssetDefinition>,
    pub span: Span,
}

/// A single asset definition within an asset library.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetDefinition {
    pub id: String,
    pub properties: Vec<Property>,
    pub parts: Vec<AssetPartDefinition>,
    pub span: Span,
}

/// A geometry part within an asset.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetPartDefinition {
    pub name: String,
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// CheckSuite declaration (e.g., CheckSuite: Fire Safety — German Building Code).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckSuiteDeclaration {
    pub name: String,
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// Check declaration (e.g., Check: minimum_door_width).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckDeclaration {
    pub id: String,
    pub body: Vec<BodyItem>,
    pub span: Span,
}

/// Key-value property.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Property {
    pub key: String,
    pub value: Value,
    pub span: Span,
}

/// One item of a point list with curves. Coordinates stay raw (`[x, y]` or
/// `[x, y, z]`); defaults and validation happen at resolution.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum CurveItem {
    /// Plain point: a sharp corner
    Corner(Vec<f64>),
    /// `smooth` point: the spline passes through it
    Smooth(Vec<f64>),
    /// `nurbs { ... }` segment from its first to its last control point
    Nurbs {
        degree: f64,
        points: Vec<Vec<f64>>,
        weights: Option<Vec<f64>>,
        knots: Option<Vec<f64>>,
    },
}

/// Property value types.
#[derive(Debug, Clone, serde::Serialize)]
pub enum Value {
    /// Plain number (e.g., 2025)
    Number(f64),
    /// Number with unit (e.g., 2.80 m)
    NumberWithUnit(f64, String),
    /// Comma-separated number pair (e.g., 49.4521, 12.1803)
    NumberPair(f64, f64),
    /// Quoted string
    String(String),
    /// Boolean (true/false)
    Boolean(bool),
    /// Identifier (e.g., material name, enum value)
    Identifier(String),
    /// 2D point (x, y)
    Point(f64, f64),
    /// 3D point (x, y, z) — used for asset part positions
    Point3D(f64, f64, f64),
    /// Polygon as list of points
    Polygon(Vec<(f64, f64)>),
    /// 3D polyline [(x, y, z), ...], e.g. the route of a duct
    Path3D(Vec<(f64, f64, f64)>),
    /// Polygon with holes: outer boundary + list of inner hole polygons
    PolygonWithHoles {
        outer: Vec<(f64, f64)>,
        holes: Vec<Vec<(f64, f64)>>,
    },
    /// Point list with `smooth` points or `nurbs` segments; `dim` is 2 or 3
    CurvePath { items: Vec<CurveItem>, dim: u8 },
    /// Polygon with holes where some ring has `smooth` points or `nurbs` segments
    CurveRegion {
        outer: Vec<CurveItem>,
        holes: Vec<Vec<CurveItem>>,
    },
    /// Profile (e.g., rectangle(0.3, 0.3))
    Profile {
        profile_type: String,
        params: Vec<f64>,
    },
    /// Axis value: from point to point, optionally via a midpoint (arc)
    Axis {
        from: (f64, f64),
        to: (f64, f64),
        via: Option<(f64, f64)>,
        /// Endpoint heights above the storey (from, to) when written as `(x, y, z)`
        z: Option<(f64, f64)>,
    },
    /// Enum list: [val1, val2, val3]
    EnumList(Vec<String>),
    /// Number list: [1.1 m, 3.4 m] (`Number` / `NumberWithUnit` items)
    NumberList(Vec<Value>),
    /// Dotted path access (e.g., host.space.usage)
    PathAccess(Vec<String>),
    /// Indented `- key: value` items (e.g., the layers of a material)
    List(Vec<Property>),
    /// Comparison expression (e.g., width >= 0.90 m)
    ComparisonExpr {
        left: Box<Value>,
        op: ComparisonOp,
        right: Box<Value>,
    },
    /// In-expression (e.g., usage in [living, bedroom])
    InExpr {
        left: Box<Value>,
        values: Vec<String>,
    },
    /// Function call (e.g., count(), sum(area))
    FunctionCall { name: String, args: Vec<Value> },
    /// Geometry primitive for asset parts (e.g., box(1.6, 0.8, 0.03), cylinder(0.02, 0.45))
    GeometryPrimitive {
        geometry_type: String,
        args: Vec<Value>,
    },
}

/// Comparison operators for check expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ComparisonOp {
    Ge, // >=
    Le, // <=
    Eq, // ==
    Ne, // !=
    Gt, // >
    Lt, // <
}

/// Import statement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Import {
    pub name: String,
    pub path: String,
    pub span: Span,
}

/// Comment (line or doc).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Comment {
    pub text: String,
    pub is_doc: bool,
    pub span: Span,
}
