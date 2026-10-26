use bml_core::ast::nodes::{BodyItem, Item, SourceFile};
use bml_core::ast::span::Span;
use tower_lsp::lsp_types::*;

use crate::diagnostics::span_to_range;
use crate::document::DocumentState;

/// Provide go-to-definition for material references and element ID references.
pub fn goto_definition(doc: &DocumentState, position: Position) -> Option<Location> {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = doc.text.lines().collect();

    let line = lines.get(line_idx)?;
    let trimmed = line.trim();

    // Check if we're on a material reference (material: <id>)
    if let Some(colon_pos) = trimmed.find(':') {
        let key = trimmed[..colon_pos].trim();
        if key == "material" || key == "host" {
            let value = trimmed[colon_pos + 1..].trim();
            // Find the material/element definition in the AST
            if let Some(ast) = &doc.ast
                && let Some(span) = find_definition(ast, value)
            {
                return Some(Location {
                    uri: Url::parse("file:///").unwrap(), // placeholder — will be set by caller
                    range: span_to_range(&span),
                });
            }
        }

        // Check if we're on a storey reference (from_storey/to_storey)
        if key == "from_storey" || key == "to_storey" {
            let value = trimmed[colon_pos + 1..].trim();
            if let Some(ast) = &doc.ast
                && let Some(span) = find_storey_definition(ast, value)
            {
                return Some(Location {
                    uri: Url::parse("file:///").unwrap(),
                    range: span_to_range(&span),
                });
            }
        }
    }

    // Check if we're on an element ID — find its declaration
    let word = word_at_position(line, col)?;
    if let Some(ast) = &doc.ast
        && let Some(span) = find_element_by_id(ast, word)
    {
        return Some(Location {
            uri: Url::parse("file:///").unwrap(),
            range: span_to_range(&span),
        });
    }

    None
}

/// Find references to the symbol at the given position.
pub fn find_references(doc: &DocumentState, position: Position) -> Vec<Location> {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = doc.text.lines().collect();
    let mut locations = Vec::new();

    let line = lines.get(line_idx).copied().unwrap_or("");
    let word = match word_at_position(line, col) {
        Some(w) => w,
        None => return locations,
    };

    // Find all references to this identifier in the source text
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check element declarations
        if let Some(colon_pos) = trimmed.find(':') {
            let after_colon = trimmed[colon_pos + 1..].trim();
            if after_colon == word || after_colon.starts_with(&format!("{} ", word)) {
                let start_col = line.find(word).unwrap_or(0);
                locations.push(Location {
                    uri: Url::parse("file:///").unwrap(),
                    range: Range {
                        start: Position {
                            line: i as u32,
                            character: start_col as u32,
                        },
                        end: Position {
                            line: i as u32,
                            character: (start_col + word.len()) as u32,
                        },
                    },
                });
            }
        }

        // Check property value references
        if trimmed.contains(word)
            && let Some(colon_pos) = trimmed.find(':')
        {
            let value = trimmed[colon_pos + 1..].trim();
            let mut cols = Vec::new();
            if value == word {
                cols.push(line.rfind(word).unwrap_or(0));
            } else if let Some(items) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                // Ids in a list such as `under: [W-01, W-02]`.
                let mut col = line.rfind(value).unwrap_or(0) + 1;
                for item in items.split(',') {
                    if item.trim() == word {
                        cols.push(col + item.len() - item.trim_start().len());
                    }
                    col += item.len() + 1;
                }
            }
            for start_col in cols {
                locations.push(Location {
                    uri: Url::parse("file:///").unwrap(),
                    range: Range {
                        start: Position {
                            line: i as u32,
                            character: start_col as u32,
                        },
                        end: Position {
                            line: i as u32,
                            character: (start_col + word.len()) as u32,
                        },
                    },
                });
            }
        }
    }

    // Deduplicate
    locations.sort_by_key(|l| (l.range.start.line, l.range.start.character));
    locations.dedup_by_key(|l| (l.range.start.line, l.range.start.character));
    locations
}

fn find_definition(ast: &SourceFile, name: &str) -> Option<Span> {
    // Search for Material definition
    for item in &ast.items {
        if let Item::Material(mat) = item
            && mat.id == name
        {
            return Some(mat.span);
        }
    }

    // Search for element ID
    find_element_by_id(ast, name)
}

fn find_element_by_id(ast: &SourceFile, id: &str) -> Option<Span> {
    for item in &ast.items {
        match item {
            Item::Heading(heading) => {
                if let Some(span) = find_element_in_body(&heading.body, id) {
                    return Some(span);
                }
            }
            Item::Element(elem) => {
                if elem.id == id {
                    return Some(elem.span);
                }
                if let Some(span) = find_element_in_body(&elem.body, id) {
                    return Some(span);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_element_in_body(body: &[BodyItem], id: &str) -> Option<Span> {
    for item in body {
        if let BodyItem::Element(elem) = item {
            if elem.id == id {
                return Some(elem.span);
            }
            if let Some(span) = find_element_in_body(&elem.body, id) {
                return Some(span);
            }
        }
    }
    None
}

fn find_storey_definition(ast: &SourceFile, name: &str) -> Option<Span> {
    for item in &ast.items {
        if let Item::Heading(heading) = item
            && heading.spatial_type == "Storey"
            && heading.name == name
        {
            return Some(heading.span);
        }
    }
    None
}

fn word_at_position(line: &str, col: usize) -> Option<&str> {
    if col > line.len() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(&line[start..end])
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_references_in_list() {
        let text = "Wall: W-01\nWall: W-010\nFooting: FT-1\n  under: [W-010, W-01]\n";
        let doc = DocumentState::from_text(text.to_string());
        let refs = find_references(&doc, Position::new(0, 7));
        let found: Vec<(u32, u32)> = refs
            .iter()
            .map(|l| (l.range.start.line, l.range.start.character))
            .collect();
        assert_eq!(found, vec![(0, 6), (3, 17)]);
    }
}
