use crate::ast::nodes::Value;
use crate::ast::span::Span;

use super::diagnostics::{Diagnostic, DiagnosticCode};

/// Unit conversion factor to SI (meters for length, m2 for area, etc.)
pub(crate) fn unit_to_si_factor(unit: &str) -> Option<f64> {
    match unit {
        // Length → meters
        "m" => Some(1.0),
        "cm" => Some(0.01),
        "mm" => Some(0.001),
        "ft" => Some(0.3048),
        "in" | "inch" => Some(0.0254),

        // Area → m2
        "m2" => Some(1.0),
        "cm2" => Some(0.0001),
        "ft2" => Some(0.092903),

        // These are already in SI or don't need conversion
        "W/m2K" => Some(1.0),
        "dB" => Some(1.0),
        "kg/m3" => Some(1.0),

        _ => None,
    }
}

/// Normalize a value with a unit to SI.
/// Returns the normalized value and optionally a diagnostic if the unit is invalid.
pub fn normalize_value(value: &Value, property_key: &str, span: Span) -> (f64, Option<Diagnostic>) {
    match value {
        Value::NumberWithUnit(n, unit) => {
            if let Some(factor) = unit_to_si_factor(unit) {
                (n * factor, None)
            } else {
                (
                    *n,
                    Some(
                        Diagnostic::warning(
                            DiagnosticCode::E400,
                            format!("unknown unit `{}` on property `{}`", unit, property_key),
                            span,
                        )
                        .with_help(
                            "valid units: m, cm, mm, ft, in, m2, cm2, ft2, W/m2K, dB, kg/m3",
                        ),
                    ),
                )
            }
        }
        Value::Number(n) => (*n, None),
        _ => (0.0, None),
    }
}

/// Normalize all numeric values in an element's properties to SI units.
/// Returns diagnostics for invalid units.
pub fn normalize_properties(properties: &[crate::ast::nodes::Property]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for prop in properties {
        if let Value::List(items) = &prop.value {
            diagnostics.extend(normalize_properties(items));
        }
        let units: Vec<&str> = match &prop.value {
            Value::NumberWithUnit(_, unit) => vec![unit],
            Value::NumberList(items) => items
                .iter()
                .filter_map(|v| match v {
                    Value::NumberWithUnit(_, unit) => Some(unit.as_str()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };
        for unit in units.into_iter().filter(|u| unit_to_si_factor(u).is_none()) {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::E400,
                    format!("unknown unit `{}` on property `{}`", unit, prop.key),
                    prop.span,
                )
                .with_help("valid units: m, cm, mm, ft, in, m2, cm2, ft2, W/m2K, dB, kg/m3"),
            );
        }
    }

    diagnostics
}
