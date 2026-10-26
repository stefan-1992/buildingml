use bml_core::ast::nodes::{BodyItem, Item, SourceFile};
use tower_lsp::lsp_types::*;

use crate::diagnostics::span_to_range;

/// Generate document symbols showing the spatial hierarchy and elements.
#[allow(deprecated)] // DocumentSymbol::deprecated field is deprecated but required
pub fn document_symbols(ast: &SourceFile) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    for item in &ast.items {
        match item {
            Item::Heading(heading) => {
                let kind = match heading.spatial_type.as_str() {
                    "Site" => SymbolKind::NAMESPACE,
                    "Building" => SymbolKind::MODULE,
                    "Storey" => SymbolKind::CLASS,
                    "Space" => SymbolKind::METHOD,
                    _ => SymbolKind::STRUCT,
                };

                let children = body_symbols(&heading.body);
                let range = span_to_range(&heading.span);

                symbols.push(DocumentSymbol {
                    name: format!("{}: {}", heading.spatial_type, heading.name),
                    detail: Some(heading.spatial_type.clone()),
                    kind,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                });
            }
            Item::Material(mat) => {
                let range = span_to_range(&mat.span);
                symbols.push(DocumentSymbol {
                    name: format!("Material: {}", mat.id),
                    detail: Some("Material".into()),
                    kind: SymbolKind::CONSTANT,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            Item::Import(import) => {
                let range = span_to_range(&import.span);
                symbols.push(DocumentSymbol {
                    name: format!("import {}", import.name),
                    detail: Some(import.path.clone()),
                    kind: SymbolKind::FILE,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            Item::Element(elem) => {
                let children = body_symbols(&elem.body);
                let range = span_to_range(&elem.span);
                symbols.push(DocumentSymbol {
                    name: format!("{}: {}", elem.element_type, elem.id),
                    detail: Some(elem.element_type.clone()),
                    kind: element_symbol_kind(&elem.element_type),
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                });
            }
            Item::CheckSuite(cs) => {
                let children = body_symbols(&cs.body);
                let range = span_to_range(&cs.span);
                symbols.push(DocumentSymbol {
                    name: format!("CheckSuite: {}", cs.name),
                    detail: Some("CheckSuite".into()),
                    kind: SymbolKind::ENUM,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                });
            }
            Item::CheckDecl(c) => {
                let range = span_to_range(&c.span);
                symbols.push(DocumentSymbol {
                    name: format!("Check: {}", c.id),
                    detail: Some("Check".into()),
                    kind: SymbolKind::EVENT,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            Item::Template(t) => {
                let range = span_to_range(&t.span);
                symbols.push(DocumentSymbol {
                    name: format!("Template: {}", t.id),
                    detail: Some("Template".into()),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            Item::AssetLibrary(lib) => {
                let range = span_to_range(&lib.span);
                symbols.push(DocumentSymbol {
                    name: format!("AssetLibrary: {}", lib.name),
                    detail: Some("AssetLibrary".into()),
                    kind: SymbolKind::PACKAGE,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            Item::Comment(_) => {}
        }
    }

    symbols
}

#[allow(deprecated)]
fn body_symbols(body: &[BodyItem]) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    for item in body {
        match item {
            BodyItem::Element(elem) => {
                let children = body_symbols(&elem.body);
                let range = span_to_range(&elem.span);
                symbols.push(DocumentSymbol {
                    name: format!("{}: {}", elem.element_type, elem.id),
                    detail: Some(elem.element_type.clone()),
                    kind: element_symbol_kind(&elem.element_type),
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                });
            }
            BodyItem::CheckDecl(c) => {
                let range = span_to_range(&c.span);
                symbols.push(DocumentSymbol {
                    name: format!("Check: {}", c.id),
                    detail: Some("Check".into()),
                    kind: SymbolKind::EVENT,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                });
            }
            _ => {}
        }
    }

    symbols
}

fn element_symbol_kind(element_type: &str) -> SymbolKind {
    match element_type {
        "Wall" | "Slab" | "Roof" => SymbolKind::STRUCT,
        "Column" | "Beam" => SymbolKind::FIELD,
        "Window" | "Door" => SymbolKind::INTERFACE,
        "Stair" | "Railing" | "Ramp" => SymbolKind::OBJECT,
        "Furniture" => SymbolKind::VARIABLE,
        _ => SymbolKind::STRUCT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bml_core::parser::cst_to_ast;

    #[test]
    fn test_document_symbols_basic() {
        let source = "# Site: MySite\n## Building: House\n### Storey: Ground Floor\n  Wall: W-01\n    from: (0, 0) to: (6, 0)\n    height: 2.8 m\n    thickness: 0.24 m\n";
        let ast = cst_to_ast::parse(source).unwrap();
        let symbols = document_symbols(&ast);

        // Tree-sitter AST produces flat heading list at the top level
        assert_eq!(symbols.len(), 3);
        assert!(symbols[0].name.contains("Site"));
        assert!(symbols[1].name.contains("Building"));
        assert!(symbols[2].name.contains("Storey"));
        // Storey should have the Wall as a child
        let storey_children = symbols[2].children.as_ref().unwrap();
        assert_eq!(storey_children.len(), 1);
        assert!(storey_children[0].name.contains("Wall"));
    }
}
