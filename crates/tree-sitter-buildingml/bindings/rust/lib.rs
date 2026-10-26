use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_buildingml() -> *const ();
}

/// Returns the tree-sitter [`LanguageFn`] for BuildingML.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_buildingml) };

/// The syntax highlighting query for BuildingML.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Error loading BuildingML grammar");
    }
}
