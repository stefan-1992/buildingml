use crate::ast::nodes::{Property, Value};
use crate::ast::span::Span;

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::ElementType;

/// Expected value type for a property.
#[derive(Debug, Clone)]
pub enum ExpectedType {
    /// Number with a length unit (m, cm, mm, ft, in) or bare number
    Length,
    /// Number with an area unit (m2, ft2) or bare number
    Area,
    /// Number with thermal unit (W/m2K)
    Thermal,
    /// Number with acoustic unit (dB)
    Acoustic,
    /// Number with density unit (kg/m3)
    Density,
    /// Plain number (year, count, etc.)
    BareNumber,
    /// Boolean (true/false)
    Boolean,
    /// Identifier referencing a material
    MaterialRef,
    /// Identifier referencing a storey
    StoreyRef,
    /// Any identifier (e.g. an IFC enumeration value)
    Identifier,
    /// One of a set of known enum values
    Enum(&'static [&'static str]),
    /// 2D point
    Point,
    /// Polygon (list of points)
    Polygon,
    /// Profile specification
    Profile,
    /// Axis (from/to points)
    Axis,
    /// Quoted string
    String,
    /// Enum values or freeform quoted string (for IFC interop)
    EnumOrString(&'static [&'static str]),
    /// Comma-separated number pair (lat, lon)
    NumberPair,
    /// Direction enum (x/y)
    Direction,
    /// Polygon with holes (nested polygon list)
    PolygonWithHoles,
    /// Polygon or the identifier "auto" (for slab-follows-space)
    PolygonOrAuto,
    /// Polygon or polygon-with-holes (space boundary)
    PolygonOrHoles,
    /// Polygon, polygon-with-holes, or "auto" (full slab boundary options)
    PolygonOrHolesOrAuto,
    /// One element id or a list of them (`[W-01, W-02]`)
    ElementRefs,
    /// One length or a list of them (`[1.1 m, 3.4 m]`)
    Lengths,
    /// 3D point (x, y, z)
    Point3D,
    /// 3D polyline [(x, y, z), ...]
    Path3D,
}

/// Whether a property is required or optional.
#[derive(Debug, Clone, Copy)]
pub struct PropertySchema {
    pub required: bool,
    pub expected_type: &'static ExpectedType,
}

/// Get the property schema for a given element type and property name.
/// Returns None if the property is not known for this element type.
///
/// Delegates to the centralized element type registry.
pub fn get_property_schema(
    element_type: ElementType,
    property_name: &str,
) -> Option<PropertySchema> {
    let desc = super::registry::get_descriptor(element_type);
    desc.get_property(property_name).map(|p| PropertySchema {
        required: p.required,
        expected_type: p.expected_type,
    })
}

/// Required properties absent from `present` (the property keys of an element).
/// `flat` is whether it declares `type: flat`.
pub fn missing_required(
    element_type: ElementType,
    present: &[&str],
    flat: bool,
) -> Vec<&'static str> {
    let mut required = required_properties(element_type);
    // A `boundary` on a type that has no native boundary makes the body an explicit
    // prism, so only its height is still needed.
    if present.contains(&"boundary")
        && !super::registry::get_descriptor(element_type).has_native_boundary()
    {
        required = vec!["height"];
    }
    // An upright plate stands on its axis instead of a plan boundary.
    if element_type == ElementType::Plate && present.contains(&"from") {
        required = vec!["height", "thickness"];
    }
    // A `path:` axis stands in for `from`/`to`.
    if present.contains(&"path") && is_linear(element_type) {
        required.retain(|r| *r != "from");
    }
    // A flat roof takes `thickness`, it has no ridge.
    if element_type == ElementType::Roof && flat {
        required.retain(|r| *r != "ridge_height");
    }
    // `representation: none` declares an element without a body.
    if present.contains(&"representation") {
        required.clear();
    }
    required.retain(|r| !present.contains(r));
    required
}

/// Types whose plan axis is `from`/`to` or `path:`.
fn is_linear(element_type: ElementType) -> bool {
    matches!(
        element_type,
        ElementType::Wall | ElementType::Railing | ElementType::Beam | ElementType::Member
    )
}

/// Get the list of required properties for an element type.
///
/// Delegates to the centralized element type registry.
pub fn required_properties(element_type: ElementType) -> Vec<&'static str> {
    super::registry::get_descriptor(element_type).required_properties()
}

/// A 2D point list: a polygon, or one with `smooth` points / `nurbs` segments.
fn is_ring(value: &Value) -> bool {
    matches!(value, Value::Polygon(_) | Value::CurvePath { dim: 2, .. })
}

/// An outline with holes, plain or curved.
fn is_region(value: &Value) -> bool {
    matches!(
        value,
        Value::PolygonWithHoles { .. } | Value::CurveRegion { .. }
    )
}

/// Check that a property value matches the expected type.
/// Returns a diagnostic if there's a mismatch.
pub fn check_property_type(
    property: &Property,
    expected: &ExpectedType,
    element_type_name: &str,
) -> Option<Diagnostic> {
    let matches = match expected {
        ExpectedType::Length => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _)
        ),
        ExpectedType::Area => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _)
        ),
        ExpectedType::Thermal => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _)
        ),
        ExpectedType::Acoustic => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _)
        ),
        ExpectedType::Density => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _)
        ),
        ExpectedType::BareNumber => matches!(property.value, Value::Number(_)),
        ExpectedType::Boolean => matches!(property.value, Value::Boolean(_)),
        ExpectedType::MaterialRef | ExpectedType::StoreyRef | ExpectedType::Identifier => {
            matches!(property.value, Value::Identifier(_))
        }
        ExpectedType::Enum(valid) => match &property.value {
            Value::Identifier(id) => valid.contains(&id.as_str()),
            _ => false,
        },
        ExpectedType::EnumOrString(valid) => match &property.value {
            Value::Identifier(id) => valid.contains(&id.as_str()),
            Value::String(_) => true,
            _ => false,
        },
        ExpectedType::Point => matches!(property.value, Value::Point(_, _)),
        ExpectedType::Point3D => matches!(property.value, Value::Point3D(..)),
        ExpectedType::Path3D => matches!(
            property.value,
            Value::Path3D(_) | Value::CurvePath { dim: 3, .. }
        ),
        ExpectedType::Polygon => is_ring(&property.value),
        ExpectedType::Profile => matches!(property.value, Value::Profile { .. }),
        ExpectedType::Axis => matches!(property.value, Value::Axis { .. }),
        ExpectedType::String => matches!(property.value, Value::String(_)),
        ExpectedType::NumberPair => matches!(property.value, Value::NumberPair(_, _)),
        ExpectedType::Direction => match &property.value {
            Value::Identifier(id) => id == "x" || id == "y",
            _ => false,
        },
        ExpectedType::PolygonWithHoles => is_region(&property.value),
        ExpectedType::PolygonOrHoles => is_ring(&property.value) || is_region(&property.value),
        ExpectedType::PolygonOrAuto => {
            is_ring(&property.value)
                || matches!(&property.value, Value::Identifier(id) if id == "auto")
        }
        ExpectedType::PolygonOrHolesOrAuto => {
            is_ring(&property.value)
                || is_region(&property.value)
                || matches!(&property.value, Value::Identifier(id) if id == "auto")
        }
        ExpectedType::ElementRefs => {
            matches!(property.value, Value::Identifier(_) | Value::EnumList(_))
        }
        ExpectedType::Lengths => matches!(
            property.value,
            Value::Number(_) | Value::NumberWithUnit(_, _) | Value::NumberList(_)
        ),
    };

    if matches {
        None
    } else {
        let expected_desc = match expected {
            ExpectedType::Length => "a number (with optional length unit)",
            ExpectedType::Area => "a number (with optional area unit)",
            ExpectedType::Thermal => "a number (with optional W/m2K unit)",
            ExpectedType::Acoustic => "a number (with optional dB unit)",
            ExpectedType::Density => "a number (with optional kg/m3 unit)",
            ExpectedType::BareNumber => "a number",
            ExpectedType::Boolean => "true or false",
            ExpectedType::MaterialRef => "a material identifier",
            ExpectedType::StoreyRef => "a storey identifier",
            ExpectedType::Identifier => "an identifier",
            ExpectedType::Enum(valid) => {
                // Can't return a dynamic string from a static match, so use a generic message
                return Some(Diagnostic::error(
                    DiagnosticCode::E102,
                    format!(
                        "invalid value for `{}` on {}: expected one of: {}",
                        property.key,
                        element_type_name,
                        valid.join(", ")
                    ),
                    property.span,
                ));
            }
            ExpectedType::EnumOrString(valid) => {
                return Some(Diagnostic::error(
                    DiagnosticCode::E102,
                    format!(
                        "invalid value for `{}` on {}: expected a quoted string or one of: {}",
                        property.key,
                        element_type_name,
                        valid.join(", ")
                    ),
                    property.span,
                ));
            }
            ExpectedType::Point => "a point (x, y)",
            ExpectedType::Point3D => "a 3D point (x, y, z)",
            ExpectedType::Path3D => "a 3D path [(x, y, z), ...]",
            ExpectedType::Polygon => "a polygon [(x,y), ...]",
            ExpectedType::Profile => "a profile (e.g., rectangle(0.3, 0.3))",
            ExpectedType::Axis => "an axis (from (x,y) to (x,y))",
            ExpectedType::String => "a quoted string",
            ExpectedType::NumberPair => "a number pair (e.g., 49.45, 12.18)",
            ExpectedType::Direction => "x or y",
            ExpectedType::PolygonWithHoles => "a polygon with holes",
            ExpectedType::PolygonOrAuto => "a polygon [(x,y), ...] or `auto`",
            ExpectedType::PolygonOrHoles => "a polygon, optionally with holes",
            ExpectedType::PolygonOrHolesOrAuto => {
                "a polygon [(x,y), ...], polygon with holes, or `auto`"
            }
            ExpectedType::ElementRefs => "an element id or a list [A-01, A-02]",
            ExpectedType::Lengths => "a length or a list [1.1 m, 3.4 m]",
        };
        Some(Diagnostic::error(
            DiagnosticCode::E102,
            format!(
                "wrong type for `{}` on {}: expected {}",
                property.key, element_type_name, expected_desc
            ),
            property.span,
        ))
    }
}

/// Spatial type property schemas.
pub fn get_spatial_property_schema(
    spatial_type: &str,
    property_name: &str,
) -> Option<PropertySchema> {
    static LENGTH: ExpectedType = ExpectedType::Length;
    static AREA: ExpectedType = ExpectedType::Area;
    static POLYGON_OR_HOLES: ExpectedType = ExpectedType::PolygonOrHoles;
    static STRING: ExpectedType = ExpectedType::String;
    static BARE_NUMBER: ExpectedType = ExpectedType::BareNumber;
    static NUMBER_PAIR: ExpectedType = ExpectedType::NumberPair;
    static STOREY_REF: ExpectedType = ExpectedType::StoreyRef;

    static BUILDING_TYPES: ExpectedType = ExpectedType::Enum(&[
        "residential",
        "commercial",
        "industrial",
        "educational",
        "healthcare",
        "mixed_use",
    ]);
    static USAGE_TYPES: ExpectedType = ExpectedType::Enum(&[
        "living",
        "bedroom",
        "kitchen",
        "bathroom",
        "office",
        "classroom",
        "corridor",
        "staircase",
        "escape_route",
        "circulation",
        "storage",
        "garage",
        "utility",
        "lobby",
        "laundry",
        "mail_room",
        "plant_room",
        "waste",
        "terrace",
        "balcony",
        "parking",
        "mechanical",
        "electrical",
        "telecom",
        "dining",
    ]);

    let (required, expected_type) = match (spatial_type, property_name) {
        // Site
        ("Site", "location") => (false, &NUMBER_PAIR),
        ("Site", "address") => (false, &STRING),

        // Building
        ("Building", "type") => (false, &BUILDING_TYPES),
        ("Building", "year") => (false, &BARE_NUMBER),
        ("Building", "gross_floor_area") => (false, &AREA),

        // Storey
        ("Storey", "elevation") => (false, &LENGTH),
        ("Storey", "height") => (false, &LENGTH),

        // Space
        ("Space", "usage") => (false, &USAGE_TYPES),
        ("Space", "area") => (false, &AREA),
        ("Space", "boundary") => (false, &POLYGON_OR_HOLES),
        ("Space", "zone") => (false, &STOREY_REF),

        // Storey — template application
        ("Storey", "apply_template") => (false, &STRING),

        _ => return None,
    };

    Some(PropertySchema {
        required,
        expected_type,
    })
}

/// Compute edit distance between two strings (Levenshtein distance).
fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(a_len + 1) {
        row[0] = i;
    }
    for (j, val) in matrix[0].iter_mut().enumerate().take(b_len + 1) {
        *val = j;
    }

    for (i, ca) in a.chars().enumerate() {
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}

/// Suggest a similar name from a list of known names.
/// Returns the closest match if the edit distance is within a reasonable threshold.
fn suggest_similar<'a>(input: &str, known: &[&'a str]) -> Option<&'a str> {
    let max_distance = match input.len() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };

    known
        .iter()
        .filter_map(|&candidate| {
            let dist = edit_distance(input, candidate);
            if dist <= max_distance && dist > 0 {
                Some((candidate, dist))
            } else {
                None
            }
        })
        .min_by_key(|(_, dist)| *dist)
        .map(|(name, _)| name)
}

/// Suggest a similar element type name from the known types.
pub fn suggest_element_type<'a>(input: &str, known: &[&'a str]) -> Option<&'a str> {
    suggest_similar(input, known)
}

/// Get all known property names for an element type.
///
/// Delegates to the centralized element type registry.
pub fn known_property_names(element_type: ElementType) -> Vec<&'static str> {
    super::registry::get_descriptor(element_type).known_property_names()
}

/// Validate all properties of an element.
/// Returns diagnostics for type mismatches, unknown properties, and missing required properties.
pub fn validate_element_properties(
    element_type: ElementType,
    properties: &[Property],
    span: Span,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let type_name = element_type.as_str();

    // Check each property
    for prop in properties {
        match get_property_schema(element_type, &prop.key) {
            Some(schema) => {
                if let Some(d) = check_property_type(prop, schema.expected_type, type_name) {
                    diagnostics.push(d);
                }
            }
            None => {
                let known = known_property_names(element_type);
                let help = if let Some(suggestion) = suggest_similar(&prop.key, &known) {
                    format!("did you mean `{}`?", suggestion)
                } else {
                    format!("known properties for {}: {}", type_name, known.join(", "))
                };
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::E103,
                        format!("unknown property `{}` for {}", prop.key, type_name),
                        prop.span,
                    )
                    .with_help(help),
                );
            }
        }
    }

    // Check for missing required properties
    let present: Vec<&str> = properties.iter().map(|p| p.key.as_str()).collect();
    let flat = properties
        .iter()
        .any(|p| p.key == "type" && matches!(&p.value, Value::Identifier(t) if t == "flat"));
    if is_linear(element_type)
        && let Some(path) = properties.iter().find(|p| p.key == "path")
        && present.contains(&"from")
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E312,
                format!("{type_name} has both `path:` and `from:`/`to:`"),
                path.span,
            )
            .with_help("keep one axis: `path:` or `from:`/`to:`".to_string()),
        );
    }
    let missing = missing_required(element_type, &present, flat);
    if !missing.is_empty() {
        let provided_list = if present.is_empty() {
            "none".to_string()
        } else {
            present.join(", ")
        };
        for req in &missing {
            // Check if any provided property is similar to the missing one (potential typo)
            let typo_hint = suggest_similar(req, &present);
            let help = if let Some(similar) = typo_hint {
                format!(
                    "provided properties: {}. Did you mean `{}` instead of `{}`?",
                    provided_list, req, similar
                )
            } else {
                format!(
                    "add `{}: <value>` to the {} body. Provided properties: {}",
                    req, type_name, provided_list
                )
            };
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E101,
                    format!("{} is missing required property `{}`", type_name, req),
                    span,
                )
                .with_help(help),
            );
        }
    }

    diagnostics
}

/// Validate properties of a spatial heading (Site, Building, Storey, Space).
pub fn validate_spatial_properties(spatial_type: &str, properties: &[Property]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for prop in properties {
        if let Some(schema) = get_spatial_property_schema(spatial_type, &prop.key)
            && let Some(d) = check_property_type(prop, schema.expected_type, spatial_type)
        {
            diagnostics.push(d);
        }
        // Don't warn on unknown spatial properties — they may be user-defined metadata
    }

    diagnostics
}
