//! Regulation modeling for compliance traceability.
//!
//! Each check suite can reference a specific regulation (building code)
//! with jurisdiction, version, and section references.

use super::model::Regulation;
use crate::ast::nodes::Value;

/// Apply a regulation property from the AST to a Regulation model.
pub fn apply_regulation_property(reg: &mut Regulation, key: &str, value: &Value) {
    match key {
        "jurisdiction" => {
            if let Value::String(s) = value {
                reg.jurisdiction = Some(s.clone());
            } else if let Value::Identifier(s) = value {
                reg.jurisdiction = Some(s.clone());
            }
        }
        "version" => {
            if let Value::String(s) = value {
                reg.version = Some(s.clone());
            } else if let Value::Identifier(s) = value {
                reg.version = Some(s.clone());
            }
        }
        "section" => {
            if let Value::String(s) = value {
                reg.sections.push(s.clone());
            } else if let Value::Identifier(s) = value {
                reg.sections.push(s.clone());
            }
        }
        _ => {}
    }
}

/// Format a regulation reference for display in reports.
pub fn format_regulation(reg: &Regulation) -> String {
    let mut parts = Vec::new();

    if let Some(jurisdiction) = &reg.jurisdiction {
        parts.push(jurisdiction.clone());
    }
    if let Some(version) = &reg.version {
        parts.push(format!("v{}", version));
    }
    if !reg.sections.is_empty() {
        parts.push(format!("§{}", reg.sections.join(", §")));
    }

    if parts.is_empty() {
        "unknown regulation".to_string()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_regulation() {
        let reg = Regulation {
            jurisdiction: Some("Germany (MBO)".to_string()),
            version: Some("2023".to_string()),
            sections: vec!["36(5)".to_string(), "37(1)".to_string()],
        };

        let formatted = format_regulation(&reg);
        assert!(formatted.contains("Germany (MBO)"));
        assert!(formatted.contains("2023"));
        assert!(formatted.contains("§36(5)"));
    }

    #[test]
    fn test_apply_regulation_property() {
        let mut reg = Regulation::default();

        apply_regulation_property(&mut reg, "jurisdiction", &Value::String("DE".to_string()));
        assert_eq!(reg.jurisdiction, Some("DE".to_string()));

        apply_regulation_property(&mut reg, "version", &Value::String("2023".to_string()));
        assert_eq!(reg.version, Some("2023".to_string()));

        apply_regulation_property(&mut reg, "section", &Value::String("36(5)".to_string()));
        assert_eq!(reg.sections, vec!["36(5)"]);
    }
}
