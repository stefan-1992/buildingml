pub mod computed;
pub mod engine;
pub mod model;
pub mod regulation;
pub mod report;

use crate::ast::nodes::{BodyItem, CheckDeclaration, CheckSuiteDeclaration, Item, SourceFile};
use crate::semantic::model::SemanticModel;
use model::{Check, CheckSuite};
use report::CheckReport;

/// Run all compliance checks from a parsed source file against a semantic model.
///
/// Returns a report containing all check results.
pub fn run_checks(source_file: &SourceFile, model: &SemanticModel) -> CheckReport {
    let (suites, standalone_checks) = extract_checks(source_file);
    let mut all_results = Vec::new();

    // Run standalone checks
    for check in &standalone_checks {
        let results = engine::evaluate_check(check, None, model);
        all_results.extend(results);
    }

    // Run suite checks
    for suite in &suites {
        for check in &suite.checks {
            let results = engine::evaluate_check(check, Some(&suite.name), model);
            all_results.extend(results);
        }
    }

    CheckReport {
        suites: suites.iter().map(|s| s.name.clone()).collect(),
        results: all_results,
    }
}

/// Extract Check and CheckSuite declarations from an AST and convert to IR.
fn extract_checks(source_file: &SourceFile) -> (Vec<CheckSuite>, Vec<Check>) {
    let mut suites = Vec::new();
    let mut standalone = Vec::new();

    for item in &source_file.items {
        match item {
            Item::CheckSuite(cs) => {
                if let Some(suite) = convert_check_suite(cs) {
                    suites.push(suite);
                }
            }
            Item::CheckDecl(c) => {
                if let Some(check) = convert_check(c) {
                    standalone.push(check);
                }
            }
            _ => {}
        }
    }

    (suites, standalone)
}

fn convert_check_suite(cs: &CheckSuiteDeclaration) -> Option<CheckSuite> {
    let mut suite = CheckSuite {
        name: cs.name.clone(),
        description: None,
        checks: Vec::new(),
        regulation: None,
        span: cs.span,
    };

    for item in &cs.body {
        match item {
            BodyItem::CheckDecl(c) => {
                if let Some(check) = convert_check(c) {
                    suite.checks.push(check);
                }
            }
            BodyItem::Property(p) => match p.key.as_str() {
                "description" => {
                    if let crate::ast::nodes::Value::String(s) = &p.value {
                        suite.description = Some(s.clone());
                    }
                }
                "jurisdiction" | "version" | "section" => {
                    let reg = suite.regulation.get_or_insert_with(Default::default);
                    regulation::apply_regulation_property(reg, &p.key, &p.value);
                }
                _ => {}
            },
            _ => {}
        }
    }

    Some(suite)
}

fn convert_check(c: &CheckDeclaration) -> Option<Check> {
    let mut check = Check {
        id: c.id.clone(),
        description: None,
        reference: None,
        severity: model::CheckSeverity::Error,
        applies_to: None,
        where_clause: None,
        assert_clause: None,
        depends_on: Vec::new(),
        span: c.span,
    };

    for item in &c.body {
        if let BodyItem::Property(p) = item {
            match p.key.as_str() {
                "description" => {
                    if let crate::ast::nodes::Value::String(s) = &p.value {
                        check.description = Some(s.clone());
                    }
                }
                "reference" => {
                    if let crate::ast::nodes::Value::String(s) = &p.value {
                        check.reference = Some(s.clone());
                    }
                }
                "severity" => {
                    if let crate::ast::nodes::Value::Identifier(s) = &p.value {
                        check.severity = match s.as_str() {
                            "error" => model::CheckSeverity::Error,
                            "warning" => model::CheckSeverity::Warning,
                            "info" => model::CheckSeverity::Info,
                            _ => model::CheckSeverity::Error,
                        };
                    }
                }
                "applies_to" => {
                    if let crate::ast::nodes::Value::Identifier(s) = &p.value {
                        check.applies_to = Some(s.clone());
                    }
                }
                "where" => {
                    check.where_clause = Some(engine::value_to_expression(&p.value));
                }
                "assert" => {
                    check.assert_clause = Some(engine::value_to_expression(&p.value));
                }
                "depends_on" => match &p.value {
                    crate::ast::nodes::Value::Identifier(s) => {
                        check.depends_on.push(s.clone());
                    }
                    crate::ast::nodes::Value::EnumList(ids) => {
                        check.depends_on.extend(ids.clone());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Some(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;
    use model::CheckStatus;

    /// Helper: parse source, analyze, run checks, return report.
    fn check_source(source: &str) -> CheckReport {
        let ast = cst_to_ast::parse(source).expect("parse failed");
        let (model, _diags) = semantic::analyze(&ast);
        run_checks(&ast, &model)
    }

    /// A stair with a landing is checked once, not once more per generated
    /// flight, and its default riser height is visible to checks.
    #[test]
    fn test_stair_flights_checked_through_stair() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.4 m
  Stair: ST
    type: l_shaped
    riser_count: 20

Check: riser
  applies_to: Stair
  assert: riser_height <= 0.19
";
        let report = check_source(source);
        let ids: Vec<&str> = report
            .results
            .iter()
            .map(|r| r.element_id.as_str())
            .collect();
        assert_eq!(ids, ["ST"]);
        assert_eq!(report.results[0].status, CheckStatus::Pass);
    }

    // ── Full pipeline integration tests ──────────────────────────────

    #[test]
    fn test_compliance_checks_fixture() {
        let path = format!(
            "{}/tests/fixtures/{}",
            env!("CARGO_MANIFEST_DIR").replace("/crates/bml-core", ""),
            "compliance_checks.bml"
        );
        let source = std::fs::read_to_string(&path).expect("cannot read fixture");
        let ast = cst_to_ast::parse(&source).expect("fixture parse failed");
        let (model, diagnostics) = semantic::analyze(&ast);

        // Should have a valid spatial model
        assert!(!model.sites.is_empty());

        // Print any diagnostics for debugging
        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        // Run checks
        let report = run_checks(&ast, &model);

        // Should have at least one suite
        assert!(
            !report.suites.is_empty(),
            "expected check suites in fixture"
        );
        assert!(report.suites.contains(&"fire_safety".to_string()));

        // Should have results from both standalone and suite checks
        assert!(!report.results.is_empty(), "expected check results");

        // Verify the min_door_width standalone check ran
        let door_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.check_id == "min_door_width")
            .collect();
        assert!(
            !door_results.is_empty(),
            "expected results for min_door_width"
        );
        // D-NARROW has width 0.75 < 0.90, so at least one should fail
        assert!(
            door_results.iter().any(|r| r.status == CheckStatus::Fail),
            "expected D-NARROW to fail min_door_width check"
        );
        // D-MAIN has width 1.20 >= 0.90, so at least one should pass
        assert!(
            door_results.iter().any(|r| r.status == CheckStatus::Pass),
            "expected D-MAIN to pass min_door_width check"
        );

        // Verify fire_safety suite checks ran
        let fire_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.suite.as_deref() == Some("fire_safety"))
            .collect();
        assert!(
            !fire_results.is_empty(),
            "expected results from fire_safety suite"
        );
    }

    #[test]
    fn test_inline_check_comparison_pass() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m

Check: wall_height
  applies_to: Wall
  assert: height >= 2.5
";
        let report = check_source(source);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, CheckStatus::Pass);
        assert_eq!(report.results[0].check_id, "wall_height");
    }

    #[test]
    fn test_inline_check_comparison_fail() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 2.0 m
    thickness: 0.24 m

Check: min_wall_height
  description: \"Wall height must be at least 2.5 m\"
  applies_to: Wall
  assert: height >= 2.5
";
        let report = check_source(source);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, CheckStatus::Fail);
        assert!(report.results[0].actual_value.is_some());
        assert!(report.results[0].expected.is_some());
    }

    #[test]
    fn test_check_with_where_filters_correctly() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W-EXT
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.365 m
    is_external: true
    fire_rating: REI90

  Wall: W-INT
    from: (5, 0) to: (5, 4)
    height: 3.0 m
    thickness: 0.115 m
    is_external: false

Check: ext_wall_fire
  applies_to: Wall
  where: is_external == true
  assert: fire_rating in [REI30, REI60, REI90, REI120]
";
        let report = check_source(source);

        // W-EXT should pass (is_external=true, fire_rating=REI90)
        let ext_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.element_id == "W-EXT")
            .collect();
        assert_eq!(ext_results.len(), 1);
        assert_eq!(ext_results[0].status, CheckStatus::Pass);

        // W-INT should be skipped (is_external=false, doesn't pass where clause)
        let int_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.element_id == "W-INT")
            .collect();
        assert_eq!(int_results.len(), 1);
        assert_eq!(int_results[0].status, CheckStatus::Skip);
    }

    #[test]
    fn test_check_in_expression_fail() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m
    fire_rating: EI30

Check: valid_fire_ratings
  applies_to: Wall
  assert: fire_rating in [REI30, REI60, REI90]
";
        let report = check_source(source);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, CheckStatus::Fail);
    }

    #[test]
    fn test_check_suite_with_regulation() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m
    is_external: true
    fire_rating: REI90

CheckSuite: fire_safety
  description: \"Fire safety compliance\"
  jurisdiction: DE
  version: \"2024\"
  section: \"Section 5\"

  Check: ext_fire
    applies_to: Wall
    where: is_external == true
    assert: fire_rating in [REI60, REI90, REI120]
";
        let report = check_source(source);
        assert!(report.suites.contains(&"fire_safety".to_string()));

        let suite_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.suite.as_deref() == Some("fire_safety"))
            .collect();
        assert_eq!(suite_results.len(), 1);
        assert_eq!(suite_results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_no_matching_elements() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m

Check: door_check
  applies_to: Door
  assert: width >= 0.90
";
        let report = check_source(source);
        // No Door elements, so should be skipped
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, CheckStatus::Skip);
    }

    #[test]
    fn test_check_nested_element_door_in_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m

    Door: D1
      offset: 1.0 m
      width: 0.95 m
      height: 2.10 m

    Door: D2
      offset: 3.0 m
      width: 0.80 m
      height: 2.10 m

Check: min_door_width
  applies_to: Door
  assert: width >= 0.90
";
        let report = check_source(source);
        let door_results: Vec<_> = report
            .results
            .iter()
            .filter(|r| r.check_id == "min_door_width")
            .collect();
        assert_eq!(door_results.len(), 2);

        // D1 (0.95) passes, D2 (0.80) fails
        let d1 = door_results.iter().find(|r| r.element_id == "D1").unwrap();
        assert_eq!(d1.status, CheckStatus::Pass);

        let d2 = door_results.iter().find(|r| r.element_id == "D2").unwrap();
        assert_eq!(d2.status, CheckStatus::Fail);
    }

    #[test]
    fn test_check_without_assert_produces_error() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m

Check: incomplete_check
  applies_to: Wall
";
        let report = check_source(source);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, CheckStatus::Error);
    }

    #[test]
    fn test_multiple_checks_on_same_elements() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m
    is_external: true

Check: min_height
  applies_to: Wall
  assert: height >= 2.5

Check: min_thickness
  applies_to: Wall
  assert: thickness >= 0.20
";
        let report = check_source(source);
        assert_eq!(report.results.len(), 2);
        assert!(report.results.iter().all(|r| r.status == CheckStatus::Pass));
    }

    // ── Report format tests ──────────────────────────────────────────

    #[test]
    fn test_json_report_from_source() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.24 m

Check: wall_check
  applies_to: Wall
  assert: height >= 2.5
";
        let report = check_source(source);
        let json = report.to_json().unwrap();
        assert!(json.contains("wall_check"));
        assert!(json.contains("\"pass\""));
        assert!(json.contains("W1"));
    }

    #[test]
    fn test_html_report_from_source() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 2.0 m
    thickness: 0.24 m

Check: wall_check
  applies_to: Wall
  assert: height >= 2.5
";
        let report = check_source(source);
        let html = report.to_html("Test Report");
        assert!(html.contains("<html>"));
        assert!(html.contains("Test Report"));
        assert!(html.contains("FAIL"));
    }

    #[test]
    fn test_sarif_report_from_source() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 2.0 m
    thickness: 0.24 m

Check: wall_check
  applies_to: Wall
  assert: height >= 2.5
";
        let report = check_source(source);
        let sarif = report.to_sarif("buildingml", "0.1.0");
        assert!(sarif.contains("\"$schema\""));
        assert!(sarif.contains("wall_check"));
        assert!(sarif.contains("\"error\""));
    }

    #[test]
    fn test_report_all_passed() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3.0 m
    thickness: 0.30 m

Check: height_ok
  applies_to: Wall
  assert: height >= 2.5

Check: thick_ok
  applies_to: Wall
  assert: thickness >= 0.20
";
        let report = check_source(source);
        assert!(report.all_passed());
        assert!(report.failures().is_empty());
    }

    #[test]
    fn test_report_with_failures() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m

  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 2.0 m
    thickness: 0.10 m

Check: height_ok
  applies_to: Wall
  assert: height >= 2.5

Check: thick_ok
  applies_to: Wall
  assert: thickness >= 0.20
";
        let report = check_source(source);
        assert!(!report.all_passed());
        assert_eq!(report.failures().len(), 2);
    }
}
