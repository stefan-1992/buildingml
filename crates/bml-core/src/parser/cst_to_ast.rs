use crate::ast::nodes::*;
use crate::ast::span::Span;
use crate::parser::error::ParseError;

/// Parse a BuildingML source string into a typed AST.
pub fn parse(source: &str) -> Result<SourceFile, Vec<ParseError>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_buildingml::LANGUAGE.into())
        .map_err(|e| vec![ParseError::TreeSitter(e.to_string())])?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| vec![ParseError::TreeSitter("parse returned None".into())])?;

    let root = tree.root_node();
    let mut errors = Vec::new();
    let mut file = convert_source_file(root, source.as_bytes(), &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    // Post-processing: collect asset headings into AssetLibraryDefinition items
    collect_asset_libraries(&mut file);

    Ok(file)
}

fn convert_source_file(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> SourceFile {
    let mut items = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "heading" => {
                if let Some(h) = convert_heading(child, source, errors) {
                    items.push(Item::Heading(h));
                }
            }
            "element_declaration" => {
                if let Some(e) = convert_element(child, source, errors) {
                    items.push(Item::Element(e));
                }
            }
            "import_statement" => {
                if let Some(i) = convert_import(child, source, errors) {
                    items.push(Item::Import(i));
                }
            }
            "material_definition" => {
                if let Some(m) = convert_material(child, source, errors) {
                    items.push(Item::Material(m));
                }
            }
            "template_definition" => {
                if let Some(t) = convert_template(child, source, errors) {
                    items.push(Item::Template(t));
                }
            }
            "check_suite_declaration" => {
                if let Some(cs) = convert_check_suite(child, source, errors) {
                    items.push(Item::CheckSuite(cs));
                }
            }
            "check_declaration" => {
                if let Some(c) = convert_check(child, source, errors) {
                    items.push(Item::CheckDecl(c));
                }
            }
            "comment" => {
                items.push(Item::Comment(convert_comment(child, source, false)));
            }
            "doc_comment" => {
                items.push(Item::Comment(convert_comment(child, source, true)));
            }
            "ERROR" => {
                errors.push(ParseError::SyntaxError {
                    message: format!(
                        "syntax error: {}",
                        node_text(child, source)
                            .chars()
                            .take(50)
                            .collect::<String>()
                    ),
                    span: Span::from_ts_node(&child),
                });
            }
            _ => {} // skip newlines and other anonymous nodes
        }
    }

    SourceFile {
        items,
        span: Span::from_ts_node(&node),
    }
}

fn convert_heading(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<Heading> {
    let marker = node
        .child_by_field_name("heading_marker")
        .or_else(|| find_child_by_kind(node, "heading_marker"))?;
    let level = node_text(marker, source).len() as u8;

    let spatial_type_node = find_child_by_kind(node, "spatial_type")?;
    let spatial_type = node_text(spatial_type_node, source).to_string();

    let name_node = find_child_by_kind(node, "heading_name")?;
    let name = node_text(name_node, source).trim().to_string();

    let body = collect_body_items(node, source, errors);

    Some(Heading {
        level,
        spatial_type,
        name,
        body,
        span: Span::from_ts_node(&node),
    })
}

fn convert_element(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<ElementDeclaration> {
    let type_node = find_child_by_kind(node, "element_type")?;
    let element_type = node_text(type_node, source).to_string();

    let id = find_child_by_kind(node, "element_id")
        .map(|id_node| node_text(id_node, source).trim().to_string())
        .unwrap_or_default();

    let body = collect_body_items(node, source, errors);

    Some(ElementDeclaration {
        element_type,
        id,
        body,
        span: Span::from_ts_node(&node),
    })
}

fn convert_material(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<MaterialDefinition> {
    let id_node = find_child_by_kind(node, "material_id")?;
    let id = node_text(id_node, source).to_string();

    let body = collect_body_items(node, source, errors);

    Some(MaterialDefinition {
        id,
        body,
        span: Span::from_ts_node(&node),
    })
}

fn convert_template(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<TemplateDefinition> {
    let id_node = find_child_by_kind(node, "template_id")?;
    let id = node_text(id_node, source).trim().to_string();

    let body = collect_body_items(node, source, errors);

    Some(TemplateDefinition {
        id,
        body,
        spaces: Vec::new(), // Space headings will be associated during template expansion
        span: Span::from_ts_node(&node),
    })
}

fn convert_check_suite(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<CheckSuiteDeclaration> {
    let name_node = find_child_by_kind(node, "check_suite_name")?;
    let name = node_text(name_node, source).trim().to_string();

    let body = collect_body_items(node, source, errors);

    Some(CheckSuiteDeclaration {
        name,
        body,
        span: Span::from_ts_node(&node),
    })
}

fn convert_check(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<CheckDeclaration> {
    let id_node = find_child_by_kind(node, "element_id")?;
    let id = node_text(id_node, source).to_string();

    let body = collect_body_items(node, source, errors);

    Some(CheckDeclaration {
        id,
        body,
        span: Span::from_ts_node(&node),
    })
}

fn convert_import(
    node: tree_sitter::Node,
    source: &[u8],
    _errors: &mut Vec<ParseError>,
) -> Option<Import> {
    let name_node = node.child_by_field_name("name")?;
    let path_node = node.child_by_field_name("path")?;

    let name = node_text(name_node, source).to_string();
    let path_raw = node_text(path_node, source);
    // Strip quotes
    let path = unquote(path_raw);

    Some(Import {
        name,
        path,
        span: Span::from_ts_node(&node),
    })
}

fn convert_comment(node: tree_sitter::Node, source: &[u8], is_doc: bool) -> Comment {
    let full_text = node_text(node, source);
    let prefix = if is_doc { "///" } else { "//" };
    let text = full_text
        .strip_prefix(prefix)
        .unwrap_or(full_text)
        .trim()
        .to_string();

    Comment {
        text,
        is_doc,
        span: Span::from_ts_node(&node),
    }
}

/// Collect body items (properties, elements, checks, comments) from a heading or element node.
fn collect_body_items(
    parent: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Vec<BodyItem> {
    let mut items = Vec::new();
    let mut cursor = parent.walk();

    for child in parent.children(&mut cursor) {
        match child.kind() {
            "property" => {
                if let Some(p) = convert_property(child, source, errors) {
                    items.push(BodyItem::Property(p));
                }
            }
            "element_declaration" => {
                if let Some(e) = convert_element(child, source, errors) {
                    items.push(BodyItem::Element(e));
                }
            }
            "check_declaration" => {
                if let Some(c) = convert_check(child, source, errors) {
                    items.push(BodyItem::CheckDecl(c));
                }
            }
            "comment" => {
                items.push(BodyItem::Comment(convert_comment(child, source, false)));
            }
            "doc_comment" => {
                items.push(BodyItem::Comment(convert_comment(child, source, true)));
            }
            "ERROR" => {
                errors.push(ParseError::SyntaxError {
                    message: format!(
                        "syntax error in body: {}",
                        node_text(child, source)
                            .chars()
                            .take(50)
                            .collect::<String>()
                    ),
                    span: Span::from_ts_node(&child),
                });
            }
            _ => {} // heading_marker, spatial_type, heading_name, element_type, element_id, etc.
        }
    }

    items
}

fn convert_property(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<Property> {
    if let Some(list) = find_child_by_kind(node, "list_property") {
        let key = node_text(list.child_by_field_name("key")?, source).to_string();
        let mut items = Vec::new();
        let mut cursor = list.walk();
        for item in list.children(&mut cursor) {
            if item.kind() != "list_item" {
                continue;
            }
            let (Some(k), Some(v)) = (
                item.child_by_field_name("key"),
                item.child_by_field_name("value"),
            ) else {
                continue;
            };
            if let Some(value) = convert_value(v, source, errors) {
                items.push(Property {
                    key: node_text(k, source).to_string(),
                    value,
                    span: Span::from_ts_node(&item),
                });
            }
        }
        return Some(Property {
            key,
            value: Value::List(items),
            span: Span::from_ts_node(&node),
        });
    }

    // property -> simple_property
    let simple = find_child_by_kind(node, "simple_property")?;

    let key_node = simple.child_by_field_name("key")?;
    let key = node_text(key_node, source).to_string();

    let value_node = simple.child_by_field_name("value")?;
    let value = convert_value(value_node, source, errors)?;

    Some(Property {
        key,
        value,
        span: Span::from_ts_node(&node),
    })
}

/// Coordinates of a `smooth_point` or point node.
fn point_coords(node: tree_sitter::Node, source: &[u8]) -> Vec<f64> {
    match node.kind() {
        "smooth_point" => find_child_by_kind(node, "point")
            .or_else(|| find_child_by_kind(node, "point3d"))
            .map(|p| parse_numbers(p, source))
            .unwrap_or_default(),
        _ => parse_numbers(node, source),
    }
}

/// Items of a `polygon` node (a point list) and their dimension (2 or 3). Mixing 2D and
/// 3D points in one list is a syntax error.
fn convert_path_items(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<(Vec<CurveItem>, u8)> {
    let mut items = Vec::new();
    let mut dim: Option<usize> = None;
    let mut mixed = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let item = match child.kind() {
            "point" | "point3d" => CurveItem::Corner(point_coords(child, source)),
            "smooth_point" => CurveItem::Smooth(point_coords(child, source)),
            "nurbs" => {
                let mut c = child.walk();
                let points: Vec<Vec<f64>> = child
                    .children(&mut c)
                    .filter(|n| matches!(n.kind(), "point" | "point3d"))
                    .map(|n| parse_numbers(n, source))
                    .collect();
                let array = |field: &str| {
                    child
                        .child_by_field_name(field)
                        .map(|n| parse_numbers(n, source))
                };
                let Some(degree) = child
                    .child_by_field_name("degree")
                    .and_then(|n| parse_number(n, source))
                else {
                    errors.push(ParseError::SyntaxError {
                        message: "nurbs degree is not a number".to_string(),
                        span: Span::from_ts_node(&child),
                    });
                    return None;
                };
                CurveItem::Nurbs {
                    degree,
                    points,
                    weights: array("weights"),
                    knots: array("knots"),
                }
            }
            _ => continue,
        };
        let coords: Vec<&Vec<f64>> = match &item {
            CurveItem::Corner(p) | CurveItem::Smooth(p) => vec![p],
            CurveItem::Nurbs { points, .. } => points.iter().collect(),
        };
        for p in coords {
            mixed |= dim.is_some_and(|d| d != p.len());
            dim.get_or_insert(p.len());
        }
        items.push(item);
    }
    if mixed {
        errors.push(ParseError::SyntaxError {
            message: "a point list cannot mix 2D and 3D points".to_string(),
            span: Span::from_ts_node(&node),
        });
        return None;
    }
    Some((items, dim.unwrap_or(2) as u8))
}

/// The coordinates when the list holds only plain points.
fn plain_points(items: &[CurveItem]) -> Option<Vec<&Vec<f64>>> {
    items
        .iter()
        .map(|i| match i {
            CurveItem::Corner(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// Identifiers and element ids of an `enum_list` node.
fn list_items(node: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| matches!(child.kind(), "identifier" | "element_ref"))
        .map(|child| node_text(child, source).to_string())
        .collect()
}

fn convert_value(
    node: tree_sitter::Node,
    source: &[u8],
    errors: &mut Vec<ParseError>,
) -> Option<Value> {
    match node.kind() {
        "number" => {
            let n = parse_number(node, source)?;
            Some(Value::Number(n))
        }
        "number_with_unit" => {
            let num_node = find_child_by_kind(node, "number")?;
            let unit_node = find_child_by_kind(node, "unit")?;
            let n = parse_number(num_node, source)?;
            let unit = node_text(unit_node, source).to_string();
            Some(Value::NumberWithUnit(n, unit))
        }
        "number_pair" => {
            let mut nums = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "number"
                    && let Some(n) = parse_number(child, source)
                {
                    nums.push(n);
                }
            }
            if nums.len() == 2 {
                Some(Value::NumberPair(nums[0], nums[1]))
            } else {
                None
            }
        }
        "string" => {
            let raw = node_text(node, source);
            let s = unquote(raw);
            Some(Value::String(s))
        }
        "boolean" => {
            let b = node_text(node, source) == "true";
            Some(Value::Boolean(b))
        }
        "identifier" | "element_ref" => {
            let id = node_text(node, source).to_string();
            Some(Value::Identifier(id))
        }
        "point3d" => {
            let nums = parse_numbers(node, source);
            if nums.len() == 3 {
                Some(Value::Point3D(nums[0], nums[1], nums[2]))
            } else {
                None
            }
        }
        "point" => {
            let (x, y) = parse_point(node, source)?;
            Some(Value::Point(x, y))
        }
        "polygon" => {
            let (items, dim) = convert_path_items(node, source, errors)?;
            match plain_points(&items) {
                Some(pts) if dim == 3 => Some(Value::Path3D(
                    pts.iter().map(|p| (p[0], p[1], p[2])).collect(),
                )),
                Some(pts) => Some(Value::Polygon(pts.iter().map(|p| (p[0], p[1])).collect())),
                None => Some(Value::CurvePath { items, dim }),
            }
        }
        "polygon_with_holes" => {
            let outer_node = node.child_by_field_name("outer")?;
            let (outer, dim) = convert_path_items(outer_node, source, errors)?;
            let mut flat = dim == 2;

            let mut holes = Vec::new();
            // The holes are polygon nodes that are field "holes"
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "polygon" && child.id() != outer_node.id() {
                    let (hole, hole_dim) = convert_path_items(child, source, errors)?;
                    flat &= hole_dim == 2;
                    if !hole.is_empty() {
                        holes.push(hole);
                    }
                }
            }

            if !flat {
                errors.push(ParseError::SyntaxError {
                    message: "outer ring and holes must be 2D points".to_string(),
                    span: Span::from_ts_node(&node),
                });
                return None;
            }

            let tuples = |items: &[CurveItem]| -> Option<Vec<(f64, f64)>> {
                Some(plain_points(items)?.iter().map(|p| (p[0], p[1])).collect())
            };
            let plain = tuples(&outer).zip(holes.iter().map(|h| tuples(h)).collect());
            match plain {
                Some((outer, holes)) => Some(Value::PolygonWithHoles { outer, holes }),
                None => Some(Value::CurveRegion { outer, holes }),
            }
        }
        "profile" => {
            let type_node = find_child_by_kind(node, "profile_type")?;
            let profile_type = node_text(type_node, source).to_string();
            let mut params = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "number"
                    && let Some(n) = parse_number(child, source)
                {
                    params.push(n);
                }
            }
            Some(Value::Profile {
                profile_type,
                params,
            })
        }
        "axis_value" => {
            let from_node = node.child_by_field_name("from")?;
            let to_node = node.child_by_field_name("to")?;
            let from = parse_numbers(from_node, source);
            let to = parse_numbers(to_node, source);
            let via = node
                .child_by_field_name("via")
                .and_then(|n| parse_point(n, source));
            // Both endpoints must agree: (x, y) to: (x, y) or (x, y, z) to: (x, y, z)
            let z = match (from.len(), to.len()) {
                (2, 2) => None,
                (3, 3) => Some((from[2], to[2])),
                _ => return None,
            };
            Some(Value::Axis {
                from: (from[0], from[1]),
                to: (to[0], to[1]),
                via,
                z,
            })
        }
        "enum_list" => Some(Value::EnumList(list_items(node, source))),
        "number_list" => {
            let mut cursor = node.walk();
            let items = node
                .named_children(&mut cursor)
                .filter_map(|child| convert_value(child, source, errors))
                .collect();
            Some(Value::NumberList(items))
        }
        "path_expression" => {
            let mut segments = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    segments.push(node_text(child, source).to_string());
                }
            }
            Some(Value::PathAccess(segments))
        }
        "comparison_expression" => {
            let left_node = node.child_by_field_name("left")?;
            let right_node = node.child_by_field_name("right")?;
            let op_node = find_child_by_kind(node, "comparison_op")?;

            let left = convert_value(left_node, source, errors)?;
            let right = convert_value(right_node, source, errors)?;
            let op = parse_comparison_op(node_text(op_node, source))?;

            Some(Value::ComparisonExpr {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        }
        "in_expression" => {
            let left_node = node.child_by_field_name("left")?;
            let right_node = node.child_by_field_name("right")?;

            let left = convert_value(left_node, source, errors)?;

            // right is an enum_list
            Some(Value::InExpr {
                left: Box::new(left),
                values: list_items(right_node, source),
            })
        }
        "geometry_primitive" => {
            let type_node = find_child_by_kind(node, "geometry_type")?;
            let geometry_type = node_text(type_node, source).to_string();
            let mut args = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "number" => {
                        if let Some(n) = parse_number(child, source) {
                            args.push(Value::Number(n));
                        }
                    }
                    "string" => {
                        let raw = node_text(child, source);
                        let s = unquote(raw);
                        args.push(Value::String(s));
                    }
                    "polygon" => {
                        let mut points = Vec::new();
                        let mut pc = child.walk();
                        for pt in child.children(&mut pc) {
                            if pt.kind() == "point"
                                && let Some(p) = parse_point(pt, source)
                            {
                                points.push(p);
                            }
                        }
                        args.push(Value::Polygon(points));
                    }
                    _ => {}
                }
            }
            Some(Value::GeometryPrimitive {
                geometry_type,
                args,
            })
        }
        "function_call" => {
            let name_node = node.child_by_field_name("name")?;
            let name = node_text(name_node, source).to_string();

            let mut args = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                // Skip the name identifier and parentheses
                if child.id() == name_node.id() {
                    continue;
                }
                match child.kind() {
                    "number_with_unit" | "number" | "boolean" | "string" | "function_call"
                    | "path_expression" | "identifier" => {
                        if let Some(v) = convert_value(child, source, errors) {
                            args.push(v);
                        }
                    }
                    _ => {}
                }
            }

            Some(Value::FunctionCall { name, args })
        }
        _ => {
            errors.push(ParseError::UnexpectedNode {
                kind: node.kind().to_string(),
                span: Span::from_ts_node(&node),
            });
            None
        }
    }
}

fn parse_comparison_op(s: &str) -> Option<ComparisonOp> {
    match s {
        ">=" => Some(ComparisonOp::Ge),
        "<=" => Some(ComparisonOp::Le),
        "==" => Some(ComparisonOp::Eq),
        "!=" => Some(ComparisonOp::Ne),
        ">" => Some(ComparisonOp::Gt),
        "<" => Some(ComparisonOp::Lt),
        _ => None,
    }
}

fn parse_number(node: tree_sitter::Node, source: &[u8]) -> Option<f64> {
    node_text(node, source).parse().ok()
}

fn parse_point(node: tree_sitter::Node, source: &[u8]) -> Option<(f64, f64)> {
    let mut nums = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "number"
            && let Some(n) = parse_number(child, source)
        {
            nums.push(n);
        }
    }
    if nums.len() == 2 {
        Some((nums[0], nums[1]))
    } else {
        None
    }
}

/// Post-process flat heading items to collect AssetLibrary/Asset/Part headings
/// into structured AssetLibraryDefinition items.
///
/// The heading hierarchy:
///   `# AssetLibrary: Name` (level 1)
///   `## Asset: ID` (level 2)
///   `### Part: Name` (level 3)
///
/// These are parsed as regular Heading items by tree-sitter. This function
/// reorganizes them into AssetLibraryDefinition, AssetDefinition, AssetPartDefinition.
fn collect_asset_libraries(file: &mut SourceFile) {
    let mut new_items = Vec::new();
    let mut i = 0;

    while i < file.items.len() {
        match &file.items[i] {
            Item::Heading(h) if h.spatial_type == "AssetLibrary" => {
                let lib_heading = if let Item::Heading(h) = &file.items[i] {
                    h.clone()
                } else {
                    unreachable!()
                };
                i += 1;

                let mut properties = Vec::new();
                for bi in &lib_heading.body {
                    if let BodyItem::Property(p) = bi {
                        properties.push(p.clone());
                    }
                }

                let mut assets = Vec::new();

                // Collect ## Asset: headings that follow
                while i < file.items.len() {
                    match &file.items[i] {
                        Item::Heading(ah) if ah.spatial_type == "Asset" => {
                            let asset_heading = ah.clone();
                            i += 1;

                            let mut asset_props = Vec::new();
                            for bi in &asset_heading.body {
                                if let BodyItem::Property(p) = bi {
                                    asset_props.push(p.clone());
                                }
                            }

                            let mut parts = Vec::new();

                            // Collect ### Part: headings that follow
                            while i < file.items.len() {
                                match &file.items[i] {
                                    Item::Heading(ph) if ph.spatial_type == "Part" => {
                                        let part_heading = ph.clone();
                                        i += 1;
                                        parts.push(AssetPartDefinition {
                                            name: part_heading.name.clone(),
                                            body: part_heading.body.clone(),
                                            span: part_heading.span,
                                        });
                                    }
                                    // Stop collecting parts when we see a non-Part heading
                                    Item::Heading(h)
                                        if h.spatial_type == "Asset"
                                            || h.spatial_type == "AssetLibrary"
                                            || h.spatial_type == "Site"
                                            || h.spatial_type == "Building"
                                            || h.spatial_type == "Storey"
                                            || h.spatial_type == "Space" =>
                                    {
                                        break;
                                    }
                                    _ => break,
                                }
                            }

                            assets.push(AssetDefinition {
                                id: asset_heading.name.clone(),
                                properties: asset_props,
                                parts,
                                span: asset_heading.span,
                            });
                        }
                        // Stop collecting assets on non-Asset heading
                        Item::Heading(h)
                            if h.spatial_type == "AssetLibrary"
                                || h.spatial_type == "Site"
                                || h.spatial_type == "Building" =>
                        {
                            break;
                        }
                        _ => break,
                    }
                }

                new_items.push(Item::AssetLibrary(AssetLibraryDefinition {
                    name: lib_heading.name.clone(),
                    properties,
                    assets,
                    span: lib_heading.span,
                }));
            }
            _ => {
                new_items.push(file.items[i].clone());
                i += 1;
            }
        }
    }

    file.items = new_items;
}

fn parse_numbers(node: tree_sitter::Node, source: &[u8]) -> Vec<f64> {
    let mut nums = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "number"
            && let Some(n) = parse_number(child, source)
        {
            nums.push(n);
        }
    }
    nums
}

fn find_child_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn node_text<'a>(node: tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let source = "// Minimal\n# Site: Test Site\n  location: 49.4521, 12.1803\n";
        let file = parse(source).expect("should parse");
        assert_eq!(file.items.len(), 2); // comment + heading
        match &file.items[0] {
            Item::Comment(c) => assert_eq!(c.text, "Minimal"),
            _ => panic!("expected comment"),
        }
        match &file.items[1] {
            Item::Heading(h) => {
                assert_eq!(h.level, 1);
                assert_eq!(h.spatial_type, "Site");
                assert_eq!(h.name, "Test Site");
                assert_eq!(h.body.len(), 1);
                match &h.body[0] {
                    BodyItem::Property(p) => {
                        assert_eq!(p.key, "location");
                        match &p.value {
                            Value::NumberPair(a, b) => {
                                assert!((a - 49.4521).abs() < 0.0001);
                                assert!((b - 12.1803).abs() < 0.0001);
                            }
                            _ => panic!("expected NumberPair, got {:?}", p.value),
                        }
                    }
                    _ => panic!("expected property"),
                }
            }
            _ => panic!("expected heading"),
        }
    }

    #[test]
    fn test_parse_wall_with_window() {
        let source = "### Storey: GF\n  Wall: W-01\n    from: (0, 0) to: (6, 0)\n    height: 2.80 m\n    Window: WIN-01\n      width: 1.2 m\n";
        let file = parse(source).expect("should parse");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            Item::Heading(h) => {
                assert_eq!(h.level, 3);
                assert_eq!(h.body.len(), 1);
                match &h.body[0] {
                    BodyItem::Element(e) => {
                        assert_eq!(e.element_type, "Wall");
                        assert_eq!(e.id, "W-01");
                        assert_eq!(e.body.len(), 3); // from, height, Window
                        match &e.body[0] {
                            BodyItem::Property(p) => {
                                assert_eq!(p.key, "from");
                                match &p.value {
                                    Value::Axis { from, to, .. } => {
                                        assert_eq!(*from, (0.0, 0.0));
                                        assert_eq!(*to, (6.0, 0.0));
                                    }
                                    _ => panic!("expected Axis"),
                                }
                            }
                            _ => panic!("expected property"),
                        }
                        // Window is a child element
                        match &e.body[2] {
                            BodyItem::Element(win) => {
                                assert_eq!(win.element_type, "Window");
                                assert_eq!(win.id, "WIN-01");
                            }
                            _ => panic!("expected element"),
                        }
                    }
                    _ => panic!("expected element"),
                }
            }
            _ => panic!("expected heading"),
        }
    }

    #[test]
    fn test_parse_import() {
        let source = "import materials from \"./materials.bml\"\n";
        let file = parse(source).expect("should parse");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            Item::Import(i) => {
                assert_eq!(i.name, "materials");
                assert_eq!(i.path, "./materials.bml");
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_parse_string_escapes_and_element_refs() {
        let source = concat!(
            "### Storey: GF\n",
            "  Proxy: PX-01\n",
            "    ifc_name: \"say \\\"hi\\\" C:\\\\x\"\n",
            "    part_of: RF-41021\n",
        );
        let file = parse(source).expect("should parse");
        let Item::Heading(heading) = &file.items[0] else {
            panic!("expected heading")
        };
        let BodyItem::Element(proxy) = &heading.body[0] else {
            panic!("expected element")
        };
        let values: Vec<&Value> = proxy
            .body
            .iter()
            .filter_map(|b| match b {
                BodyItem::Property(p) => Some(&p.value),
                _ => None,
            })
            .collect();
        assert!(matches!(values[0], Value::String(s) if s == "say \"hi\" C:\\x"));
        assert!(matches!(values[1], Value::Identifier(s) if s == "RF-41021"));
    }

    #[test]
    fn test_unquote_keeps_other_backslashes() {
        assert_eq!(unquote(r#""C:\dir\a.bml""#), r"C:\dir\a.bml");
        assert_eq!(unquote(r#""a\\b \"q\"""#), r#"a\b "q""#);
    }

    #[test]
    fn test_parse_all_value_types() {
        let source = concat!(
            "### Storey: GF\n",
            "  Wall: W-01\n",
            "    height: 2.80 m\n",
            "    is_external: true\n",
            "    material: concrete\n",
            "    fire_rating: REI90\n",
            "    address: \"here\"\n",
            "    boundary: [(0,0), (6,0), (6,5)]\n",
            "    profile: rectangle(0.3, 0.3)\n",
        );
        let file = parse(source).expect("should parse");
        let heading = match &file.items[0] {
            Item::Heading(h) => h,
            _ => panic!("expected heading"),
        };
        let wall = match &heading.body[0] {
            BodyItem::Element(e) => e,
            _ => panic!("expected element"),
        };
        // height: 2.80 m -> NumberWithUnit
        match &wall.body[0] {
            BodyItem::Property(p) => {
                assert!(matches!(&p.value, Value::NumberWithUnit(_, _)));
            }
            _ => panic!("expected property"),
        }
        // is_external: true -> Boolean
        match &wall.body[1] {
            BodyItem::Property(p) => {
                assert!(matches!(&p.value, Value::Boolean(true)));
            }
            _ => panic!("expected property"),
        }
        // material: concrete -> Identifier
        match &wall.body[2] {
            BodyItem::Property(p) => {
                assert!(matches!(&p.value, Value::Identifier(_)));
            }
            _ => panic!("expected property"),
        }
        // boundary: polygon
        match &wall.body[5] {
            BodyItem::Property(p) => match &p.value {
                Value::Polygon(pts) => assert_eq!(pts.len(), 3),
                _ => panic!("expected Polygon"),
            },
            _ => panic!("expected property"),
        }
        // profile: rectangle(0.3, 0.3)
        match &wall.body[6] {
            BodyItem::Property(p) => match &p.value {
                Value::Profile {
                    profile_type,
                    params,
                } => {
                    assert_eq!(profile_type, "rectangle");
                    assert_eq!(params.len(), 2);
                }
                _ => panic!("expected Profile"),
            },
            _ => panic!("expected property"),
        }
    }

    #[test]
    fn test_parse_fixtures() {
        let fixtures = [
            "minimal.bml",
            "wall_only.bml",
            "slab_only.bml",
            "spatial_hierarchy.bml",
            "full_example.bml",
            "compliance_checks.bml",
            "all_elements.bml",
            "two_storey_house.bml",
            "furniture_assets.bml",
        ];
        for name in &fixtures {
            let path = format!(
                "{}/tests/fixtures/{}",
                env!("CARGO_MANIFEST_DIR").replace("/crates/bml-core", ""),
                name
            );
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {}", path, e));
            let result = parse(&source);
            assert!(
                result.is_ok(),
                "fixture {} failed: {:?}",
                name,
                result.err()
            );
        }
    }

    #[test]
    fn test_parse_check_declaration() {
        let source = concat!(
            "Check: minimum_door_width\n",
            "  description: \"Doors must have minimum clear width\"\n",
            "  severity: error\n",
            "  applies_to: Door\n",
            "  assert: width >= 0.90 m\n",
        );
        let file = parse(source).expect("should parse");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            Item::CheckDecl(c) => {
                assert_eq!(c.id, "minimum_door_width");
                // description, severity, applies_to, assert
                assert_eq!(c.body.len(), 4);
                // Check assert is a comparison expression
                match &c.body[3] {
                    BodyItem::Property(p) => {
                        assert_eq!(p.key, "assert");
                        match &p.value {
                            Value::ComparisonExpr { left, op, right } => {
                                assert!(
                                    matches!(left.as_ref(), Value::Identifier(s) if s == "width")
                                );
                                assert_eq!(*op, ComparisonOp::Ge);
                                assert!(
                                    matches!(right.as_ref(), Value::NumberWithUnit(n, u) if (*n - 0.9).abs() < 0.001 && u == "m")
                                );
                            }
                            _ => panic!("expected ComparisonExpr, got {:?}", p.value),
                        }
                    }
                    _ => panic!("expected property"),
                }
            }
            _ => panic!("expected CheckDecl, got {:?}", file.items[0]),
        }
    }

    #[test]
    fn test_parse_check_with_where() {
        let source = concat!(
            "Check: fire_rated_walls\n",
            "  applies_to: Wall\n",
            "  where: is_fire_compartment_boundary == true\n",
            "  assert: fire_rating in [REI30, REI60, REI90, REI120]\n",
        );
        let file = parse(source).expect("should parse");
        match &file.items[0] {
            Item::CheckDecl(c) => {
                assert_eq!(c.id, "fire_rated_walls");
                // where is a comparison expression
                match &c.body[1] {
                    BodyItem::Property(p) => {
                        assert_eq!(p.key, "where");
                        assert!(matches!(&p.value, Value::ComparisonExpr { .. }));
                    }
                    _ => panic!("expected property"),
                }
                // assert is an in-expression
                match &c.body[2] {
                    BodyItem::Property(p) => {
                        assert_eq!(p.key, "assert");
                        match &p.value {
                            Value::InExpr { left, values } => {
                                assert!(
                                    matches!(left.as_ref(), Value::Identifier(s) if s == "fire_rating")
                                );
                                assert_eq!(values, &["REI30", "REI60", "REI90", "REI120"]);
                            }
                            _ => panic!("expected InExpr, got {:?}", p.value),
                        }
                    }
                    _ => panic!("expected property"),
                }
            }
            _ => panic!("expected CheckDecl"),
        }
    }

    #[test]
    fn test_parse_check_suite() {
        let source = concat!(
            "CheckSuite: Fire Safety\n",
            "  Check: min_door_width\n",
            "    severity: error\n",
            "    applies_to: Door\n",
            "    assert: width >= 0.90 m\n",
        );
        let file = parse(source).expect("should parse");
        assert_eq!(file.items.len(), 1);
        match &file.items[0] {
            Item::CheckSuite(cs) => {
                assert_eq!(cs.name, "Fire Safety");
                assert_eq!(cs.body.len(), 1);
                match &cs.body[0] {
                    BodyItem::CheckDecl(c) => {
                        assert_eq!(c.id, "min_door_width");
                    }
                    _ => panic!("expected CheckDecl in suite body"),
                }
            }
            _ => panic!("expected CheckSuite"),
        }
    }

    #[test]
    fn test_parse_axis_with_via() {
        let source = "### Storey: GF\n  Wall: W-ARC\n    from: (0, 0) via: (3, 2) to: (6, 0)\n    height: 3.0 m\n";
        let file = parse(source).expect("should parse");
        let h = match &file.items[0] {
            Item::Heading(h) => h,
            _ => panic!("expected heading"),
        };
        let wall = match &h.body[0] {
            BodyItem::Element(e) => e,
            _ => panic!("expected element"),
        };
        match &wall.body[0] {
            BodyItem::Property(p) => {
                assert_eq!(p.key, "from");
                match &p.value {
                    Value::Axis { from, to, via, .. } => {
                        assert_eq!(*from, (0.0, 0.0));
                        assert_eq!(*to, (6.0, 0.0));
                        assert_eq!(*via, Some((3.0, 2.0)));
                    }
                    _ => panic!("expected Axis, got {:?}", p.value),
                }
            }
            _ => panic!("expected property"),
        }
    }

    #[test]
    fn test_parse_polygon_with_holes() {
        let source = "### Storey: GF\n  Slab: S1\n    boundary: { outer: [(0,0), (10,0), (10,10), (0,10)], holes: [[(3,3), (7,3), (7,7), (3,7)]] }\n";
        let file = parse(source).expect("should parse");
        let h = match &file.items[0] {
            Item::Heading(h) => h,
            _ => panic!("expected heading"),
        };
        let slab = match &h.body[0] {
            BodyItem::Element(e) => e,
            _ => panic!("expected element"),
        };
        match &slab.body[0] {
            BodyItem::Property(p) => {
                assert_eq!(p.key, "boundary");
                match &p.value {
                    Value::PolygonWithHoles { outer, holes } => {
                        assert_eq!(outer.len(), 4);
                        assert_eq!(holes.len(), 1);
                        assert_eq!(holes[0].len(), 4);
                    }
                    _ => panic!("expected PolygonWithHoles, got {:?}", p.value),
                }
            }
            _ => panic!("expected property"),
        }
    }

    fn first_property_value(source: &str) -> Value {
        let file = parse(source).expect("should parse");
        let Item::Heading(h) = &file.items[0] else {
            panic!("expected heading")
        };
        let BodyItem::Element(e) = &h.body[0] else {
            panic!("expected element")
        };
        let BodyItem::Property(p) = &e.body[0] else {
            panic!("expected property")
        };
        p.value.clone()
    }

    #[test]
    fn smooth_and_nurbs_parse_to_curve_path() {
        let v = first_property_value(
            "### Storey: GF\n  Slab: S1\n    boundary: [(0, 0), smooth (10, -2), nurbs { degree: 2, points: [(10, -2), (14, 0), (18, -2)], weights: [1, 2, 1] }, (20, 5)]\n",
        );
        let Value::CurvePath { items, dim } = v else {
            panic!("expected CurvePath, got {v:?}")
        };
        assert_eq!(dim, 2);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], CurveItem::Corner(vec![0.0, 0.0]));
        assert_eq!(items[1], CurveItem::Smooth(vec![10.0, -2.0]));
        assert_eq!(
            items[2],
            CurveItem::Nurbs {
                degree: 2.0,
                points: vec![vec![10.0, -2.0], vec![14.0, 0.0], vec![18.0, -2.0]],
                weights: Some(vec![1.0, 2.0, 1.0]),
                knots: None,
            }
        );
        assert_eq!(items[3], CurveItem::Corner(vec![20.0, 5.0]));
    }

    #[test]
    fn smooth_3d_path_has_dim_3() {
        let v = first_property_value(
            "### Storey: GF\n  Duct: D1\n    path: [(0, 0, 3), smooth (2, 1, 3), (4, 0, 3.2)]\n",
        );
        let Value::CurvePath { items, dim } = v else {
            panic!("expected CurvePath, got {v:?}")
        };
        assert_eq!(dim, 3);
        assert_eq!(items[1], CurveItem::Smooth(vec![2.0, 1.0, 3.0]));
    }

    #[test]
    fn plain_polygon_unchanged() {
        let v = first_property_value(
            "### Storey: GF\n  Slab: S1\n    boundary: [(0, 0), (1, 0), (1, 1)]\n",
        );
        assert!(matches!(&v, Value::Polygon(p) if p == &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]));
        let v =
            first_property_value("### Storey: GF\n  Duct: D1\n    path: [(0, 0, 3), (2, 1, 3)]\n");
        assert!(matches!(v, Value::Path3D(p) if p.len() == 2));
    }

    #[test]
    fn smooth_hole_gives_curve_region() {
        let v = first_property_value(
            "### Storey: GF\n  Slab: S1\n    boundary: { outer: [(0,0), (10,0), (10,10), (0,10)], holes: [[(3,3), smooth (5,2), (7,3), (5,7)]] }\n",
        );
        let Value::CurveRegion { outer, holes } = v else {
            panic!("expected CurveRegion, got {v:?}")
        };
        assert_eq!(outer.len(), 4);
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0][1], CurveItem::Smooth(vec![5.0, 2.0]));
    }

    #[test]
    fn region_rings_must_be_2d() {
        let outer3d =
            "### Storey: GF\n  Slab: S1\n    boundary: { outer: [(0,0,1), (1,0,1), (1,1,1)] }\n";
        assert!(parse(outer3d).is_err());
        let hole3d = "### Storey: GF\n  Slab: S1\n    boundary: { outer: [(0,0), (9,0), (9,9)], holes: [[(1,1,1), smooth (2,1,1), (2,2,1)]] }\n";
        assert!(parse(hole3d).is_err());
    }

    #[test]
    fn mixed_dimension_nurbs_points_is_an_error() {
        let src = "### Storey: GF\n  Duct: D1\n    path: [(0, 0, 3), nurbs { degree: 1, points: [(0, 0, 3), (1, 1)] }]\n";
        assert!(parse(src).is_err());
        let src = "### Storey: GF\n  Duct: D1\n    path: [(0, 0, 3), smooth (1, 1)]\n";
        assert!(parse(src).is_err());
    }

    #[test]
    fn test_parse_path_expression() {
        let source = concat!(
            "Check: corridor_doors\n",
            "  applies_to: Door\n",
            "  where: host.space.usage in [corridor, staircase]\n",
            "  assert: width >= 0.90 m\n",
        );
        let file = parse(source).expect("should parse");
        match &file.items[0] {
            Item::CheckDecl(c) => match &c.body[1] {
                BodyItem::Property(p) => {
                    assert_eq!(p.key, "where");
                    match &p.value {
                        Value::InExpr { left, values } => {
                            match left.as_ref() {
                                Value::PathAccess(segments) => {
                                    assert_eq!(segments, &["host", "space", "usage"]);
                                }
                                _ => panic!("expected PathAccess"),
                            }
                            assert_eq!(values, &["corridor", "staircase"]);
                        }
                        _ => panic!("expected InExpr"),
                    }
                }
                _ => panic!("expected property"),
            },
            _ => panic!("expected CheckDecl"),
        }
    }

    #[test]
    fn test_parse_asset_library() {
        let source = concat!(
            "# AssetLibrary: Office Furniture\n",
            "  description: \"Standard collection\"\n",
            "\n",
            "## Asset: OfficeDesk\n",
            "  width: 1.6 m\n",
            "  depth: 0.8 m\n",
            "  height: 0.75 m\n",
            "\n",
            "### Part: Desktop\n",
            "  geometry: box(1.6, 0.8, 0.03)\n",
            "  position: (0, 0, 0.72)\n",
            "  material: oak_veneer\n",
            "\n",
            "### Part: LeftLeg\n",
            "  geometry: box(0.04, 0.6, 0.72)\n",
            "  position: (0.02, 0.1, 0)\n",
        );
        let file = parse(source).expect("should parse asset library");

        // Should have 1 item: AssetLibrary
        let asset_lib = match &file.items[0] {
            Item::AssetLibrary(lib) => lib,
            other => panic!(
                "expected AssetLibrary, got {:?}",
                std::mem::discriminant(other)
            ),
        };

        assert_eq!(asset_lib.name, "Office Furniture");
        assert_eq!(asset_lib.properties.len(), 1); // description
        assert_eq!(asset_lib.assets.len(), 1); // OfficeDesk

        let desk = &asset_lib.assets[0];
        assert_eq!(desk.id, "OfficeDesk");
        assert_eq!(desk.properties.len(), 3); // width, depth, height
        assert_eq!(desk.parts.len(), 2); // Desktop, LeftLeg

        let desktop = &desk.parts[0];
        assert_eq!(desktop.name, "Desktop");

        // Verify geometry primitive parsed
        let geom_prop = desktop
            .body
            .iter()
            .find_map(|bi| {
                if let BodyItem::Property(p) = bi {
                    if p.key == "geometry" {
                        return Some(p);
                    }
                }
                None
            })
            .expect("should have geometry property");
        assert!(
            matches!(&geom_prop.value, Value::GeometryPrimitive { geometry_type, args }
            if geometry_type == "box" && args.len() == 3)
        );

        // Verify 3D point parsed
        let pos_prop = desktop
            .body
            .iter()
            .find_map(|bi| {
                if let BodyItem::Property(p) = bi {
                    if p.key == "position" {
                        return Some(p);
                    }
                }
                None
            })
            .expect("should have position property");
        assert!(matches!(&pos_prop.value, Value::Point3D(x, y, z)
            if (*x - 0.0).abs() < 0.01 && (*y - 0.0).abs() < 0.01 && (*z - 0.72).abs() < 0.01));
    }

    #[test]
    fn test_parse_geometry_primitives() {
        let source = concat!(
            "# AssetLibrary: Geo Test\n",
            "## Asset: TestAsset\n",
            "### Part: BoxPart\n",
            "  geometry: box(1.0, 2.0, 3.0)\n",
            "### Part: CylPart\n",
            "  geometry: cylinder(0.5, 1.0)\n",
            "### Part: ExtPart\n",
            "  geometry: extrude([(0, 0), (1, 0), (1, 1), (0, 1)], 0.5)\n",
        );
        let file = parse(source).expect("should parse");
        let lib = match &file.items[0] {
            Item::AssetLibrary(lib) => lib,
            _ => panic!("expected AssetLibrary"),
        };
        assert_eq!(lib.assets[0].parts.len(), 3);

        // Box
        let box_geom = lib.assets[0].parts[0]
            .body
            .iter()
            .find_map(|bi| {
                if let BodyItem::Property(p) = bi {
                    if p.key == "geometry" {
                        return Some(&p.value);
                    }
                }
                None
            })
            .unwrap();
        assert!(
            matches!(box_geom, Value::GeometryPrimitive { geometry_type, args }
            if geometry_type == "box" && args.len() == 3)
        );

        // Cylinder
        let cyl_geom = lib.assets[0].parts[1]
            .body
            .iter()
            .find_map(|bi| {
                if let BodyItem::Property(p) = bi {
                    if p.key == "geometry" {
                        return Some(&p.value);
                    }
                }
                None
            })
            .unwrap();
        assert!(
            matches!(cyl_geom, Value::GeometryPrimitive { geometry_type, args }
            if geometry_type == "cylinder" && args.len() == 2)
        );

        // Extrude
        let ext_geom = lib.assets[0].parts[2]
            .body
            .iter()
            .find_map(|bi| {
                if let BodyItem::Property(p) = bi {
                    if p.key == "geometry" {
                        return Some(&p.value);
                    }
                }
                None
            })
            .unwrap();
        match ext_geom {
            Value::GeometryPrimitive {
                geometry_type,
                args,
            } => {
                assert_eq!(geometry_type, "extrude");
                assert_eq!(args.len(), 2); // polygon + number
                assert!(matches!(&args[0], Value::Polygon(pts) if pts.len() == 4));
            }
            _ => panic!("expected GeometryPrimitive"),
        }
    }

    #[test]
    fn test_parse_furniture_assets_fixture() {
        let source = std::fs::read_to_string(format!(
            "{}/tests/fixtures/furniture_assets.bml",
            env!("CARGO_MANIFEST_DIR").replace("/crates/bml-core", "")
        ))
        .unwrap();
        let file = parse(&source).expect("furniture_assets.bml should parse");

        // Should have: doc_comment, AssetLibrary, Site
        let asset_libs: Vec<_> = file
            .items
            .iter()
            .filter(|i| matches!(i, Item::AssetLibrary(_)))
            .collect();
        assert_eq!(asset_libs.len(), 1, "should have 1 asset library");

        let sites: Vec<_> = file
            .items
            .iter()
            .filter(|i| matches!(i, Item::Heading(h) if h.spatial_type == "Site"))
            .collect();
        assert_eq!(sites.len(), 1, "should have 1 site");
    }
}

/// Strip the quotes of a string literal and resolve its `\"` and `\\` escapes.
fn unquote(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    // Only `\"` and `\\` are escapes; any other backslash is kept, so a
    // Windows path like `"C:\dir\a.bml"` reads as written.
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(&n @ ('"' | '\\'))) => {
                out.push(n);
                chars.next();
            }
            (c, _) => out.push(c),
        }
    }
    out
}
