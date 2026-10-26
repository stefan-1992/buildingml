//! Template expansion pass.
//!
//! Expands `apply_template:` directives in storeys by substituting
//! template bodies with variable replacements. This pass runs between
//! parsing and semantic analysis.

use std::collections::{HashMap, HashSet};

use crate::ast::nodes::*;

/// Expand all templates in a parsed source file.
///
/// This finds all `Template:` definitions, then processes each storey heading
/// that has an `apply_template:` property. The template body is cloned,
/// variables are substituted, and the resulting elements are injected into
/// the storey body. Template space headings (#### Space:) that follow a
/// Template: definition are expanded as proper space headings under the
/// target storey.
pub fn expand_templates(source_file: &mut SourceFile) {
    // 1. Collect template definitions
    let templates: HashMap<String, TemplateDefinition> = source_file
        .items
        .iter()
        .filter_map(|item| {
            if let Item::Template(t) = item {
                Some((t.id.clone(), t.clone()))
            } else {
                None
            }
        })
        .collect();

    if templates.is_empty() {
        return;
    }

    // 2. Collect space headings that follow templates (level 4 headings
    //    that appear between a Template: and the next non-space heading).
    //    Space headings inside templates are parsed as top-level headings,
    //    so we associate them with their template.
    let template_spaces = collect_template_spaces(&source_file.items);

    // 3. Collect indices of template space headings so we can remove them
    let template_space_indices = collect_template_space_indices(&source_file.items);

    // 4. Process each storey heading for apply_template, skip template spaces
    let mut new_items = Vec::new();
    for (i, item) in source_file.items.iter().enumerate() {
        // Skip space headings that belong to a template definition
        if template_space_indices.contains(&i) {
            continue;
        }
        match item {
            Item::Heading(heading) if heading.spatial_type == "Storey" => {
                let mut heading = heading.clone();
                let expanded_spaces =
                    apply_template_to_storey(&mut heading, &templates, &template_spaces);
                new_items.push(Item::Heading(heading));
                // Insert expanded space headings right after the storey heading
                for space in expanded_spaces {
                    new_items.push(Item::Heading(space));
                }
            }
            _ => {
                new_items.push(item.clone());
            }
        }
    }

    source_file.items = new_items;
}

/// Collect indices of level-4 headings that follow Template: definitions.
fn collect_template_space_indices(items: &[Item]) -> HashSet<usize> {
    let mut indices = HashSet::new();
    let mut in_template = false;

    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Template(_) => {
                in_template = true;
            }
            Item::Heading(h) if h.level == 4 && in_template => {
                indices.insert(i);
            }
            Item::Heading(h) if h.level <= 3 => {
                in_template = false;
            }
            _ => {}
        }
    }

    indices
}

/// Collect space headings (level 4) that follow each Template: definition.
/// Returns a map from template ID to list of space headings.
fn collect_template_spaces(items: &[Item]) -> HashMap<String, Vec<Heading>> {
    let mut result: HashMap<String, Vec<Heading>> = HashMap::new();
    let mut current_template: Option<String> = None;

    for item in items {
        match item {
            Item::Template(t) => {
                current_template = Some(t.id.clone());
                result.entry(t.id.clone()).or_default();
            }
            Item::Heading(h) if h.level == 4 && current_template.is_some() => {
                // This is a #### Space: heading following a Template: — associate it
                if let Some(ref tid) = current_template {
                    result.entry(tid.clone()).or_default().push(h.clone());
                }
            }
            Item::Heading(h) if h.level <= 3 => {
                // A non-space heading ends the template's space collection
                current_template = None;
            }
            _ => {}
        }
    }

    result
}

/// Apply template to a storey if it has an `apply_template:` property.
///
/// Returns a list of expanded space headings that should be inserted as
/// top-level items after the storey heading. Template body items (elements,
/// properties) are added directly to the storey body; template space
/// headings are returned separately so they appear as proper #### Space:
/// headings under the storey.
fn apply_template_to_storey(
    heading: &mut Heading,
    templates: &HashMap<String, TemplateDefinition>,
    template_spaces: &HashMap<String, Vec<Heading>>,
) -> Vec<Heading> {
    // Find apply_template property and variable bindings
    let mut template_name = None;
    let mut variables: HashMap<String, String> = HashMap::new();
    let mut apply_index = None;

    for (i, item) in heading.body.iter().enumerate() {
        if let BodyItem::Property(prop) = item
            && prop.key == "apply_template"
        {
            template_name = Some(match &prop.value {
                Value::Identifier(id) => id.clone(),
                Value::String(s) => s.clone(),
                _ => continue,
            });
            apply_index = Some(i);
        }
    }

    let template_name = match template_name {
        Some(name) => name,
        None => return Vec::new(),
    };

    // Collect variable bindings: properties that come after apply_template
    // In the grammar, variable bindings appear as indented properties under apply_template.
    // Since we parse them as regular properties, collect all properties after apply_template
    // that look like variable bindings (short, single-word keys).
    if let Some(idx) = apply_index {
        // Look for variable bindings after the apply_template property
        for item in heading.body.iter().skip(idx + 1) {
            if let BodyItem::Property(prop) = item {
                // Treat remaining simple identifier properties as variable bindings
                let val = match &prop.value {
                    Value::Identifier(v) => v.clone(),
                    Value::String(v) => v.clone(),
                    Value::Number(n) => format!("{}", n),
                    _ => continue,
                };
                variables.insert(prop.key.clone(), val);
            }
        }
    }

    // Look up the template
    let template = match templates.get(&template_name) {
        Some(t) => t,
        None => return Vec::new(), // Template not found — will be caught by semantic analysis
    };

    // Clone and substitute template body items (storey-level elements)
    let mut expanded_items: Vec<BodyItem> = Vec::new();
    for item in &template.body {
        expanded_items.push(substitute_body_item(item, &variables));
    }

    // Clone and substitute template space headings — these are returned
    // as top-level headings to be inserted after the storey
    let mut expanded_spaces = Vec::new();
    if let Some(space_headings) = template_spaces.get(&template_name) {
        for space in space_headings {
            expanded_spaces.push(substitute_heading(space, &variables));
        }
    }

    // Rebuild the body: keep pre-template items, inject expanded, then keep remaining
    if let Some(idx) = apply_index {
        // Keep everything before apply_template
        let mut new_body: Vec<BodyItem> = heading.body[..idx].to_vec();
        // Add expanded template body items (storey-level elements)
        new_body.extend(expanded_items);
        // Keep items after apply_template that are NOT variable bindings
        // (elements, comments, and non-binding properties)
        for item in heading.body.iter().skip(idx + 1) {
            match item {
                BodyItem::Property(prop) if variables.contains_key(&prop.key) => {
                    // Skip variable binding properties
                }
                other => {
                    new_body.push(other.clone());
                }
            }
        }
        heading.body = new_body;
    }

    expanded_spaces
}

/// Substitute {variables} in a body item.
fn substitute_body_item(item: &BodyItem, variables: &HashMap<String, String>) -> BodyItem {
    match item {
        BodyItem::Property(prop) => BodyItem::Property(Property {
            key: substitute_vars(&prop.key, variables),
            value: substitute_value(&prop.value, variables),
            span: prop.span,
        }),
        BodyItem::Element(elem) => BodyItem::Element(substitute_element(elem, variables)),
        BodyItem::CheckDecl(c) => BodyItem::CheckDecl(c.clone()),
        BodyItem::Comment(c) => BodyItem::Comment(c.clone()),
    }
}

/// Substitute {variables} in an element declaration.
fn substitute_element(
    elem: &ElementDeclaration,
    variables: &HashMap<String, String>,
) -> ElementDeclaration {
    ElementDeclaration {
        element_type: elem.element_type.clone(),
        id: substitute_vars(&elem.id, variables),
        body: elem
            .body
            .iter()
            .map(|item| substitute_body_item(item, variables))
            .collect(),
        span: elem.span,
    }
}

/// Substitute {variables} in a heading.
fn substitute_heading(heading: &Heading, variables: &HashMap<String, String>) -> Heading {
    Heading {
        level: heading.level,
        spatial_type: heading.spatial_type.clone(),
        name: substitute_vars(&heading.name, variables),
        body: heading
            .body
            .iter()
            .map(|item| substitute_body_item(item, variables))
            .collect(),
        span: heading.span,
    }
}

/// Substitute {variables} in a value.
fn substitute_value(value: &Value, variables: &HashMap<String, String>) -> Value {
    match value {
        Value::String(s) => Value::String(substitute_vars(s, variables)),
        Value::Identifier(id) => Value::Identifier(substitute_vars(id, variables)),
        // Other value types don't contain text that needs substitution
        other => other.clone(),
    }
}

/// Replace `{name}` patterns in a string with variable values.
fn substitute_vars(text: &str, variables: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    for (key, value) in variables {
        let pattern = format!("{{{}}}", key);
        result = result.replace(&pattern, value);
    }
    result
}
