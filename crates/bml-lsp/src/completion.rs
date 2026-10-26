use bml_core::semantic::model::ElementType;
use bml_core::semantic::type_check;
use tower_lsp::lsp_types::*;

use crate::document::DocumentState;

/// Provide completions based on cursor position and document context.
pub fn provide_completions(doc: &DocumentState, position: Position) -> Vec<CompletionItem> {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = doc.text.lines().collect();

    if line_idx >= lines.len() {
        return spatial_and_element_completions();
    }

    let line = lines[line_idx];
    let text_before_cursor = if col <= line.len() {
        &line[..col]
    } else {
        line
    };
    let trimmed = text_before_cursor.trim_start();

    // If line starts with '#' -> spatial type completions
    if trimmed.starts_with('#') {
        return spatial_type_completions(trimmed);
    }

    // Detect if we're inside an element body by checking indentation context
    let indent_level = line.len() - line.trim_start().len();

    // If we're typing a key-value pair (has colon or is indented), offer property completions
    if indent_level >= 2 {
        // Try to find what element we're in by scanning backwards
        if let Some(element_type) = find_enclosing_element(&lines, line_idx) {
            if trimmed.is_empty() || !trimmed.contains(':') {
                // We're starting a new property — suggest properties for this element type
                return property_completions(element_type, &lines, line_idx);
            }
            // If we're after a colon, suggest values
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                let after_colon = trimmed[colon_pos + 1..].trim();
                return value_completions(element_type, key, after_colon);
            }
        }
    }

    // Default: suggest element types and spatial headings
    spatial_and_element_completions()
}

fn spatial_type_completions(prefix: &str) -> Vec<CompletionItem> {
    let hash_count = prefix.chars().take_while(|c| *c == '#').count();
    let spatial = match hash_count {
        1 => Some(("Site", "# Site: ")),
        2 => Some(("Building", "## Building: ")),
        3 => Some(("Storey", "### Storey: ")),
        4 => Some(("Space", "#### Space: ")),
        _ => None,
    };

    if let Some((label, insert)) = spatial {
        vec![CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(format!("IFC spatial hierarchy ({} hashes)", hash_count)),
            insert_text: Some(insert.to_string()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        }]
    } else {
        Vec::new()
    }
}

fn spatial_and_element_completions() -> Vec<CompletionItem> {
    let mut items = vec![
        CompletionItem {
            label: "# Site:".to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("IFC Site — top-level spatial container".into()),
            insert_text: Some("# Site: ${1:name}\n  location: ${2:lat}, ${3:lon}\n".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "## Building:".to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("IFC Building".into()),
            insert_text: Some("## Building: ${1:name}\n  type: ${2:residential}\n".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "### Storey:".to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("IFC Building Storey".into()),
            insert_text: Some("### Storey: ${1:name}\n  elevation: ${2:0.0} m\n".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "#### Space:".to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("IFC Space".into()),
            insert_text: Some("#### Space: ${1:name}\n  usage: ${2:living}\n".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
    ];

    items.extend(element_type_completions());
    items
}

fn element_type_completions() -> Vec<CompletionItem> {
    use bml_core::semantic::registry::ELEMENT_REGISTRY;

    let mut items: Vec<CompletionItem> = ELEMENT_REGISTRY
        .iter()
        .map(|desc| CompletionItem {
            label: desc.name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(format!("{} ({})", desc.ifc_entity, desc.name)),
            insert_text: Some(format!("{}\n", desc.completion_snippet)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect();

    // Material is not an element type but still needs a completion
    items.push(CompletionItem {
        label: "Material".to_string(),
        kind: Some(CompletionItemKind::CLASS),
        detail: Some("Material definition".into()),
        insert_text: Some(
            "Material: ${1:material_id}\n  type: ${2:homogeneous}\n  category: ${3:concrete}\n"
                .into(),
        ),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    });

    items
}

fn find_enclosing_element(lines: &[&str], current_line: usize) -> Option<ElementType> {
    let current_indent = lines
        .get(current_line)
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    // Scan backwards for the nearest element declaration at a lesser indent level
    for i in (0..current_line).rev() {
        let line = lines[i];
        let trimmed = line.trim();
        let indent = line.len() - trimmed.len();

        // Element declaration pattern: "<Type>: <ID>"
        if indent < current_indent {
            if let Some(colon_pos) = trimmed.find(':') {
                let type_name = trimmed[..colon_pos].trim();
                if let Some(et) = ElementType::parse(type_name) {
                    return Some(et);
                }
            }
            // If we hit a heading, we're at the spatial level (not inside an element)
            if trimmed.starts_with('#') {
                return None;
            }
        }
    }
    None
}

fn property_completions(
    element_type: ElementType,
    lines: &[&str],
    current_line: usize,
) -> Vec<CompletionItem> {
    // Collect already-used properties
    let mut existing_props = Vec::new();
    let base_indent = lines
        .get(current_line)
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(4);
    for i in (0..current_line).rev() {
        let line = lines[i];
        let indent = line.len() - line.trim_start().len();
        if indent < base_indent {
            break; // left the current element
        }
        if indent == base_indent {
            let trimmed = line.trim();
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                // Don't count element declarations as properties
                if ElementType::parse(key).is_none() {
                    existing_props.push(key.to_string());
                }
            }
        }
    }

    // Get all known properties for this element type
    let all_props = all_properties_for_type(element_type);

    all_props
        .into_iter()
        .filter(|(name, _, _)| !existing_props.contains(&name.to_string()))
        .map(|(name, detail, required)| {
            let sort_prefix = if required { "0" } else { "1" };
            CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(if required {
                    format!("{} (required)", detail)
                } else {
                    detail.to_string()
                }),
                insert_text: Some(format!("{}: ", name)),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                sort_text: Some(format!("{}{}", sort_prefix, name)),
                ..Default::default()
            }
        })
        .collect()
}

fn value_completions(element_type: ElementType, key: &str, _prefix: &str) -> Vec<CompletionItem> {
    if let Some(schema) = type_check::get_property_schema(element_type, key) {
        match schema.expected_type {
            type_check::ExpectedType::Boolean => {
                return vec![
                    CompletionItem {
                        label: "true".into(),
                        kind: Some(CompletionItemKind::VALUE),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "false".into(),
                        kind: Some(CompletionItemKind::VALUE),
                        ..Default::default()
                    },
                ];
            }
            type_check::ExpectedType::Enum(values)
            | type_check::ExpectedType::EnumOrString(values) => {
                return values
                    .iter()
                    .map(|v| CompletionItem {
                        label: v.to_string(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    })
                    .collect();
            }
            type_check::ExpectedType::Direction => {
                return vec![
                    CompletionItem {
                        label: "x".into(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "y".into(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    },
                ];
            }
            _ => {}
        }
    }

    // Check spatial property schemas too
    for spatial_type in &["Site", "Building", "Storey", "Space"] {
        if let Some(schema) = type_check::get_spatial_property_schema(spatial_type, key)
            && let type_check::ExpectedType::Enum(values) = schema.expected_type
        {
            return values
                .iter()
                .map(|v| CompletionItem {
                    label: v.to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    ..Default::default()
                })
                .collect();
        }
    }

    // Unit suggestions for numeric properties
    if let Some(schema) = type_check::get_property_schema(element_type, key) {
        return unit_completions(schema.expected_type);
    }

    Vec::new()
}

fn unit_completions(expected_type: &type_check::ExpectedType) -> Vec<CompletionItem> {
    let units: &[&str] = match expected_type {
        type_check::ExpectedType::Length => &["m", "cm", "mm", "ft", "in"],
        type_check::ExpectedType::Area => &["m2", "ft2"],
        type_check::ExpectedType::Thermal => &["W/m2K"],
        type_check::ExpectedType::Acoustic => &["dB"],
        type_check::ExpectedType::Density => &["kg/m3"],
        _ => return Vec::new(),
    };

    units
        .iter()
        .map(|u| CompletionItem {
            label: u.to_string(),
            kind: Some(CompletionItemKind::UNIT),
            detail: Some("unit".into()),
            ..Default::default()
        })
        .collect()
}

/// All known properties for an element type, with (name, description, is_required).
fn all_properties_for_type(element_type: ElementType) -> Vec<(&'static str, &'static str, bool)> {
    let desc = bml_core::semantic::registry::get_descriptor(element_type);
    desc.properties
        .iter()
        .map(|p| (p.name, p.description, p.required))
        .collect()
}
