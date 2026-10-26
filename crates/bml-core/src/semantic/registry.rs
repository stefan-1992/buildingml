//! Element Type Registry — centralized metadata for all BML element types.
//!
//! This module provides `ElementTypeDescriptor` structs for each element type,
//! consolidating scattered match arms from type_check.rs, containment.rs,
//! graph.rs, floorplan.rs, hover.rs, and completion.rs into a single source
//! of truth.
//!
//! # Adding a new element type
//!
//! 1. Add a variant to `ElementType` in `model.rs`
//! 2. Add an entry to `ELEMENT_REGISTRY` in this file
//! 3. Add the keyword to `grammar.js` and run `npx tree-sitter generate`
//!
//! The registry provides:
//! - Auto-ID prefix (e.g., "W" for Wall)
//! - Property schemas (required/optional with expected types)
//! - Scene graph color (RGBA)
//! - SVG CSS class name
//! - IFC entity name
//! - LSP hover documentation
//! - LSP completion snippet

use super::model::ElementType;
use super::type_check::ExpectedType;

/// Describes a single property's schema within an element type.
#[derive(Debug, Clone)]
pub struct PropertyDescriptor {
    pub name: &'static str,
    pub required: bool,
    pub expected_type: &'static ExpectedType,
    /// Human-readable description for LSP completions.
    pub description: &'static str,
}

/// Complete descriptor for one BML element type.
#[derive(Debug)]
pub struct ElementTypeDescriptor {
    /// Element type enum variant.
    pub element_type: ElementType,
    /// Display name (e.g., "Wall").
    pub name: &'static str,
    /// IFC entity name (e.g., "IfcWall").
    pub ifc_entity: &'static str,
    /// Auto-ID prefix (e.g., "W" for Wall → W-GF-001).
    pub auto_id_prefix: &'static str,

    /// Property schemas for type checking.
    pub properties: &'static [PropertyDescriptor],

    /// LSP hover documentation (Markdown).
    pub hover_text: &'static str,

    /// LSP completion snippet with tab-stop placeholders.
    pub completion_snippet: &'static str,

    /// Scene graph RGBA color.
    pub scene_color: [f32; 4],

    /// SVG CSS class name for floorplan rendering.
    pub svg_css_class: &'static str,
}

impl ElementTypeDescriptor {
    /// Get required property names.
    pub fn required_properties(&self) -> Vec<&'static str> {
        self.properties
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name)
            .collect()
    }

    /// Get all known property names, including `COMMON_PROPERTIES` not shadowed by the type.
    pub fn known_property_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.properties.iter().map(|p| p.name).collect();
        for p in COMMON_PROPERTIES {
            if !names.contains(&p.name) {
                names.push(p.name);
            }
        }
        names
    }

    /// Whether the type's own geometry is a `boundary` polygon (slab, plate, roof).
    /// On other types a `boundary` makes the body an explicit vertical prism.
    pub fn has_native_boundary(&self) -> bool {
        self.properties.iter().any(|p| p.name == "boundary")
    }

    /// Look up a property schema by name, falling back to `COMMON_PROPERTIES`.
    pub fn get_property(&self, name: &str) -> Option<&PropertyDescriptor> {
        self.properties
            .iter()
            .chain(COMMON_PROPERTIES)
            .find(|p| p.name == name)
    }
}

// ---- Static ExpectedType instances for use in const/static arrays ----

static LENGTH: ExpectedType = ExpectedType::Length;
static THERMAL: ExpectedType = ExpectedType::Thermal;
static ACOUSTIC: ExpectedType = ExpectedType::Acoustic;
static BARE_NUMBER: ExpectedType = ExpectedType::BareNumber;
static BOOLEAN: ExpectedType = ExpectedType::Boolean;
static MATERIAL_REF: ExpectedType = ExpectedType::MaterialRef;
static STOREY_REF: ExpectedType = ExpectedType::StoreyRef;
static IDENTIFIER: ExpectedType = ExpectedType::Identifier;
static AXIS: ExpectedType = ExpectedType::Axis;
static POLYGON: ExpectedType = ExpectedType::Polygon;
static POLYGON_WITH_HOLES: ExpectedType = ExpectedType::PolygonWithHoles;
static POLYGON_OR_HOLES_OR_AUTO: ExpectedType = ExpectedType::PolygonOrHolesOrAuto;
static PROFILE: ExpectedType = ExpectedType::Profile;
static POINT: ExpectedType = ExpectedType::Point;
static STRING: ExpectedType = ExpectedType::String;
static ELEMENT_REFS: ExpectedType = ExpectedType::ElementRefs;
static LENGTHS: ExpectedType = ExpectedType::Lengths;
static POINT3D: ExpectedType = ExpectedType::Point3D;
static PATH3D: ExpectedType = ExpectedType::Path3D;
static FITTINGS: ExpectedType = ExpectedType::Enum(&["bend", "tee", "sleeve"]);

// Enum sets for property validation
static WINDOW_TYPES: ExpectedType = ExpectedType::Enum(&[
    "single_glazed",
    "double_glazed",
    "triple_glazed",
    "fixed",
    "casement",
    "sliding",
]);
static DOOR_TYPES: ExpectedType = ExpectedType::Enum(&[
    "single_leaf",
    "double_leaf",
    "sliding",
    "revolving",
    "folding",
]);
static STAIR_TYPES: ExpectedType =
    ExpectedType::Enum(&["straight", "l_shaped", "u_shaped", "spiral"]);
static STAIR_TURNS: ExpectedType = ExpectedType::Enum(&["left", "right"]);
static ROOF_TYPES: ExpectedType = ExpectedType::Enum(&["gable", "hip", "flat", "mansard", "shed"]);
static SLAB_TYPES: ExpectedType = ExpectedType::Enum(&["floor", "roof", "landing"]);
/// EN 13501-2 classes (R load-bearing, EI separating, REI both) and DIN 4102 door classes (T).
static FIRE_RATINGS: ExpectedType = ExpectedType::EnumOrString(&[
    "R30", "R60", "R90", "R120", "EI30", "EI60", "EI90", "EI120", "REI30", "REI60", "REI90",
    "REI120", "T30", "T60", "T90",
]);
static ELEVATOR_TYPES: ExpectedType = ExpectedType::Enum(&["passenger", "freight", "service"]);
static DIRECTION: ExpectedType = ExpectedType::Direction;
static REPRESENTATION: ExpectedType = ExpectedType::Enum(&["none"]);

// ---------------------------------------------------------------------------
// Property arrays per element type
// ---------------------------------------------------------------------------

/// Duct, pipe and cable tray (`Specs_V0_3.md` §24).
static RUN_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "from",
        required: false,
        expected_type: &AXIS,
        description: "Straight segment from (x, y, z) to (x, y, z), z above the storey",
    },
    PropertyDescriptor {
        name: "path",
        required: false,
        expected_type: &PATH3D,
        description: "Route [(x, y, z), ...] expanded into segments and bends, or the three points of a bend",
    },
    PropertyDescriptor {
        name: "diameter",
        required: false,
        expected_type: &LENGTH,
        description: "Round section diameter",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Rectangular section width (across, horizontal)",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Rectangular section height",
    },
    PropertyDescriptor {
        name: "fitting",
        required: false,
        expected_type: &FITTINGS,
        description: "`bend` (`path` of three points), `tee` (`from`/`to` main run plus `branch`) or `sleeve` (lines an opening)",
    },
    PropertyDescriptor {
        name: "openings",
        required: false,
        expected_type: &BOOLEAN,
        description: "Cut openings where a straight run passes through walls and slabs (default true)",
    },
    PropertyDescriptor {
        name: "clearance",
        required: false,
        expected_type: &LENGTH,
        description: "Gap between the run and its openings, per side (default 0.025 m)",
    },
    PropertyDescriptor {
        name: "sleeve",
        required: false,
        expected_type: &BOOLEAN,
        description: "Line each opening of the run with a sleeve",
    },
    PropertyDescriptor {
        name: "hangers",
        required: false,
        expected_type: &LENGTH,
        description: "Hang the horizontal segments of the run from the slab above, one trapeze hanger per this spacing",
    },
    PropertyDescriptor {
        name: "bend_radius",
        required: false,
        expected_type: &LENGTH,
        description: "Centreline radius of the bends of a route (default 1.5 diameters for pipes, else the larger side)",
    },
    PropertyDescriptor {
        name: "branch",
        required: false,
        expected_type: &POINT3D,
        description: "End of a tee's branch",
    },
    PropertyDescriptor {
        name: "branch_diameter",
        required: false,
        expected_type: &LENGTH,
        description: "Round section of a tee's branch",
    },
    PropertyDescriptor {
        name: "branch_width",
        required: false,
        expected_type: &LENGTH,
        description: "Rectangular width of a tee's branch",
    },
    PropertyDescriptor {
        name: "branch_height",
        required: false,
        expected_type: &LENGTH,
        description: "Rectangular height of a tee's branch",
    },
    PropertyDescriptor {
        name: "branch_rotation",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Rotation of a tee's branch section about its axis, degrees",
    },
    PropertyDescriptor {
        name: "system",
        required: false,
        expected_type: &IDENTIFIER,
        description: "Distribution system the element belongs to (a `System:` id)",
    },
];

/// Terminal and equipment (`Specs_V0_3.md` §24).
static TERMINAL_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &IDENTIFIER,
        description: "Kind of terminal, equipment or accessory (sets IFC class and default size)",
    },
    PropertyDescriptor {
        name: "position",
        required: true,
        expected_type: &POINT,
        description: "Plan position of the body centre",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Body width (local x)",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Body depth (local y)",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Body height, up from `z_offset`",
    },
    PropertyDescriptor {
        name: "diameter",
        required: false,
        expected_type: &LENGTH,
        description: "Round body diameter",
    },
    PropertyDescriptor {
        name: "system",
        required: false,
        expected_type: &IDENTIFIER,
        description: "Distribution system the element belongs to (a `System:` id)",
    },
];

/// Properties every element type accepts. Most of them carry IFC data through a
/// decompile -> compile round-trip; `boundary` + `height` + `z_offset` describe an
/// explicit prism body for shapes the parametric properties cannot express.
pub static COMMON_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "opening",
        required: false,
        expected_type: &IDENTIFIER,
        description: "Opening the element fills (IfcRelFillsElement)",
    },
    PropertyDescriptor {
        name: "guid",
        required: false,
        expected_type: &STRING,
        description: "IFC GlobalId, written back on compile",
    },
    PropertyDescriptor {
        name: "ifc_name",
        required: false,
        expected_type: &STRING,
        description: "Original IFC entity name (decompiler metadata)",
    },
    PropertyDescriptor {
        name: "ifc_class",
        required: false,
        expected_type: &STRING,
        description: "IFC class when it differs from the default (e.g. IfcFurniture)",
    },
    PropertyDescriptor {
        name: "predefined_type",
        required: false,
        expected_type: &IDENTIFIER,
        description: "IFC PredefinedType of the class, in snake_case (e.g. shear, partitioning, ceiling)",
    },
    PropertyDescriptor {
        name: "z_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Base height above the storey elevation",
    },
    PropertyDescriptor {
        name: "boundary",
        required: false,
        expected_type: &POLYGON_OR_HOLES_OR_AUTO,
        description: "Plan footprint; with `height` the body is a vertical prism and other required properties become optional",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Height",
    },
    PropertyDescriptor {
        name: "rotation",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Rotation in degrees (about Z, or about the axis for beams and members)",
    },
    PropertyDescriptor {
        name: "representation",
        required: false,
        expected_type: &REPRESENTATION,
        description: "`none` for an element without a body, such as an assembly of parts; no geometry is required",
    },
    PropertyDescriptor {
        name: "part_of",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Element this one is a part of (IfcRelAggregates), e.g. a stair flight of a stair",
    },
    PropertyDescriptor {
        name: "connects",
        required: false,
        expected_type: &ELEMENT_REFS,
        description: "Elements this one is connected to (IfcRelConnectsElements); on a fastener or discrete accessory, the elements it connects (IfcRelConnectsWithRealizingElements), e.g. [SL-01, MB-01]",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference (IfcRelAssociatesMaterial)",
    },
];

const FIRE_RATING_PROP: PropertyDescriptor = PropertyDescriptor {
    name: "fire_rating",
    required: false,
    expected_type: &FIRE_RATINGS,
    description: "Fire resistance rating (e.g., REI90)",
};
const THERMAL_PROP: PropertyDescriptor = PropertyDescriptor {
    name: "thermal_transmittance",
    required: false,
    expected_type: &THERMAL,
    description: "Thermal transmittance (U-value in W/m²K)",
};
const ACOUSTIC_PROP: PropertyDescriptor = PropertyDescriptor {
    name: "acoustic_rating",
    required: false,
    expected_type: &ACOUSTIC,
    description: "Sound insulation rating (dB)",
};

static WALL_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "from",
        required: true,
        expected_type: &AXIS,
        description: "Wall axis line (start→end, optionally curved via midpoint)",
    },
    PropertyDescriptor {
        name: "path",
        required: false,
        expected_type: &POLYGON,
        description: "Plan axis [(x, y), ...] instead of `from`/`to`; `smooth` points make it a curve",
    },
    PropertyDescriptor {
        name: "via",
        required: false,
        expected_type: &POINT,
        description: "Midpoint for curved wall arcs",
    },
    PropertyDescriptor {
        name: "height",
        required: true,
        expected_type: &LENGTH,
        description: "Wall height",
    },
    PropertyDescriptor {
        name: "thickness",
        required: true,
        expected_type: &LENGTH,
        description: "Wall thickness",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the wall is an exterior wall",
    },
    FIRE_RATING_PROP,
    THERMAL_PROP,
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the wall is load-bearing",
    },
    ACOUSTIC_PROP,
    PropertyDescriptor {
        name: "extend_to_structure",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the wall extends to structural elements above",
    },
    PropertyDescriptor {
        name: "mullions",
        required: false,
        expected_type: &LENGTHS,
        description: "Curtain wall: largest mullion spacing, or mullion positions along the axis",
    },
    PropertyDescriptor {
        name: "transoms",
        required: false,
        expected_type: &LENGTHS,
        description: "Curtain wall: largest transom spacing, or transom heights above the wall base",
    },
    PropertyDescriptor {
        name: "mullion_profile",
        required: false,
        expected_type: &PROFILE,
        description: "Curtain wall: mullion section, face width x depth",
    },
    PropertyDescriptor {
        name: "transom_profile",
        required: false,
        expected_type: &PROFILE,
        description: "Curtain wall: transom section, face width x depth (default: mullion_profile)",
    },
    PropertyDescriptor {
        name: "panel_material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Curtain wall: material of the panels",
    },
    PropertyDescriptor {
        name: "brackets",
        required: false,
        expected_type: &BOOLEAN,
        description: "Curtain wall: a bracket on each mullion at each slab edge it passes, connecting the two",
    },
];

static SLAB_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "boundary",
        required: true,
        expected_type: &POLYGON_OR_HOLES_OR_AUTO,
        description: "Slab boundary polygon (with optional holes) or `auto` for space inference",
    },
    PropertyDescriptor {
        name: "thickness",
        required: true,
        expected_type: &LENGTH,
        description: "Slab thickness",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "slab_type",
        required: false,
        expected_type: &SLAB_TYPES,
        description: "Slab classification (floor, roof, landing)",
    },
    PropertyDescriptor {
        name: "holes",
        required: false,
        expected_type: &POLYGON_WITH_HOLES,
        description: "Interior holes/voids in the slab",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the slab is an exterior slab",
    },
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the slab is load-bearing",
    },
    FIRE_RATING_PROP,
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static COLUMN_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "position",
        required: true,
        expected_type: &POINT,
        description: "Column center position (x, y)",
    },
    PropertyDescriptor {
        name: "profile",
        required: true,
        expected_type: &PROFILE,
        description: "Cross-section profile (e.g., rectangle(0.3, 0.3))",
    },
    PropertyDescriptor {
        name: "height",
        required: true,
        expected_type: &LENGTH,
        description: "Column height",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the column is external",
    },
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the column is load-bearing",
    },
    FIRE_RATING_PROP,
];

static BEAM_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "from",
        required: true,
        expected_type: &AXIS,
        description: "Beam axis line (start→end)",
    },
    PropertyDescriptor {
        name: "path",
        required: false,
        expected_type: &POLYGON,
        description: "Plan axis [(x, y), ...] instead of `from`/`to`; `smooth` points make it a curve",
    },
    PropertyDescriptor {
        name: "profile",
        required: false,
        expected_type: &PROFILE,
        description: "Cross-section profile",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the beam is external",
    },
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the beam is load-bearing",
    },
    PropertyDescriptor {
        name: "span",
        required: false,
        expected_type: &LENGTH,
        description: "Beam span length",
    },
    FIRE_RATING_PROP,
];

static WINDOW_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "offset",
        required: true,
        expected_type: &LENGTH,
        description: "Position along parent wall axis",
    },
    PropertyDescriptor {
        name: "width",
        required: true,
        expected_type: &LENGTH,
        description: "Window width",
    },
    PropertyDescriptor {
        name: "height",
        required: true,
        expected_type: &LENGTH,
        description: "Window height",
    },
    PropertyDescriptor {
        name: "sill_height",
        required: true,
        expected_type: &LENGTH,
        description: "Height from floor to window sill",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &WINDOW_TYPES,
        description: "Window type (single_glazed, double_glazed, etc.)",
    },
    PropertyDescriptor {
        name: "u_value",
        required: false,
        expected_type: &THERMAL,
        description: "Thermal transmittance (U-value)",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the window is an exterior window",
    },
    PropertyDescriptor {
        name: "fire_rating",
        required: false,
        expected_type: &FIRE_RATINGS,
        description: "Fire resistance rating",
    },
    PropertyDescriptor {
        name: "lateral_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Offset from the wall axis across the thickness",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Size across the wall thickness",
    },
    PropertyDescriptor {
        name: "opening",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Opening element this one fills (default: an opening is generated)",
    },
    PropertyDescriptor {
        name: "opening_guid",
        required: false,
        expected_type: &STRING,
        description: "IFC GlobalId of the generated opening",
    },
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static DOOR_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "offset",
        required: true,
        expected_type: &LENGTH,
        description: "Position along parent wall axis",
    },
    PropertyDescriptor {
        name: "width",
        required: true,
        expected_type: &LENGTH,
        description: "Door width",
    },
    PropertyDescriptor {
        name: "height",
        required: true,
        expected_type: &LENGTH,
        description: "Door height",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &DOOR_TYPES,
        description: "Door type (single_leaf, double_leaf, etc.)",
    },
    PropertyDescriptor {
        name: "sill_height",
        required: false,
        expected_type: &LENGTH,
        description: "Height of the door bottom above the wall base (default 0)",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the door is an exterior door",
    },
    PropertyDescriptor {
        name: "fire_rating",
        required: false,
        expected_type: &FIRE_RATINGS,
        description: "Fire resistance rating",
    },
    PropertyDescriptor {
        name: "lateral_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Offset from the wall axis across the thickness",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Size across the wall thickness",
    },
    PropertyDescriptor {
        name: "opening",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Opening element this one fills (default: an opening is generated)",
    },
    PropertyDescriptor {
        name: "opening_guid",
        required: false,
        expected_type: &STRING,
        description: "IFC GlobalId of the generated opening",
    },
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static ROOF_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "type",
        required: true,
        expected_type: &ROOF_TYPES,
        description: "Roof type (gable, hip, flat, mansard, shed)",
    },
    PropertyDescriptor {
        name: "boundary",
        required: true,
        expected_type: &POLYGON,
        description: "Roof boundary polygon",
    },
    PropertyDescriptor {
        name: "ridge_height",
        required: true,
        expected_type: &LENGTH,
        description: "Height of the roof ridge above eave",
    },
    PropertyDescriptor {
        name: "ridge_direction",
        required: false,
        expected_type: &DIRECTION,
        description: "Ridge direction (x or y)",
    },
    PropertyDescriptor {
        name: "overhang",
        required: false,
        expected_type: &LENGTH,
        description: "Roof overhang distance",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "holes",
        required: false,
        expected_type: &POLYGON_WITH_HOLES,
        description: "Roof openings (skylights, etc.)",
    },
    PropertyDescriptor {
        name: "thickness",
        required: false,
        expected_type: &LENGTH,
        description: "Flat roof thickness (default 0.15 m)",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the roof is external",
    },
    FIRE_RATING_PROP,
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static STAIR_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "from_storey",
        required: false,
        expected_type: &STOREY_REF,
        description: "Lower storey reference",
    },
    PropertyDescriptor {
        name: "to_storey",
        required: false,
        expected_type: &STOREY_REF,
        description: "Upper storey reference",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STAIR_TYPES,
        description: "Stair type (straight, l_shaped, u_shaped, spiral)",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Stair width",
    },
    PropertyDescriptor {
        name: "riser_count",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Number of risers",
    },
    PropertyDescriptor {
        name: "riser_height",
        required: false,
        expected_type: &LENGTH,
        description: "Height of each riser",
    },
    PropertyDescriptor {
        name: "tread_depth",
        required: false,
        expected_type: &LENGTH,
        description: "Depth of each tread",
    },
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Stair start position (x, y)",
    },
    PropertyDescriptor {
        name: "direction",
        required: false,
        expected_type: &POINT,
        description: "Stair direction vector",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the stair is external",
    },
    PropertyDescriptor {
        name: "nosing_length",
        required: false,
        expected_type: &LENGTH,
        description: "Nosing projection length",
    },
    PropertyDescriptor {
        name: "landing_after",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Risers before the landing (default half); with a straight stair adds a mid landing",
    },
    PropertyDescriptor {
        name: "landing_depth",
        required: false,
        expected_type: &LENGTH,
        description: "Landing depth along the first flight (default the width)",
    },
    PropertyDescriptor {
        name: "turn",
        required: false,
        expected_type: &STAIR_TURNS,
        description: "Side the second flight of an L or U stair turns to (left, right)",
    },
    PropertyDescriptor {
        name: "gap",
        required: false,
        expected_type: &LENGTH,
        description: "Gap between the flights of a U stair (default 0.1 m)",
    },
    PropertyDescriptor {
        name: "waist_thickness",
        required: false,
        expected_type: &LENGTH,
        description: "Flight thickness under the steps, perpendicular to the pitch (default solid to the base)",
    },
    PropertyDescriptor {
        name: "walking_line_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Walking line offset from inner edge",
    },
    PropertyDescriptor {
        name: "tread_length",
        required: false,
        expected_type: &LENGTH,
        description: "Tread length at walking line",
    },
    PropertyDescriptor {
        name: "tread_length_at_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Tread length at walking line offset",
    },
    PropertyDescriptor {
        name: "tread_length_at_inner_side",
        required: false,
        expected_type: &LENGTH,
        description: "Tread length at inner side",
    },
    FIRE_RATING_PROP,
];

static RAILING_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Railing height",
    },
    PropertyDescriptor {
        name: "length",
        required: false,
        expected_type: &LENGTH,
        description: "Railing length",
    },
    PropertyDescriptor {
        name: "from",
        required: false,
        expected_type: &AXIS,
        description: "Railing axis line",
    },
    PropertyDescriptor {
        name: "path",
        required: false,
        expected_type: &POLYGON,
        description: "Plan axis [(x, y), ...] instead of `from`/`to`; `smooth` points make it a curve",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the railing is external",
    },
    PropertyDescriptor {
        name: "thickness",
        required: false,
        expected_type: &LENGTH,
        description: "Railing thickness (default 0.05 m)",
    },
];

static RAMP_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Ramp width",
    },
    PropertyDescriptor {
        name: "slope",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Slope ratio (e.g., 0.08 for 8%)",
    },
    PropertyDescriptor {
        name: "from",
        required: false,
        expected_type: &AXIS,
        description: "Ramp axis line",
    },
];

static FURNITURE_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Furniture position (x, y)",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Furniture type description",
    },
    PropertyDescriptor {
        name: "asset",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Asset library reference",
    },
    PropertyDescriptor {
        name: "rotation",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Rotation angle in degrees",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Furniture width",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Furniture depth",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Furniture height",
    },
];

static ELEVATOR_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "position",
        required: true,
        expected_type: &POINT,
        description: "Shaft position (x, y)",
    },
    PropertyDescriptor {
        name: "shaft_width",
        required: true,
        expected_type: &LENGTH,
        description: "Shaft width",
    },
    PropertyDescriptor {
        name: "shaft_depth",
        required: true,
        expected_type: &LENGTH,
        description: "Shaft depth",
    },
    PropertyDescriptor {
        name: "from_storey",
        required: false,
        expected_type: &STOREY_REF,
        description: "Lowest storey served",
    },
    PropertyDescriptor {
        name: "to_storey",
        required: false,
        expected_type: &STOREY_REF,
        description: "Highest storey served",
    },
    PropertyDescriptor {
        name: "capacity",
        required: false,
        expected_type: &STRING,
        description: "Elevator capacity",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &ELEVATOR_TYPES,
        description: "Elevator type (passenger, freight, service)",
    },
];

static PROXY_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "position",
        required: true,
        expected_type: &POINT,
        description: "Element position (x, y)",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Element width",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Element depth",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Element height",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Element type description",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
];

static MEMBER_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "from",
        required: false,
        expected_type: &AXIS,
        description: "Member axis line",
    },
    PropertyDescriptor {
        name: "path",
        required: false,
        expected_type: &POLYGON,
        description: "Plan axis [(x, y), ...] instead of `from`/`to`; `smooth` points make it a curve",
    },
    PropertyDescriptor {
        name: "profile",
        required: false,
        expected_type: &PROFILE,
        description: "Cross-section profile",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Member type description",
    },
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Member position (x, y)",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Member height/length (decompiler fallback)",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the member is external",
    },
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the member is load-bearing",
    },
    FIRE_RATING_PROP,
];

static COVERING_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "boundary",
        required: false,
        expected_type: &POLYGON,
        description: "Covering outline; in a space it defaults to the space minus its walls and columns",
    },
    PropertyDescriptor {
        name: "thickness",
        required: false,
        expected_type: &LENGTH,
        description: "Covering thickness, extruded up from `z_offset` (default: 0.02 m)",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Covering type description",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the covering is external",
    },
    FIRE_RATING_PROP,
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static FOOTING_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "depth",
        required: true,
        expected_type: &LENGTH,
        description: "Footing depth, extruded down from `z_offset` (its top)",
    },
    PropertyDescriptor {
        name: "boundary",
        required: false,
        expected_type: &POLYGON,
        description: "Footing outline; defaults from `under:` or `position:`",
    },
    PropertyDescriptor {
        name: "under",
        required: false,
        expected_type: &ELEMENT_REFS,
        description: "Walls a strip footing runs under, e.g. [W-01, W-02]",
    },
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Centre of a pad footing",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Strip width, or pad size along x",
    },
    PropertyDescriptor {
        name: "length",
        required: false,
        expected_type: &LENGTH,
        description: "Pad size along y (default: `width`)",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
];

static PLATE_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "boundary",
        required: true,
        expected_type: &POLYGON,
        description: "Plate boundary polygon",
    },
    PropertyDescriptor {
        name: "from",
        required: false,
        expected_type: &AXIS,
        description: "Axis of an upright plate (instead of `boundary`)",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Height of an upright plate",
    },
    PropertyDescriptor {
        name: "thickness",
        required: true,
        expected_type: &LENGTH,
        description: "Plate thickness",
    },
    PropertyDescriptor {
        name: "material",
        required: false,
        expected_type: &MATERIAL_REF,
        description: "Material reference",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Plate type description",
    },
    PropertyDescriptor {
        name: "is_external",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the plate is external",
    },
    PropertyDescriptor {
        name: "load_bearing",
        required: false,
        expected_type: &BOOLEAN,
        description: "Whether the plate is load-bearing",
    },
    FIRE_RATING_PROP,
    THERMAL_PROP,
    ACOUSTIC_PROP,
];

static OPENING_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "offset",
        required: false,
        expected_type: &LENGTH,
        description: "Position along parent wall axis",
    },
    PropertyDescriptor {
        name: "width",
        required: false,
        expected_type: &LENGTH,
        description: "Opening width",
    },
    PropertyDescriptor {
        name: "height",
        required: false,
        expected_type: &LENGTH,
        description: "Opening height",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Opening type description",
    },
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Opening position (x, y)",
    },
    PropertyDescriptor {
        name: "sill_height",
        required: false,
        expected_type: &LENGTH,
        description: "Height of the opening bottom above the wall base",
    },
    PropertyDescriptor {
        name: "lateral_offset",
        required: false,
        expected_type: &LENGTH,
        description: "Offset from the wall axis across the thickness",
    },
    PropertyDescriptor {
        name: "depth",
        required: false,
        expected_type: &LENGTH,
        description: "Size across the wall thickness (default: wall thickness)",
    },
];

static GRID_PROPERTIES: &[PropertyDescriptor] = &[
    PropertyDescriptor {
        name: "position",
        required: false,
        expected_type: &POINT,
        description: "Grid origin position",
    },
    PropertyDescriptor {
        name: "x_axes",
        required: false,
        expected_type: &STRING,
        description: "X-axis labels (e.g., \"A,B,C\")",
    },
    PropertyDescriptor {
        name: "y_axes",
        required: false,
        expected_type: &STRING,
        description: "Y-axis labels (e.g., \"1,2,3\")",
    },
    PropertyDescriptor {
        name: "x_spacing",
        required: false,
        expected_type: &LENGTH,
        description: "Spacing between X-axis lines",
    },
    PropertyDescriptor {
        name: "y_spacing",
        required: false,
        expected_type: &LENGTH,
        description: "Spacing between Y-axis lines",
    },
    PropertyDescriptor {
        name: "x_count",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Number of X-axis lines",
    },
    PropertyDescriptor {
        name: "y_count",
        required: false,
        expected_type: &BARE_NUMBER,
        description: "Number of Y-axis lines",
    },
    PropertyDescriptor {
        name: "type",
        required: false,
        expected_type: &STRING,
        description: "Grid type description",
    },
];

// ---------------------------------------------------------------------------
// The Registry
// ---------------------------------------------------------------------------

/// The central element type registry — one entry per BML element type.
pub static ELEMENT_REGISTRY: &[ElementTypeDescriptor] = &[
    // ---- Wall ----
    ElementTypeDescriptor {
        element_type: ElementType::Wall,
        name: "Wall",
        ifc_entity: "IfcWall",
        auto_id_prefix: "W",
        properties: WALL_PROPERTIES,
        hover_text: "**Wall** (`IfcWall`)\n\nA wall element defined by an axis line.\n\n**Required:** `from` (axis), `height`, `thickness`\n\n**IFC Mapping:** IfcWallStandardCase with IfcExtrudedAreaSolid geometry\n\nWith `mullions:` it is an IfcCurtainWall: a grid of mullions, transoms (`transoms:`) and panels of `thickness`",
        completion_snippet: "Wall: ${1:W-01}\n  from: (${2:0}, ${3:0}) to: (${4:6}, ${5:0})\n  height: ${6:2.8} m\n  thickness: ${7:0.24} m",
        scene_color: [0.85, 0.82, 0.78, 1.0],
        svg_css_class: "wall",
    },
    // ---- Slab ----
    ElementTypeDescriptor {
        element_type: ElementType::Slab,
        name: "Slab",
        ifc_entity: "IfcSlab",
        auto_id_prefix: "SL",
        properties: SLAB_PROPERTIES,
        hover_text: "**Slab** (`IfcSlab`)\n\nA horizontal element (floor, roof slab, landing).\n\n**Required:** `boundary` (polygon), `thickness`\n\n**IFC Mapping:** IfcSlab with extruded polygon geometry",
        completion_snippet: "Slab: ${1:SL-01}\n  boundary: [(${2:0},${3:0}), (${4:10},${5:0}), (${6:10},${7:8}), (${8:0},${9:8})]\n  thickness: ${10:0.25} m",
        scene_color: [0.75, 0.75, 0.73, 1.0],
        svg_css_class: "slab",
    },
    // ---- Column ----
    ElementTypeDescriptor {
        element_type: ElementType::Column,
        name: "Column",
        ifc_entity: "IfcColumn",
        auto_id_prefix: "COL",
        properties: COLUMN_PROPERTIES,
        hover_text: "**Column** (`IfcColumn`)\n\nA vertical structural element.\n\n**Required:** `position`, `profile`, `height`\n\n**IFC Mapping:** IfcColumn with IfcExtrudedAreaSolid",
        completion_snippet: "Column: ${1:COL-01}\n  position: (${2:0}, ${3:0})\n  profile: rectangle(${4:0.3}, ${5:0.3})\n  height: ${6:3.0} m",
        scene_color: [0.70, 0.70, 0.68, 1.0],
        svg_css_class: "column",
    },
    // ---- Beam ----
    ElementTypeDescriptor {
        element_type: ElementType::Beam,
        name: "Beam",
        ifc_entity: "IfcBeam",
        auto_id_prefix: "B",
        properties: BEAM_PROPERTIES,
        hover_text: "**Beam** (`IfcBeam`)\n\nA horizontal structural element.\n\n**Required:** `from` (axis), `profile`\n\n**IFC Mapping:** IfcBeam with swept profile geometry",
        completion_snippet: "Beam: ${1:B-01}\n  from: (${2:0}, ${3:0}) to: (${4:6}, ${5:0})\n  profile: rectangle(${6:0.3}, ${7:0.5})",
        scene_color: [0.65, 0.65, 0.63, 1.0],
        svg_css_class: "beam",
    },
    // ---- Window ----
    ElementTypeDescriptor {
        element_type: ElementType::Window,
        name: "Window",
        ifc_entity: "IfcWindow",
        auto_id_prefix: "WIN",
        properties: WINDOW_PROPERTIES,
        hover_text: "**Window** (`IfcWindow`)\n\nA window opening in a wall.\n\n**Required:** `offset`, `width`, `height`, `sill_height`\n\n**Parent:** Must be nested inside a Wall\n\n**IFC Mapping:** IfcWindow + IfcOpeningElement",
        completion_snippet: "Window: ${1:WIN-01}\n  offset: ${2:1.0} m\n  width: ${3:1.2} m\n  height: ${4:1.5} m\n  sill_height: ${5:0.9} m",
        scene_color: [0.50, 0.70, 0.90, 0.5],
        svg_css_class: "window",
    },
    // ---- Door ----
    ElementTypeDescriptor {
        element_type: ElementType::Door,
        name: "Door",
        ifc_entity: "IfcDoor",
        auto_id_prefix: "D",
        properties: DOOR_PROPERTIES,
        hover_text: "**Door** (`IfcDoor`)\n\nA door opening in a wall.\n\n**Required:** `offset`, `width`, `height`\n\n**Parent:** Must be nested inside a Wall\n\n**IFC Mapping:** IfcDoor + IfcOpeningElement",
        completion_snippet: "Door: ${1:D-01}\n  offset: ${2:1.0} m\n  width: ${3:0.9} m\n  height: ${4:2.1} m",
        scene_color: [0.55, 0.35, 0.15, 1.0],
        svg_css_class: "door",
    },
    // ---- Roof ----
    ElementTypeDescriptor {
        element_type: ElementType::Roof,
        name: "Roof",
        ifc_entity: "IfcRoof",
        auto_id_prefix: "RF",
        properties: ROOF_PROPERTIES,
        hover_text: "**Roof** (`IfcRoof`)\n\nA roof element.\n\n**Required:** `type`, `boundary`, `ridge_height` (not for `type: flat`)\n\n**IFC Mapping:** IfcRoof with typed geometry",
        completion_snippet: "Roof: ${1:RF-01}\n  type: ${2:gable}\n  boundary: [(${3:0},${4:0}), (${5:10},${6:0}), (${7:10},${8:8}), (${9:0},${10:8})]\n  ridge_height: ${11:3.0} m",
        scene_color: [0.72, 0.33, 0.26, 1.0],
        svg_css_class: "roof",
    },
    // ---- Stair ----
    ElementTypeDescriptor {
        element_type: ElementType::Stair,
        name: "Stair",
        ifc_entity: "IfcStair",
        auto_id_prefix: "ST",
        properties: STAIR_PROPERTIES,
        hover_text: "**Stair** (`IfcStair`)\n\nA staircase connecting storeys.\n\n**Required:** `from_storey`, `to_storey`, `type`, `width`, `riser_count`, `riser_height`, `tread_depth`",
        completion_snippet: "Stair: ${1:ST-01}\n  from_storey: ${2:Ground Floor}\n  to_storey: ${3:First Floor}\n  type: ${4:straight}\n  width: ${5:1.2} m\n  riser_count: ${6:18}\n  riser_height: ${7:0.167} m\n  tread_depth: ${8:0.28} m",
        scene_color: [0.80, 0.78, 0.75, 1.0],
        svg_css_class: "stair",
    },
    // ---- Railing ----
    ElementTypeDescriptor {
        element_type: ElementType::Railing,
        name: "Railing",
        ifc_entity: "IfcRailing",
        auto_id_prefix: "RL",
        properties: RAILING_PROPERTIES,
        hover_text: "**Railing** (`IfcRailing`)\n\nA railing or balustrade.\n\n**Optional:** `from` (axis), `height`, `length`",
        completion_snippet: "Railing: ${1:RL-01}\n  from: (${2:0}, ${3:0}) to: (${4:3}, ${5:0})\n  height: ${6:1.0} m",
        scene_color: [0.40, 0.40, 0.38, 1.0],
        svg_css_class: "railing",
    },
    // ---- Ramp ----
    ElementTypeDescriptor {
        element_type: ElementType::Ramp,
        name: "Ramp",
        ifc_entity: "IfcRamp",
        auto_id_prefix: "RM",
        properties: RAMP_PROPERTIES,
        hover_text: "**Ramp** (`IfcRamp`)\n\nAn inclined access element.\n\n**Optional:** `from` (axis), `width`, `slope`",
        completion_snippet: "Ramp: ${1:RM-01}\n  from: (${2:0}, ${3:0}) to: (${4:6}, ${5:0})\n  width: ${6:1.5} m\n  slope: ${7:0.08}",
        scene_color: [0.78, 0.76, 0.72, 1.0],
        svg_css_class: "ramp",
    },
    // ---- Furniture ----
    ElementTypeDescriptor {
        element_type: ElementType::Furniture,
        name: "Furniture",
        ifc_entity: "IfcFurnishingElement",
        auto_id_prefix: "FUR",
        properties: FURNITURE_PROPERTIES,
        hover_text: "**Furniture** (`IfcFurnishingElement`)\n\nA furnishing element.\n\n**Optional:** `position`, `type`",
        completion_snippet: "Furniture: ${1:FUR-01}\n  position: (${2:2.0}, ${3:3.0})\n  type: \"${4:desk}\"",
        scene_color: [0.60, 0.45, 0.30, 1.0],
        svg_css_class: "furniture",
    },
    // ---- Elevator ----
    ElementTypeDescriptor {
        element_type: ElementType::Elevator,
        name: "Elevator",
        ifc_entity: "IfcTransportElement",
        auto_id_prefix: "EL",
        properties: ELEVATOR_PROPERTIES,
        hover_text: "**Elevator** (`IfcTransportElement`)\n\nA vertical transportation element (elevator/lift shaft).\n\n**Required:** `position`, `shaft_width`, `shaft_depth`\n\n**Optional:** `type`, `capacity`, `from_storey`, `to_storey`",
        completion_snippet: "Elevator: ${1:EL-01}\n  position: (${2:5.0}, ${3:4.0})\n  shaft_width: ${4:2.0} m\n  shaft_depth: ${5:2.0} m",
        scene_color: [0.50, 0.50, 0.55, 0.7],
        svg_css_class: "elevator",
    },
    // ---- Proxy ----
    ElementTypeDescriptor {
        element_type: ElementType::Proxy,
        name: "Proxy",
        ifc_entity: "IfcBuildingElementProxy",
        auto_id_prefix: "PX",
        properties: PROXY_PROPERTIES,
        hover_text: "**Proxy** (`IfcBuildingElementProxy`)\n\nA generic building element for miscellaneous or unclassified components.\n\n**Required:** `position`\n\n**Optional:** `width`, `depth`, `height`, `type`, `material`",
        completion_snippet: "Proxy: ${1:PX-01}\n  position: (${2:1.0}, ${3:1.0})\n  width: ${4:0.5} m\n  depth: ${5:0.5} m\n  height: ${6:1.0} m",
        scene_color: [0.60, 0.60, 0.55, 0.8],
        svg_css_class: "proxy",
    },
    // ---- Member ----
    ElementTypeDescriptor {
        element_type: ElementType::Member,
        name: "Member",
        ifc_entity: "IfcMember",
        auto_id_prefix: "MB",
        properties: MEMBER_PROPERTIES,
        hover_text: "**Member** (`IfcMember`)\n\nA structural member (brace, strut, tie).\n\n**Required:** `from` (axis), `profile`\n\n**Optional:** `material`, `type`",
        completion_snippet: "Member: ${1:MB-01}\n  from: (${2:0}, ${3:0}) to: (${4:3}, ${5:3})\n  profile: rectangle(${6:0.1}, ${7:0.1})",
        scene_color: [0.55, 0.55, 0.60, 1.0],
        svg_css_class: "member",
    },
    // ---- Plate ----
    ElementTypeDescriptor {
        element_type: ElementType::Plate,
        name: "Plate",
        ifc_entity: "IfcPlate",
        auto_id_prefix: "PL",
        properties: PLATE_PROPERTIES,
        hover_text: "**Plate** (`IfcPlate`)\n\nA flat structural element (panel, steel plate).\n\n**Required:** `boundary` (or an upright plate's `from` axis and `height`), `thickness`\n\n**Optional:** `material`, `type`",
        completion_snippet: "Plate: ${1:PL-01}\n  boundary: [(${2:0},${3:0}), (${4:2},${5:0}), (${6:2},${7:1}), (${8:0},${9:1})]\n  thickness: ${10:0.01} m",
        scene_color: [0.70, 0.68, 0.65, 1.0],
        svg_css_class: "plate",
    },
    // ---- Covering ----
    ElementTypeDescriptor {
        element_type: ElementType::Covering,
        name: "Covering",
        ifc_entity: "IfcCovering",
        auto_id_prefix: "CV",
        properties: COVERING_PROPERTIES,
        hover_text: "**Covering** (`IfcCovering`)\n\nA finish: suspended ceiling, floor finish, cladding.\n\nIn a space its outline defaults to the space minus its walls and columns, and a `predefined_type: ceiling` defaults to `z_offset` = the space height.\n\n**Optional:** `boundary`, `thickness`, `material`, `predefined_type`",
        completion_snippet: "Covering: ${1:CV-01}\n  predefined_type: ${2:ceiling}\n  thickness: ${3:0.02} m",
        scene_color: [0.92, 0.91, 0.88, 1.0],
        svg_css_class: "covering",
    },
    // ---- Footing ----
    ElementTypeDescriptor {
        element_type: ElementType::Footing,
        name: "Footing",
        ifc_entity: "IfcFooting",
        auto_id_prefix: "FT",
        properties: FOOTING_PROPERTIES,
        hover_text: "**Footing** (`IfcFooting`)\n\nA foundation below walls or columns, extruded down by `depth` from `z_offset` (its top).\n\nA strip footing runs `under:` walls with `width:`; a pad footing sits at `position:` with `width:` (and `length:`). Else give a `boundary:`.\n\n**Required:** `depth`\n\n**Optional:** `under`, `position`, `width`, `length`, `boundary`, `material`, `predefined_type`",
        completion_snippet: "Footing: ${1:FT-01}\n  predefined_type: ${2:strip_footing}\n  under: [${3:W-01}]\n  width: ${4:0.6} m\n  depth: ${5:0.4} m",
        scene_color: [0.55, 0.55, 0.53, 1.0],
        svg_css_class: "footing",
    },
    // ---- Duct ----
    ElementTypeDescriptor {
        element_type: ElementType::Duct,
        name: "Duct",
        ifc_entity: "IfcDuctSegment",
        auto_id_prefix: "DU",
        properties: RUN_PROPERTIES,
        hover_text: "**Duct** (`IfcDuctSegment`, `IfcDuctFitting`)\n\nA straight segment `from:` (x, y, z) `to:` (x, y, z), or a route `path: [(x, y, z), ...]` that expands into segments and bends; a route ending on another route's segment joins it with a tee.\n\n**Section:** `diameter`, or `width` and `height`\n\n**Optional:** `system`, `fitting`, `branch`, `rotation`, `material`, `sleeve`, `clearance`, `openings`, `hangers`",
        completion_snippet: "Duct: ${1:DU-01}\n  system: ${2:SUPPLY}\n  width: ${3:0.4} m\n  height: ${4:0.25} m\n  path: [(${5:0}, ${6:0}, ${7:2.8}), (${8:5}, ${9:0}, ${10:2.8})]",
        scene_color: [0.62, 0.70, 0.78, 1.0],
        svg_css_class: "duct",
    },
    // ---- Pipe ----
    ElementTypeDescriptor {
        element_type: ElementType::Pipe,
        name: "Pipe",
        ifc_entity: "IfcPipeSegment",
        auto_id_prefix: "PI",
        properties: RUN_PROPERTIES,
        hover_text: "**Pipe** (`IfcPipeSegment`, `IfcPipeFitting`)\n\nA straight segment `from:` (x, y, z) `to:` (x, y, z), or a route `path: [(x, y, z), ...]` that expands into segments and bends; a route ending on another route's segment joins it with a tee.\n\n**Section:** `diameter`, or `width` and `height`\n\n**Optional:** `system`, `fitting`, `branch`, `rotation`, `material`, `sleeve`, `clearance`, `openings`, `hangers`",
        completion_snippet: "Pipe: ${1:PI-01}\n  system: ${2:DCW}\n  diameter: ${3:0.05} m\n  path: [(${4:0}, ${5:0}, ${6:2.8}), (${7:5}, ${8:0}, ${9:2.8})]",
        scene_color: [0.20, 0.45, 0.80, 1.0],
        svg_css_class: "pipe",
    },
    // ---- CableTray ----
    ElementTypeDescriptor {
        element_type: ElementType::CableTray,
        name: "CableTray",
        ifc_entity: "IfcCableCarrierSegment",
        auto_id_prefix: "CT",
        properties: RUN_PROPERTIES,
        hover_text: "**CableTray** (`IfcCableCarrierSegment`, `IfcCableCarrierFitting`)\n\nA straight segment `from:` (x, y, z) `to:` (x, y, z), or a route `path: [(x, y, z), ...]` that expands into segments and bends; a route ending on another route's segment joins it with a tee.\n\n**Section:** `diameter`, or `width` and `height`\n\n**Optional:** `system`, `fitting`, `branch`, `rotation`, `material`, `sleeve`, `clearance`, `openings`, `hangers`",
        completion_snippet: "CableTray: ${1:CT-01}\n  width: ${2:0.3} m\n  height: ${3:0.06} m\n  path: [(${4:0}, ${5:0}, ${6:2.9}), (${7:5}, ${8:0}, ${9:2.9})]",
        scene_color: [0.85, 0.65, 0.20, 1.0],
        svg_css_class: "cable-tray",
    },
    // ---- Terminal ----
    ElementTypeDescriptor {
        element_type: ElementType::Terminal,
        name: "Terminal",
        ifc_entity: "IfcFlowTerminal",
        auto_id_prefix: "TM",
        properties: TERMINAL_PROPERTIES,
        hover_text: "**Terminal** (air terminal, sanitary, sprinkler, light, outlet, ...)\n\nA box (or `diameter` cylinder) at `position:`, up from `z_offset`. `type:` sets the IFC class and default size: air_supply, air_extract, sprinkler, hose_reel, wc, washbasin, sink, urinal, shower, light, outlet, data_outlet, floor_drain, radiator, switch, smoke_detector, alarm.\n\n**Required:** `position`\n\n**Optional:** `type`, `width`, `depth`, `height`, `diameter`, `rotation`, `system`",
        completion_snippet: "Terminal: ${1:TM-01}\n  type: ${2:air_supply}\n  system: ${3:SUPPLY}\n  position: (${4:0}, ${5:0})\n  z_offset: ${6:2.75} m",
        scene_color: [0.90, 0.90, 0.92, 1.0],
        svg_css_class: "terminal",
    },
    // ---- Equipment ----
    ElementTypeDescriptor {
        element_type: ElementType::Equipment,
        name: "Equipment",
        ifc_entity: "IfcDistributionFlowElement",
        auto_id_prefix: "EQ",
        properties: TERMINAL_PROPERTIES,
        hover_text: "**Equipment** (air handler, fan, pump, tank, boiler, distribution board, damper, valve, ...)\n\nA box (or `diameter` cylinder) at `position:`, up from `z_offset`. `type:` sets the IFC class and default size: air_handler, fan, pump, tank, boiler, chiller, heat_exchanger, distribution_board, transformer, damper, fire_damper, valve.\n\n**Required:** `position`\n\n**Optional:** `type`, `width`, `depth`, `height`, `diameter`, `rotation`, `system`",
        completion_snippet: "Equipment: ${1:EQ-01}\n  type: ${2:air_handler}\n  position: (${3:0}, ${4:0})",
        scene_color: [0.55, 0.60, 0.55, 1.0],
        svg_css_class: "equipment",
    },
    // ---- Accessory ----
    ElementTypeDescriptor {
        element_type: ElementType::Accessory,
        name: "Accessory",
        ifc_entity: "IfcDiscreteAccessory",
        auto_id_prefix: "AC",
        properties: TERMINAL_PROPERTIES,
        hover_text: "**Accessory** (`IfcDiscreteAccessory`, `IfcMechanicalFastener`)\n\nA box (or `diameter` cylinder) at `position:`, up from `z_offset`. `type:` sets the IFC class and default size: hanger, bracket, base_plate, shoe, anchor_bolt. A hanger is a trapeze: a strut `width` wide with its top at `z_offset` and two rods `height` up. Runs with `hangers:` generate them.\n\n**Required:** `position`\n\n**Optional:** `type`, `z_offset`, `width`, `depth`, `height`, `diameter`, `rotation`, `system`",
        completion_snippet: "Accessory: ${1:AC-01}\n  type: ${2:hanger}\n  position: (${3:0}, ${4:0})\n  z_offset: ${5:2.6} m\n  width: ${6:0.4} m\n  height: ${7:0.5} m",
        scene_color: [0.45, 0.45, 0.48, 1.0],
        svg_css_class: "accessory",
    },
    // ---- System ----
    ElementTypeDescriptor {
        element_type: ElementType::System,
        name: "System",
        ifc_entity: "IfcDistributionSystem",
        auto_id_prefix: "SYS",
        properties: &[],
        hover_text: "**System** (`IfcDistributionSystem`)\n\nA distribution system that ducts, pipes, cable trays, terminals and equipment join with `system: <id>`. No geometry.\n\n**Optional:** `predefined_type` (e.g. ventilation, domesticcoldwater, fireprotection, electrical), `ifc_name`",
        completion_snippet: "System: ${1:SUPPLY}\n  predefined_type: ${2:ventilation}",
        scene_color: [0.0, 0.0, 0.0, 0.0],
        svg_css_class: "system",
    },
    // ---- Opening ----
    ElementTypeDescriptor {
        element_type: ElementType::Opening,
        name: "Opening",
        ifc_entity: "IfcOpeningElement",
        auto_id_prefix: "OP",
        properties: OPENING_PROPERTIES,
        hover_text: "**Opening** (`IfcOpeningElement`)\n\nA void/opening in a wall (child of Wall).\n\n**Required:** `offset`, `width`, `height`\n\n**Parent:** Must be nested inside a Wall\n\n**IFC Mapping:** IfcOpeningElement with extruded geometry",
        completion_snippet: "Opening: ${1:OP-01}\n  offset: ${2:2.0} m\n  width: ${3:1.0} m\n  height: ${4:2.0} m",
        scene_color: [0.30, 0.30, 0.30, 0.3],
        svg_css_class: "opening",
    },
    // ---- Grid ----
    ElementTypeDescriptor {
        element_type: ElementType::Grid,
        name: "Grid",
        ifc_entity: "IfcGrid",
        auto_id_prefix: "GR",
        properties: GRID_PROPERTIES,
        hover_text: "**Grid** (`IfcGrid`)\n\nA structural reference grid with labeled axes.\n\n**Optional:** `position`, `x_spacing`, `y_spacing`, `x_count`, `y_count`\n\n**IFC Mapping:** IfcGrid with IfcGridAxis entities",
        completion_snippet: "Grid: ${1:GR-01}\n  position: (${2:0}, ${3:0})\n  x_spacing: ${4:6.0} m\n  y_spacing: ${5:6.0} m\n  x_count: ${6:4}\n  y_count: ${7:3}",
        scene_color: [0.40, 0.40, 0.80, 0.5],
        svg_css_class: "grid",
    },
];

/// Look up a descriptor by element type.
pub fn get_descriptor(element_type: ElementType) -> &'static ElementTypeDescriptor {
    ELEMENT_REGISTRY
        .iter()
        .find(|d| d.element_type == element_type)
        .expect("all element types must be in the registry")
}

/// Look up a descriptor by name string.
pub fn get_descriptor_by_name(name: &str) -> Option<&'static ElementTypeDescriptor> {
    ELEMENT_REGISTRY.iter().find(|d| d.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_covers_all_element_types() {
        // Ensure every ElementType variant is in the registry
        let all = ElementType::all_names();
        for name in &all {
            let et = ElementType::parse(name).unwrap();
            let desc = get_descriptor(et);
            assert_eq!(desc.name, *name);
        }
        assert_eq!(ELEMENT_REGISTRY.len(), all.len());
    }

    #[test]
    fn test_required_properties_consistency() {
        for desc in ELEMENT_REGISTRY {
            let required = desc.required_properties();
            for prop in &required {
                let p = desc.get_property(prop).unwrap();
                assert!(p.required, "{}.{} should be required", desc.name, prop);
            }
        }
    }

    #[test]
    fn test_auto_id_prefix_nonempty() {
        for desc in ELEMENT_REGISTRY {
            assert!(
                !desc.auto_id_prefix.is_empty(),
                "{} has empty auto_id_prefix",
                desc.name
            );
        }
    }

    #[test]
    fn test_hover_text_contains_name() {
        for desc in ELEMENT_REGISTRY {
            assert!(
                desc.hover_text.contains(desc.name),
                "{} hover text doesn't contain its name",
                desc.name
            );
        }
    }

    #[test]
    fn test_ifc_entity_nonempty() {
        for desc in ELEMENT_REGISTRY {
            assert!(
                desc.ifc_entity.starts_with("Ifc"),
                "{} has invalid IFC entity: {}",
                desc.name,
                desc.ifc_entity
            );
        }
    }
}
