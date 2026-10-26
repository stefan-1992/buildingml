use bml_core::semantic::model::ElementType;
use bml_core::semantic::type_check;
use tower_lsp::lsp_types::*;

use crate::document::DocumentState;

/// Provide hover information at the given position.
pub fn provide_hover(doc: &DocumentState, position: Position) -> Option<Hover> {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = doc.text.lines().collect();

    let line = lines.get(line_idx)?;

    // Find the word under the cursor
    let (word, word_start, word_end) = word_at(line, col)?;

    // Check if hovering over a spatial type keyword
    if let Some(info) = spatial_type_hover(word) {
        return Some(hover_result(info, line_idx, word_start, word_end));
    }

    // Check if hovering over an element type keyword
    if let Some(info) = element_type_hover(word) {
        return Some(hover_result(info, line_idx, word_start, word_end));
    }

    // Check if hovering over a property key (before the colon)
    let trimmed = line.trim();
    if let Some(colon_pos) = trimmed.find(':') {
        let key = trimmed[..colon_pos].trim();
        // Check if cursor is on the key
        let key_start_in_line = line.len() - trimmed.len();
        let key_end_in_line = key_start_in_line + key.len();
        if col >= key_start_in_line && col <= key_end_in_line {
            // This isn't an element declaration (element types start with uppercase)
            if !key.chars().next().is_some_and(|c| c.is_uppercase())
                || ElementType::parse(key).is_none()
            {
                // It's a property key — find the enclosing element
                if let Some(element_type) = find_enclosing_element_type(&lines, line_idx)
                    && let Some(info) = property_hover(element_type, key)
                {
                    return Some(hover_result(
                        info,
                        line_idx,
                        key_start_in_line,
                        key_end_in_line,
                    ));
                }

                // Try spatial properties
                if let Some(info) = spatial_property_hover(key) {
                    return Some(hover_result(
                        info,
                        line_idx,
                        key_start_in_line,
                        key_end_in_line,
                    ));
                }
            }
        }
    }

    // Check for unit hover
    if let Some(info) = unit_hover(word) {
        return Some(hover_result(info, line_idx, word_start, word_end));
    }

    None
}

fn hover_result(content: String, line: usize, start: usize, end: usize) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: content,
        }),
        range: Some(Range {
            start: Position {
                line: line as u32,
                character: start as u32,
            },
            end: Position {
                line: line as u32,
                character: end as u32,
            },
        }),
    }
}

fn word_at(line: &str, col: usize) -> Option<(&str, usize, usize)> {
    if col > line.len() {
        return None;
    }
    let bytes = line.as_bytes();

    // Find word boundaries
    let mut start = col;
    while start > 0 && is_word_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_char(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some((&line[start..end], start, end))
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'/' || b == b'#'
}

fn spatial_type_hover(word: &str) -> Option<String> {
    match word {
        "Site" => Some("**Site** (`IfcSite`)\n\nTop-level spatial container in the IFC hierarchy.\n\n**Properties:**\n- `location`: latitude, longitude\n- `address`: quoted string".into()),
        "Building" => Some("**Building** (`IfcBuilding`)\n\nA building within a site.\n\n**Properties:**\n- `type`: residential, commercial, industrial, etc.\n- `year`: construction year\n- `gross_floor_area`: total floor area".into()),
        "Storey" => Some("**Storey** (`IfcBuildingStorey`)\n\nA floor level within a building.\n\n**Properties:**\n- `elevation`: height above ground (m)\n- `height`: storey height (m)".into()),
        "Space" => Some("**Space** (`IfcSpace`)\n\nA room or functional area within a storey.\n\n**Properties:**\n- `usage`: living, bedroom, kitchen, etc.\n- `area`: floor area (m²)".into()),
        _ => None,
    }
}

fn element_type_hover(word: &str) -> Option<String> {
    // Special case: Material is not an element type
    if word == "Material" {
        return Some("**Material Definition**\n\nDefines a material that can be referenced by elements.\n\nTypes:\n- `homogeneous`: single material (concrete, steel, glass)\n- `layered`: multi-layer assembly (wall assemblies)".into());
    }
    let desc = bml_core::semantic::registry::get_descriptor_by_name(word)?;
    Some(desc.hover_text.to_string())
}

fn property_hover(element_type: ElementType, property_name: &str) -> Option<String> {
    let schema = type_check::get_property_schema(element_type, property_name)?;
    let type_desc = expected_type_description(schema.expected_type);
    let required = if schema.required {
        "Required"
    } else {
        "Optional"
    };

    let ifc_mapping = property_ifc_mapping(element_type, property_name);

    let mut info = format!(
        "**{}** on {}\n\n{} — {}\n\nExpected: {}",
        property_name,
        element_type.as_str(),
        required,
        property_description(element_type, property_name),
        type_desc,
    );

    if let Some(mapping) = ifc_mapping {
        info.push_str(&format!("\n\n**IFC:** {}", mapping));
    }

    Some(info)
}

fn spatial_property_hover(property_name: &str) -> Option<String> {
    let descriptions: &[(&str, &str)] = &[
        ("location", "Geographic coordinates (latitude, longitude)"),
        ("address", "Postal address of the site"),
        (
            "elevation",
            "Height above the reference level (typically ground)",
        ),
        ("height", "Floor-to-floor height of the storey"),
        ("usage", "Functional usage of the space"),
        ("area", "Floor area of the space"),
        ("type", "Classification type"),
        ("year", "Year of construction"),
        ("gross_floor_area", "Total gross floor area of the building"),
    ];

    let desc = descriptions.iter().find(|(n, _)| *n == property_name)?;
    Some(format!("**{}**\n\n{}", property_name, desc.1))
}

fn expected_type_description(expected: &type_check::ExpectedType) -> &'static str {
    match expected {
        type_check::ExpectedType::Length => "number with optional length unit (m, cm, mm)",
        type_check::ExpectedType::Area => "number with optional area unit (m², ft²)",
        type_check::ExpectedType::Thermal => "number with optional thermal unit (W/m²K)",
        type_check::ExpectedType::Acoustic => "number with optional acoustic unit (dB)",
        type_check::ExpectedType::Density => "number with optional density unit (kg/m³)",
        type_check::ExpectedType::BareNumber => "number",
        type_check::ExpectedType::Boolean => "`true` or `false`",
        type_check::ExpectedType::MaterialRef => "material identifier",
        type_check::ExpectedType::StoreyRef => "storey name",
        type_check::ExpectedType::Identifier => "identifier",
        type_check::ExpectedType::Enum(_) => "enum value",
        type_check::ExpectedType::EnumOrString(_) => "enum value or quoted string",
        type_check::ExpectedType::Point => "point `(x, y)`",
        type_check::ExpectedType::Polygon => "polygon `[(x,y), ...]`",
        type_check::ExpectedType::Profile => "profile (e.g., `rectangle(0.3, 0.3)`)",
        type_check::ExpectedType::Axis => "axis `(x1, y1) to: (x2, y2)`",
        type_check::ExpectedType::String => "quoted string",
        type_check::ExpectedType::NumberPair => "number pair (e.g., `49.45, 12.18`)",
        type_check::ExpectedType::Direction => "`x` or `y`",
        type_check::ExpectedType::PolygonWithHoles => "polygon with holes `[[(x,y), ...]]`",
        type_check::ExpectedType::PolygonOrAuto => "polygon `[(x,y), ...]` or `auto`",
        type_check::ExpectedType::PolygonOrHoles => {
            "polygon `[(x,y), ...]` or `{ outer: [...], holes: [[...]] }`"
        }
        type_check::ExpectedType::PolygonOrHolesOrAuto => {
            "polygon `[(x,y), ...]`, polygon with holes, or `auto`"
        }
        type_check::ExpectedType::ElementRefs => "element id or list `[A-01, A-02]`",
        type_check::ExpectedType::Lengths => "length or list `[1.1 m, 3.4 m]`",
        type_check::ExpectedType::Point3D => "3D point `(x, y, z)`",
        type_check::ExpectedType::Path3D => "3D path `[(x, y, z), ...]`",
    }
}

fn property_description(element_type: ElementType, property_name: &str) -> &'static str {
    use ElementType::*;
    match (element_type, property_name) {
        (Wall, "from") => "wall axis line (start and end points)",
        (Wall, "height") => "wall height",
        (Wall, "thickness") => "wall thickness",
        (Wall, "material") => "reference to a material definition",
        (Wall, "is_external") => "whether the wall is an exterior wall",
        (Wall, "fire_rating") => "fire resistance rating",
        (Wall, "thermal_transmittance") => "thermal transmittance (U-value)",
        (Wall, "load_bearing") => "whether the wall is structural",
        (Wall, "acoustic_rating") => "sound insulation rating",
        (Slab, "boundary") => "slab boundary polygon",
        (Slab, "thickness") => "slab thickness",
        (Slab, "slab_type") => "slab function: floor, roof, or landing",
        (Column, "position") => "column center position",
        (Column, "profile") => "column cross-section profile",
        (Column, "height") => "column height",
        (Window, "offset") => "distance along wall axis from start point",
        (Window, "width") => "window width",
        (Window, "height") => "window height",
        (Window, "sill_height") => "height of window sill from floor",
        (Window, "type") => "glazing type",
        (Window, "u_value") => "thermal transmittance",
        (Door, "offset") => "distance along wall axis from start point",
        (Door, "width") => "door width",
        (Door, "height") => "door height",
        (Door, "type") => "door leaf type",
        (Door, "is_external") => "whether the door is an exterior door",
        (Door, "fire_rating") => "fire resistance rating",
        _ => "element property",
    }
}

fn property_ifc_mapping(element_type: ElementType, property_name: &str) -> Option<&'static str> {
    use ElementType::*;
    match (element_type, property_name) {
        (Wall, "is_external") => Some("Pset_WallCommon.IsExternal"),
        (Wall, "fire_rating") => Some("Pset_WallCommon.FireRating"),
        (Wall, "thermal_transmittance") => Some("Pset_WallCommon.ThermalTransmittance"),
        (Wall, "load_bearing") => Some("Pset_WallCommon.LoadBearing"),
        (Wall, "acoustic_rating") => Some("Pset_WallCommon.AcousticRating"),
        (Door, "is_external") => Some("Pset_DoorCommon.IsExternal"),
        (Door, "fire_rating") => Some("Pset_DoorCommon.FireRating"),
        (Window, "u_value") => Some("Pset_WindowCommon.ThermalTransmittance"),
        _ => None,
    }
}

fn find_enclosing_element_type(lines: &[&str], current_line: usize) -> Option<ElementType> {
    let current_indent = lines
        .get(current_line)
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    for i in (0..current_line).rev() {
        let line = lines[i];
        let trimmed = line.trim();
        let indent = line.len() - trimmed.len();

        if indent < current_indent {
            if let Some(colon_pos) = trimmed.find(':') {
                let type_name = trimmed[..colon_pos].trim();
                if let Some(et) = ElementType::parse(type_name) {
                    return Some(et);
                }
            }
            if trimmed.starts_with('#') {
                return None;
            }
        }
    }
    None
}

fn unit_hover(word: &str) -> Option<String> {
    match word {
        "m" => Some("**Meter** — SI base unit of length".into()),
        "cm" => Some("**Centimeter** — 0.01 meters".into()),
        "mm" => Some("**Millimeter** — 0.001 meters".into()),
        "m2" => Some("**Square meter** — SI unit of area".into()),
        "m3" => Some("**Cubic meter** — SI unit of volume".into()),
        "ft" => Some("**Foot** — 0.3048 meters".into()),
        "W/m2K" => {
            Some("**Watts per square meter kelvin** — thermal transmittance (U-value)".into())
        }
        "dB" => Some("**Decibel** — acoustic measurement unit".into()),
        "kg/m3" => Some("**Kilograms per cubic meter** — density unit".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_at() {
        let line = "    height: 2.80 m";
        let (word, start, end) = word_at(line, 6).unwrap();
        assert_eq!(word, "height");
        assert_eq!(start, 4);
        assert_eq!(end, 10);
    }

    #[test]
    fn test_spatial_type_hover() {
        assert!(spatial_type_hover("Site").is_some());
        assert!(spatial_type_hover("Unknown").is_none());
    }

    #[test]
    fn test_element_type_hover() {
        assert!(element_type_hover("Wall").is_some());
        assert!(element_type_hover("Door").is_some());
        assert!(element_type_hover("Xyz").is_none());
    }
}
