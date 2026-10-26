use tower_lsp::lsp_types::*;

/// Format a BuildingML document.
///
/// Formatting rules:
/// - Headings: no leading whitespace, one space after # markers
/// - Element declarations: indented 2 spaces per nesting level
/// - Properties: indented 2 more spaces than their parent element
/// - Blank lines between top-level items (headings, materials)
/// - Consistent 2-space indentation
/// - Trim trailing whitespace
/// - Single newline at end of file
pub fn format_document(text: &str) -> Vec<TextEdit> {
    let lines: Vec<&str> = text.lines().collect();
    let formatted = format_lines(&lines);

    // If no changes, return empty
    if formatted == text {
        return Vec::new();
    }

    // Replace entire document
    let line_count = lines.len();
    let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0);

    vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: line_count as u32,
                character: last_line_len as u32,
            },
        },
        new_text: formatted,
    }]
}

fn format_lines(lines: &[&str]) -> String {
    let mut result = Vec::new();
    let mut prev_was_blank = false;
    let mut prev_was_heading = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Handle blank lines
        if trimmed.is_empty() {
            if !prev_was_blank && i > 0 {
                result.push(String::new());
                prev_was_blank = true;
            }
            prev_was_heading = false;
            continue;
        }

        prev_was_blank = false;

        // Heading: no indent, normalize spacing
        if trimmed.starts_with('#') {
            // Add blank line before heading (except at start of file)
            if !result.is_empty()
                && !prev_was_heading
                && result
                    .last()
                    .map(|l: &String| !l.is_empty())
                    .unwrap_or(false)
            {
                result.push(String::new());
            }
            result.push(format_heading(trimmed));
            prev_was_heading = true;
            continue;
        }

        prev_was_heading = false;

        // Import statements: no indent
        if trimmed.starts_with("import ") {
            result.push(format_import(trimmed));
            continue;
        }

        // Comment at column 0
        if trimmed.starts_with("//") && !line.starts_with(' ') {
            result.push(trimmed.to_string());
            continue;
        }

        // Material at top level
        if trimmed.starts_with("Material:") && !line.starts_with(' ') {
            if !result.is_empty()
                && result
                    .last()
                    .map(|l: &String| !l.is_empty())
                    .unwrap_or(false)
            {
                result.push(String::new());
            }
            result.push(trimmed.to_string());
            continue;
        }

        // Indented content: preserve relative indentation but normalize to 2-space
        let current_indent = line.len() - trimmed.len();
        let normalized_indent = normalize_indent(current_indent);
        result.push(format!("{}{}", " ".repeat(normalized_indent), trimmed));
    }

    // Ensure single trailing newline
    let mut output = result.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn format_heading(trimmed: &str) -> String {
    let hash_count = trimmed.chars().take_while(|c| *c == '#').count();
    let rest = trimmed[hash_count..].trim();
    // Collapse multiple internal spaces to single space
    let normalized: String = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{} {}", "#".repeat(hash_count), normalized)
}

fn format_import(trimmed: &str) -> String {
    // Normalize: import <name> from "<path>"
    trimmed.to_string()
}

/// Normalize indentation to multiples of 2 spaces.
fn normalize_indent(spaces: usize) -> usize {
    // Round to nearest 2-space boundary
    spaces.div_ceil(2) * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_heading() {
        assert_eq!(format_heading("#Site: Foo"), "# Site: Foo");
        assert_eq!(format_heading("## Building:  House"), "## Building: House");
    }

    #[test]
    fn test_normalize_indent() {
        assert_eq!(normalize_indent(0), 0);
        assert_eq!(normalize_indent(1), 2);
        assert_eq!(normalize_indent(2), 2);
        assert_eq!(normalize_indent(3), 4);
        assert_eq!(normalize_indent(4), 4);
    }

    #[test]
    fn test_format_basic_document() {
        let input = "#  Site: Test\n  location: 49.45, 12.18\n";
        let edits = format_document(input);
        assert!(!edits.is_empty());
        assert!(edits[0].new_text.starts_with("# Site: Test"));
    }
}
