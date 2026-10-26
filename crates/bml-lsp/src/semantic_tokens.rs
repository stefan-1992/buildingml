use bml_core::ast::nodes::{BodyItem, Item, SourceFile, Value};
use tower_lsp::lsp_types::*;

/// Semantic token types used by BuildingML.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE, // 0: spatial types (Site, Building, etc.)
    SemanticTokenType::TYPE,      // 1: element types (Wall, Slab, etc.)
    SemanticTokenType::VARIABLE,  // 2: element IDs
    SemanticTokenType::PROPERTY,  // 3: property keys
    SemanticTokenType::NUMBER,    // 4: numeric values
    SemanticTokenType::STRING,    // 5: string values
    SemanticTokenType::KEYWORD,   // 6: keywords (import, from, to, true, false)
    SemanticTokenType::COMMENT,   // 7: comments
    SemanticTokenType::ENUM_MEMBER, // 8: enum/identifier values
    SemanticTokenType::new("unit"), // 9: units (m, cm, mm, etc.)
    SemanticTokenType::new("heading"), // 10: heading markers (#, ##, etc.)
    SemanticTokenType::new("material"), // 11: material definitions
];

/// Semantic token modifiers.
pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DEFINITION,    // 0: definition site
    SemanticTokenModifier::DECLARATION,   // 1: declaration
    SemanticTokenModifier::DOCUMENTATION, // 2: doc comment
];

/// Generate semantic tokens for the document.
pub fn semantic_tokens(ast: &SourceFile, source: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for item in &ast.items {
        match item {
            Item::Heading(heading) => {
                let line = heading.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");

                // Heading marker (# symbols)
                let hash_end = line_text.chars().take_while(|c| *c == '#').count();
                if hash_end > 0 {
                    tokens.push(make_token(line as u32, 0, hash_end as u32, 10, 0));
                }

                // Spatial type
                if let Some(pos) = line_text.find(&heading.spatial_type) {
                    tokens.push(make_token(
                        line as u32,
                        pos as u32,
                        heading.spatial_type.len() as u32,
                        0,
                        1,
                    ));
                }

                // Heading name
                if let Some(colon_pos) = line_text.find(':') {
                    let name_start = colon_pos + 1;
                    let name = line_text[name_start..].trim();
                    if !name.is_empty() {
                        let actual_start =
                            name_start + line_text[name_start..].find(name).unwrap_or(0);
                        tokens.push(make_token(
                            line as u32,
                            actual_start as u32,
                            name.len() as u32,
                            2,
                            1,
                        ));
                    }
                }

                // Body items
                tokenize_body(&heading.body, &lines, &mut tokens);
            }
            Item::Element(elem) => {
                tokenize_element_declaration(elem, &lines, &mut tokens);
                tokenize_body(&elem.body, &lines, &mut tokens);
            }
            Item::Material(mat) => {
                let line = mat.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");

                // "Material" keyword
                if let Some(pos) = line_text.find("Material") {
                    tokens.push(make_token(line as u32, pos as u32, 8, 11, 1));
                }
                // Material ID
                if let Some(colon_pos) = line_text.find(':') {
                    let id_start = colon_pos + 1;
                    let id = line_text[id_start..].trim();
                    if !id.is_empty() {
                        let actual_start = id_start + line_text[id_start..].find(id).unwrap_or(0);
                        tokens.push(make_token(
                            line as u32,
                            actual_start as u32,
                            id.len() as u32,
                            2,
                            0,
                        ));
                    }
                }

                tokenize_body(&mat.body, &lines, &mut tokens);
            }
            Item::Import(import) => {
                let line = import.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");

                // "import" keyword
                if let Some(pos) = line_text.find("import") {
                    tokens.push(make_token(line as u32, pos as u32, 6, 6, 0));
                }
                // "from" keyword
                if let Some(pos) = line_text.find("from") {
                    tokens.push(make_token(line as u32, pos as u32, 4, 6, 0));
                }
                // Path string
                if let Some(pos) = line_text.find('"') {
                    let end = line_text[pos + 1..]
                        .find('"')
                        .map(|e| e + pos + 2)
                        .unwrap_or(line_text.len());
                    tokens.push(make_token(
                        line as u32,
                        pos as u32,
                        (end - pos) as u32,
                        5,
                        0,
                    ));
                }
            }
            Item::CheckSuite(cs) => {
                tokenize_body(&cs.body, &lines, &mut tokens);
            }
            Item::CheckDecl(c) => {
                tokenize_body(&c.body, &lines, &mut tokens);
            }
            Item::Template(t) => {
                tokenize_body(&t.body, &lines, &mut tokens);
            }
            Item::AssetLibrary(_) => {
                // Asset library headings are handled via the heading path
            }
            Item::Comment(comment) => {
                let line = comment.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");
                let modifier = if comment.is_doc { 2 } else { 0 };
                tokens.push(make_token(
                    line as u32,
                    0,
                    line_text.len() as u32,
                    7,
                    modifier,
                ));
            }
        }
    }

    // Sort by line then column
    tokens.sort_by_key(|t| (t.delta_line, t.delta_start));

    // Convert to delta encoding
    to_delta_encoding(tokens)
}

fn tokenize_body(body: &[BodyItem], lines: &[&str], tokens: &mut Vec<SemanticToken>) {
    for item in body {
        match item {
            BodyItem::Property(prop) => {
                let line = prop.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");
                let trimmed = line_text.trim_start();
                let indent = line_text.len() - trimmed.len();

                // Property key
                if let Some(colon_pos) = trimmed.find(':') {
                    let key = trimmed[..colon_pos].trim();
                    tokens.push(make_token(
                        line as u32,
                        indent as u32,
                        key.len() as u32,
                        3,
                        0,
                    ));

                    // Property value
                    tokenize_value(&prop.value, line as u32, line_text, tokens);
                }
            }
            BodyItem::Element(elem) => {
                tokenize_element_declaration(elem, lines, tokens);
                tokenize_body(&elem.body, lines, tokens);
            }
            BodyItem::CheckDecl(c) => {
                tokenize_body(&c.body, lines, tokens);
            }
            BodyItem::Comment(comment) => {
                let line = comment.span.start.line.saturating_sub(1);
                let line_text = lines.get(line).copied().unwrap_or("");
                let modifier = if comment.is_doc { 2 } else { 0 };
                tokens.push(make_token(
                    line as u32,
                    0,
                    line_text.len() as u32,
                    7,
                    modifier,
                ));
            }
        }
    }
}

fn tokenize_element_declaration(
    elem: &bml_core::ast::nodes::ElementDeclaration,
    lines: &[&str],
    tokens: &mut Vec<SemanticToken>,
) {
    let line = elem.span.start.line.saturating_sub(1);
    let line_text = lines.get(line).copied().unwrap_or("");
    let trimmed = line_text.trim_start();
    let indent = line_text.len() - trimmed.len();

    // Element type
    if let Some(colon_pos) = trimmed.find(':') {
        let type_name = trimmed[..colon_pos].trim();
        tokens.push(make_token(
            line as u32,
            indent as u32,
            type_name.len() as u32,
            1,
            1,
        ));

        // Element ID
        let id = trimmed[colon_pos + 1..].trim();
        if !id.is_empty() {
            let id_start = indent + colon_pos + 1 + trimmed[colon_pos + 1..].find(id).unwrap_or(0);
            tokens.push(make_token(
                line as u32,
                id_start as u32,
                id.len() as u32,
                2,
                0,
            ));
        }
    }
}

fn tokenize_value(value: &Value, line: u32, line_text: &str, tokens: &mut Vec<SemanticToken>) {
    match value {
        Value::Number(n) => {
            let s = format_number(*n);
            if let Some(pos) = find_value_position(line_text, &s) {
                tokens.push(make_token(line, pos as u32, s.len() as u32, 4, 0));
            }
        }
        Value::NumberWithUnit(n, unit) => {
            let s = format_number(*n);
            if let Some(pos) = find_value_position(line_text, &s) {
                tokens.push(make_token(line, pos as u32, s.len() as u32, 4, 0));
            }
            if let Some(pos) = line_text.rfind(unit.as_str()) {
                tokens.push(make_token(line, pos as u32, unit.len() as u32, 9, 0));
            }
        }
        Value::Boolean(b) => {
            let s = if *b { "true" } else { "false" };
            if let Some(pos) = find_value_position(line_text, s) {
                tokens.push(make_token(line, pos as u32, s.len() as u32, 6, 0));
            }
        }
        Value::String(s) => {
            // Find the quoted string
            let quoted = format!("\"{}\"", s);
            if let Some(pos) = line_text.find(&quoted) {
                tokens.push(make_token(line, pos as u32, quoted.len() as u32, 5, 0));
            }
        }
        Value::Identifier(id) => {
            if let Some(pos) = find_value_position(line_text, id) {
                tokens.push(make_token(line, pos as u32, id.len() as u32, 8, 0));
            }
        }
        _ => {} // Complex values (points, polygons, etc.) — skip detailed tokenization
    }
}

fn format_number(n: f64) -> String {
    if n == n.floor() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn find_value_position(line: &str, value: &str) -> Option<usize> {
    // Find value after the colon
    if let Some(colon_pos) = line.find(':') {
        line[colon_pos + 1..].find(value).map(|p| p + colon_pos + 1)
    } else {
        line.find(value)
    }
}

fn make_token(
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    token_modifiers: u32,
) -> SemanticToken {
    // Store absolute positions first, convert to deltas later
    SemanticToken {
        delta_line: line,
        delta_start: start,
        length,
        token_type,
        token_modifiers_bitset: token_modifiers,
    }
}

/// Convert absolute-positioned tokens to delta-encoded tokens.
fn to_delta_encoding(tokens: Vec<SemanticToken>) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in tokens {
        let delta_line = token.delta_line - prev_line;
        let delta_start = if delta_line == 0 {
            token.delta_start - prev_start
        } else {
            token.delta_start
        };

        prev_line = token.delta_line;
        prev_start = token.delta_start;

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.token_modifiers_bitset,
        });
    }

    result
}
