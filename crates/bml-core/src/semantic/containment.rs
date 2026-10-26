use std::collections::HashMap;

use crate::ast::nodes::{BodyItem, ElementDeclaration, Heading, Item, Property, SourceFile};

use super::curves;
use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::model::*;
use super::type_check;

/// Counter for auto-generating element IDs when omitted.
struct AutoIdCounter {
    counts: HashMap<String, u32>,
}

impl AutoIdCounter {
    fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Generate an auto-ID for an element type with an optional storey abbreviation.
    fn next_id(&mut self, element_type: ElementType, storey_abbrev: &str) -> String {
        let prefix = super::registry::get_descriptor(element_type).auto_id_prefix;
        let key = format!("{}-{}", prefix, storey_abbrev);
        let count = self.counts.entry(key.clone()).or_insert(0);
        *count += 1;
        format!("{}-{:03}", key, count)
    }
}

/// Expected heading level for each spatial type.
fn expected_level(spatial_type: &str) -> Option<u8> {
    match spatial_type {
        "Site" => Some(1),
        "Building" => Some(2),
        "Storey" => Some(3),
        "Space" => Some(4),
        _ => None,
    }
}

/// Valid child element types for each parent element type.
pub(crate) fn valid_child_types(parent: ElementType) -> &'static [ElementType] {
    match parent {
        ElementType::Wall => &[ElementType::Window, ElementType::Door, ElementType::Opening],
        // Openings can void any host (slabs, roofs, plates, ...), as in IFC.
        _ => &[ElementType::Opening],
    }
}

/// Build the semantic model from a flat AST and collect diagnostics.
/// Curved point lists become polylines with chord error at most `curve_tolerance` (metres).
pub fn build_model(
    source_file: &SourceFile,
    curve_tolerance: f64,
) -> (SemanticModel, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut materials = Vec::new();
    let mut asset_libraries = Vec::new();
    let mut auto_id = AutoIdCounter::new();

    // Collect headings and top-level elements in order
    let mut sites: Vec<ResolvedSite> = Vec::new();

    for item in &source_file.items {
        match item {
            Item::Heading(heading) => {
                process_heading(
                    heading,
                    &mut sites,
                    &mut diagnostics,
                    &mut auto_id,
                    curve_tolerance,
                );
            }
            Item::Element(elem) => {
                let display_id = if elem.id.is_empty() {
                    "<anonymous>".to_string()
                } else {
                    elem.id.clone()
                };
                // Top-level elements (outside any heading) — warning
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::E201,
                        format!(
                            "element `{}` declared outside any spatial context",
                            display_id
                        ),
                        elem.span,
                    )
                    .with_help(
                        "place elements inside a spatial heading (### Storey or #### Space)",
                    ),
                );

                // Still resolve it so we don't lose data
                let resolved = resolve_element(
                    elem,
                    None,
                    &mut diagnostics,
                    &mut auto_id,
                    "ORPHAN",
                    curve_tolerance,
                );
                if let Some(resolved) = resolved {
                    // Try to add to most recent storey if available
                    if let Some(site) = sites.last_mut()
                        && let Some(building) = site.buildings.last_mut()
                        && let Some(storey) = building.storeys.last_mut()
                    {
                        storey.elements.push(resolved);
                        continue;
                    }
                    // Otherwise it's truly orphaned — already warned above
                }
            }
            Item::Material(mat) => {
                let props = resolve_properties(&mat.body);
                let material_type = classify_material(&props);
                let resolved = ResolvedMaterial {
                    id: mat.id.clone(),
                    material_type,
                    properties: props,
                    span: mat.span,
                };
                materials.push(resolved);
            }
            Item::AssetLibrary(lib) => {
                let resolved = resolve_asset_library(lib);
                asset_libraries.push(resolved);
            }
            Item::Template(_)
            | Item::CheckSuite(_)
            | Item::CheckDecl(_)
            | Item::Import(_)
            | Item::Comment(_) => {}
        }
    }

    let model = SemanticModel {
        sites,
        materials,
        asset_libraries,
    };
    (model, diagnostics)
}

fn process_heading(
    heading: &Heading,
    sites: &mut Vec<ResolvedSite>,
    diagnostics: &mut Vec<Diagnostic>,
    auto_id: &mut AutoIdCounter,
    tol: f64,
) {
    // Validate heading level matches spatial type
    if let Some(expected) = expected_level(&heading.spatial_type)
        && heading.level != expected
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E200,
                format!(
                    "heading level {} does not match spatial type `{}` (expected level {})",
                    heading.level, heading.spatial_type, expected
                ),
                heading.span,
            )
            .with_help(format!(
                "use {} {} for {}",
                "#".repeat(expected as usize),
                heading.spatial_type,
                heading.spatial_type
            )),
        );
        // Continue processing despite mismatch
    }

    // Derive a storey abbreviation for auto-IDs
    let storey_abbrev = if heading.spatial_type == "Storey" {
        abbreviate_storey_name(&heading.name)
    } else {
        String::new()
    };

    // Extract properties and elements from heading body
    let (mut properties, elements) = extract_body(
        &heading.body,
        None,
        diagnostics,
        auto_id,
        &storey_abbrev,
        tol,
    );

    // Validate spatial properties
    let body_props: Vec<Property> = heading
        .body
        .iter()
        .filter_map(|b| match b {
            BodyItem::Property(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    diagnostics.extend(type_check::validate_spatial_properties(
        &heading.spatial_type,
        &body_props,
    ));
    let mut curves = curves::resolve(
        &body_props,
        &mut properties,
        &heading.name,
        tol,
        diagnostics,
    );

    match heading.spatial_type.as_str() {
        "Site" => {
            sites.push(ResolvedSite {
                name: heading.name.clone(),
                implicit: false,
                properties,
                buildings: Vec::new(),
                spaces: Vec::new(),
                elements,
                span: heading.span,
            });
        }
        "Building" => {
            let building = ResolvedBuilding {
                name: heading.name.clone(),
                properties,
                storeys: Vec::new(),
                spaces: Vec::new(),
                elements,
                span: heading.span,
            };
            if let Some(site) = sites.last_mut() {
                site.buildings.push(building);
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E201,
                        format!("Building `{}` declared without a parent Site", heading.name),
                        heading.span,
                    )
                    .with_help("add a `# Site: <name>` heading before this Building"),
                );
                // Create an implicit site
                sites.push(ResolvedSite {
                    name: "ImplicitSite".to_string(),
                    implicit: true,
                    properties: Vec::new(),
                    buildings: vec![building],
                    spaces: Vec::new(),
                    elements: Vec::new(),
                    span: heading.span,
                });
            }
        }
        "Storey" => {
            let building = sites.last_mut().and_then(|s| s.buildings.last_mut());
            if let Some(building) = building {
                building.storeys.push(ResolvedStorey {
                    name: heading.name.clone(),
                    properties,
                    spaces: Vec::new(),
                    elements,
                    span: heading.span,
                });
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E201,
                        format!(
                            "Storey `{}` declared without a parent Building",
                            heading.name
                        ),
                        heading.span,
                    )
                    .with_help("add `# Site` and `## Building` headings before this Storey"),
                );
            }
        }
        "Space" => {
            let space = ResolvedSpace {
                name: heading.name.clone(),
                properties,
                elements,
                span: heading.span,
                curves: std::mem::take(&mut curves),
            };
            // A space belongs to the latest storey, else building, else site. A new
            // site or building starts empty, so this is the latest heading.
            let spaces = sites
                .last_mut()
                .map(|site| match site.buildings.last_mut() {
                    Some(b) => match b.storeys.last_mut() {
                        Some(storey) => &mut storey.spaces,
                        None => &mut b.spaces,
                    },
                    None => &mut site.spaces,
                });
            if let Some(spaces) = spaces {
                spaces.push(space);
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E201,
                        format!("Space `{}` declared without a parent Site", heading.name),
                        heading.span,
                    )
                    .with_help("add a `# Site` heading before this Space"),
                );
            }
        }
        _ => {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::E200,
                format!("unknown spatial type `{}`", heading.spatial_type),
                heading.span,
            ));
        }
    }
}

/// Extract resolved properties and elements from a body item list.
fn extract_body(
    body: &[BodyItem],
    parent_element_type: Option<ElementType>,
    diagnostics: &mut Vec<Diagnostic>,
    auto_id: &mut AutoIdCounter,
    storey_abbrev: &str,
    tol: f64,
) -> (Vec<ResolvedProperty>, Vec<ResolvedElement>) {
    let mut properties = Vec::new();
    let mut elements = Vec::new();

    for item in body {
        match item {
            BodyItem::Property(prop) => {
                properties.push(resolve_property(prop));
            }
            BodyItem::Element(elem) => {
                if let Some(resolved) = resolve_element(
                    elem,
                    parent_element_type,
                    diagnostics,
                    auto_id,
                    storey_abbrev,
                    tol,
                ) {
                    elements.push(resolved);
                }
            }
            BodyItem::CheckDecl(_) | BodyItem::Comment(_) => {}
        }
    }

    (properties, elements)
}

/// Resolve an element declaration into a ResolvedElement.
fn resolve_element(
    elem: &ElementDeclaration,
    parent_element_type: Option<ElementType>,
    diagnostics: &mut Vec<Diagnostic>,
    auto_id: &mut AutoIdCounter,
    storey_abbrev: &str,
    tol: f64,
) -> Option<ResolvedElement> {
    let element_type = match ElementType::parse(&elem.element_type) {
        Some(t) => t,
        None => {
            let known_types = ElementType::all_names();
            let help = if let Some(suggestion) =
                type_check::suggest_element_type(&elem.element_type, &known_types)
            {
                format!("did you mean `{}`?", suggestion)
            } else {
                format!("known element types: {}", known_types.join(", "))
            };
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E100,
                    format!("unknown element type `{}`", elem.element_type),
                    elem.span,
                )
                .with_help(help),
            );
            return None;
        }
    };

    // Auto-generate ID if omitted
    let id = if elem.id.is_empty() {
        auto_id.next_id(element_type, storey_abbrev)
    } else {
        elem.id.clone()
    };

    // Check nesting validity
    if let Some(parent) = parent_element_type {
        let valid_children = valid_child_types(parent);
        // A window or door filling an `opening:` sits next to it under any host.
        let fills_opening = matches!(element_type, ElementType::Window | ElementType::Door)
            && elem
                .body
                .iter()
                .any(|b| matches!(b, BodyItem::Property(p) if p.key == "opening"));
        if !valid_children.contains(&element_type) && !fills_opening {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::E202,
                    format!(
                        "{} cannot be nested inside {}",
                        element_type.as_str(),
                        parent.as_str()
                    ),
                    elem.span,
                )
                .with_help(format!(
                    "{} only allows child elements: {}",
                    parent.as_str(),
                    valid_children
                        .iter()
                        .map(|t| t.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            );
        }
    }

    // Extract properties and children
    let body_props: Vec<Property> = elem
        .body
        .iter()
        .filter_map(|b| match b {
            BodyItem::Property(p) => Some(p.clone()),
            _ => None,
        })
        .collect();

    // Type-check element properties
    diagnostics.extend(type_check::validate_element_properties(
        element_type,
        &body_props,
        elem.span,
    ));

    let (mut properties, children) = extract_body(
        &elem.body,
        Some(element_type),
        diagnostics,
        auto_id,
        storey_abbrev,
        tol,
    );
    let curves = curves::resolve(&body_props, &mut properties, &id, tol, diagnostics);

    Some(ResolvedElement {
        element_type,
        id,
        properties,
        children,
        span: elem.span,
        curves,
    })
}

/// Abbreviate a storey name for use in auto-generated IDs.
fn abbreviate_storey_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("ground") || lower.contains("gf") {
        "GF".to_string()
    } else if lower.contains("basement") {
        "BAS".to_string()
    } else if lower.contains("roof") {
        "RF".to_string()
    } else {
        // Extract floor number if present (e.g., "Floor 1" -> "F1", "First Floor" -> "FF")
        let words: Vec<&str> = name.split_whitespace().collect();
        if words.len() >= 2 {
            // Try to extract a number
            for word in &words {
                if let Ok(n) = word.parse::<u32>() {
                    return format!("F{}", n);
                }
            }
            // Use initials
            words
                .iter()
                .map(|w| w.chars().next().unwrap_or('X'))
                .collect::<String>()
                .to_uppercase()
        } else {
            // Single word — use first 2 chars
            name.chars().take(2).collect::<String>().to_uppercase()
        }
    }
}

fn resolve_property(prop: &Property) -> ResolvedProperty {
    ResolvedProperty {
        key: prop.key.clone(),
        value: resolve_value(&prop.value),
        span: prop.span,
    }
}

fn resolve_properties(body: &[BodyItem]) -> Vec<ResolvedProperty> {
    body.iter()
        .filter_map(|item| match item {
            BodyItem::Property(p) => Some(resolve_property(p)),
            _ => None,
        })
        .collect()
}

/// The items of a material's `layers:` list.
pub(crate) fn layer_list(properties: &[ResolvedProperty]) -> Option<&[ResolvedProperty]> {
    properties.iter().find_map(|p| match &p.value {
        ResolvedValue::List(items) if p.key == "layers" => Some(items.as_slice()),
        _ => None,
    })
}

/// Classify a material as layered or homogeneous based on its properties.
/// A material is layered when it has a `layers:` list (`- material: thickness`
/// items, first layer first) or says `type: layered`.
fn classify_material(properties: &[ResolvedProperty]) -> MaterialType {
    let layer_list = layer_list(properties);
    let typed_layered = properties.iter().any(|p| {
        p.key == "type" && matches!(&p.value, ResolvedValue::Identifier(id) if id == "layered")
    });
    if layer_list.is_some() || typed_layered {
        let layers = layer_list
            .into_iter()
            .flatten()
            .filter_map(|item| match item.value {
                ResolvedValue::Number(thickness) => Some(MaterialLayer {
                    name: item.key.clone(),
                    thickness,
                }),
                _ => None,
            })
            .collect();
        return MaterialType::Layered { layers };
    }

    // Default: homogeneous material
    let category = properties
        .iter()
        .find(|p| p.key == "category")
        .and_then(|p| {
            if let ResolvedValue::Identifier(ref s) = p.value {
                Some(s.clone())
            } else if let ResolvedValue::String(ref s) = p.value {
                Some(s.clone())
            } else {
                None
            }
        });

    let grade = properties.iter().find(|p| p.key == "grade").and_then(|p| {
        if let ResolvedValue::Identifier(ref s) = p.value {
            Some(s.clone())
        } else if let ResolvedValue::String(ref s) = p.value {
            Some(s.clone())
        } else {
            None
        }
    });

    let density = properties
        .iter()
        .find(|p| p.key == "density")
        .and_then(|p| {
            if let ResolvedValue::Number(n) = &p.value {
                Some(*n)
            } else {
                None
            }
        });

    MaterialType::Homogeneous {
        category,
        grade,
        density,
    }
}

fn resolve_value(value: &crate::ast::nodes::Value) -> ResolvedValue {
    use crate::ast::nodes::Value;
    match value {
        Value::Number(n) => ResolvedValue::Number(*n),
        Value::NumberWithUnit(n, unit) => {
            // Unknown units keep their number; the unit check pass warns about them
            ResolvedValue::Number(n * super::unit_normalize::unit_to_si_factor(unit).unwrap_or(1.0))
        }
        Value::NumberPair(a, b) => ResolvedValue::Point(*a, *b),
        Value::String(s) => ResolvedValue::String(s.clone()),
        Value::Boolean(b) => ResolvedValue::Boolean(*b),
        Value::Identifier(id) => ResolvedValue::Identifier(id.clone()),
        Value::Point(x, y) => ResolvedValue::Point(*x, *y),
        Value::Polygon(pts) => ResolvedValue::Polygon(pts.clone()),
        Value::Path3D(pts) => ResolvedValue::Path3D(pts.clone()),
        Value::PolygonWithHoles { outer, holes } => ResolvedValue::PolygonWithHoles {
            outer: outer.clone(),
            holes: holes.clone(),
        },
        Value::Profile {
            profile_type,
            params,
        } => ResolvedValue::Profile {
            profile_type: profile_type.clone(),
            params: params.clone(),
        },
        Value::Axis { from, to, via, z } => ResolvedValue::Axis {
            from: *from,
            to: *to,
            via: *via,
            z: *z,
        },
        Value::EnumList(vals) => ResolvedValue::EnumList(vals.clone()),
        Value::NumberList(items) => ResolvedValue::NumberList(
            items
                .iter()
                .filter_map(|v| match resolve_value(v) {
                    ResolvedValue::Number(n) => Some(n),
                    _ => None,
                })
                .collect(),
        ),
        Value::Point3D(x, y, z) => ResolvedValue::Point3D(*x, *y, *z),
        Value::GeometryPrimitive {
            geometry_type,
            args,
        } => ResolvedValue::GeometryPrimitive(resolve_geometry_primitive(geometry_type, args)),
        // Check expression values — resolve to String representation for property storage
        Value::List(items) => ResolvedValue::List(items.iter().map(resolve_property).collect()),
        Value::PathAccess(segments) => ResolvedValue::Identifier(segments.join(".")),
        // Points as written; elements and spaces replace them by the tessellated curve
        Value::CurvePath { items, dim } => curves::raw_path(items, *dim),
        Value::CurveRegion { outer, holes } => curves::raw_region(outer, holes),
        Value::ComparisonExpr { .. } | Value::InExpr { .. } | Value::FunctionCall { .. } => {
            // Expression values are handled by the check evaluation engine, not property resolution
            ResolvedValue::String(format!("{:?}", value))
        }
    }
}

/// Resolve an asset library definition from the AST into the semantic model.
fn resolve_asset_library(lib: &crate::ast::nodes::AssetLibraryDefinition) -> ResolvedAssetLibrary {
    let properties = resolve_properties_from_ast(&lib.properties);

    let assets = lib
        .assets
        .iter()
        .map(|asset| {
            let asset_props = resolve_properties_from_ast(&asset.properties);
            let parts = asset
                .parts
                .iter()
                .map(|part| {
                    let part_props = resolve_properties(&part.body);
                    let geometry = part_props.iter().find_map(|p| {
                        if p.key == "geometry"
                            && let ResolvedValue::GeometryPrimitive(g) = &p.value
                        {
                            return Some(g.clone());
                        }
                        None
                    });
                    let position = part_props
                        .iter()
                        .find_map(|p| {
                            if p.key == "position" {
                                match &p.value {
                                    ResolvedValue::Point3D(x, y, z) => Some((*x, *y, *z)),
                                    ResolvedValue::Point(x, y) => Some((*x, *y, 0.0)),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        })
                        .unwrap_or((0.0, 0.0, 0.0));
                    let rotation = part_props
                        .iter()
                        .find_map(|p| {
                            if p.key == "rotation"
                                && let ResolvedValue::Number(n) = &p.value
                            {
                                return Some(*n);
                            }
                            None
                        })
                        .unwrap_or(0.0);
                    let material = part_props.iter().find_map(|p| {
                        if p.key == "material"
                            && let ResolvedValue::Identifier(id) = &p.value
                        {
                            return Some(id.clone());
                        }
                        None
                    });
                    ResolvedAssetPart {
                        name: part.name.clone(),
                        geometry,
                        position,
                        rotation,
                        material,
                        span: part.span,
                    }
                })
                .collect();
            ResolvedAsset {
                id: asset.id.clone(),
                properties: asset_props,
                parts,
                span: asset.span,
            }
        })
        .collect();

    ResolvedAssetLibrary {
        name: lib.name.clone(),
        properties,
        assets,
        span: lib.span,
    }
}

/// Resolve a list of AST Property values to ResolvedProperty.
fn resolve_properties_from_ast(props: &[crate::ast::nodes::Property]) -> Vec<ResolvedProperty> {
    props
        .iter()
        .map(|p| ResolvedProperty {
            key: p.key.clone(),
            value: resolve_value(&p.value),
            span: p.span,
        })
        .collect()
}

/// Resolve a geometry primitive AST value to a semantic GeometryPrimitive.
fn resolve_geometry_primitive(
    geometry_type: &str,
    args: &[crate::ast::nodes::Value],
) -> GeometryPrimitive {
    use crate::ast::nodes::Value;

    match geometry_type {
        "box" => {
            let w = match args.first() {
                Some(Value::Number(n)) => *n,
                _ => 1.0,
            };
            let d = match args.get(1) {
                Some(Value::Number(n)) => *n,
                _ => 1.0,
            };
            let h = match args.get(2) {
                Some(Value::Number(n)) => *n,
                _ => 1.0,
            };
            GeometryPrimitive::Box(w, d, h)
        }
        "cylinder" => {
            let r = match args.first() {
                Some(Value::Number(n)) => *n,
                _ => 0.5,
            };
            let h = match args.get(1) {
                Some(Value::Number(n)) => *n,
                _ => 1.0,
            };
            GeometryPrimitive::Cylinder(r, h)
        }
        "extrude" => {
            let pts = match args.first() {
                Some(Value::Polygon(p)) => p.clone(),
                _ => vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            };
            let h = match args.get(1) {
                Some(Value::Number(n)) => *n,
                _ => 1.0,
            };
            GeometryPrimitive::Extrude(pts, h)
        }
        "import" => {
            let path = match args.first() {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            GeometryPrimitive::Import(path)
        }
        _ => GeometryPrimitive::Box(1.0, 1.0, 1.0),
    }
}
