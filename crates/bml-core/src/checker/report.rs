//! Report generation for compliance check results.
//!
//! Supports three output formats:
//! - JSON: Machine-readable structured output
//! - HTML: Human-readable styled report
//! - SARIF: Static Analysis Results Interchange Format (for CI/CD integration)

use super::model::{CheckResult, CheckSeverity, CheckStatus};

/// A compliance check report containing all results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckReport {
    pub suites: Vec<String>,
    pub results: Vec<CheckResult>,
}

impl CheckReport {
    /// Count results by status.
    pub fn count_by_status(&self, status: CheckStatus) -> usize {
        self.results.iter().filter(|r| r.status == status).count()
    }

    /// Get all failing results.
    pub fn failures(&self) -> Vec<&CheckResult> {
        self.results
            .iter()
            .filter(|r| r.status == CheckStatus::Fail)
            .collect()
    }

    /// Get all error results.
    pub fn errors(&self) -> Vec<&CheckResult> {
        self.results
            .iter()
            .filter(|r| r.status == CheckStatus::Error)
            .collect()
    }

    /// Check if all checks passed (no failures or errors).
    pub fn all_passed(&self) -> bool {
        !self
            .results
            .iter()
            .any(|r| r.status == CheckStatus::Fail || r.status == CheckStatus::Error)
    }

    /// Generate JSON report.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Generate HTML report.
    pub fn to_html(&self, title: &str) -> String {
        let mut html = String::new();
        html.push_str("<!DOCTYPE html>\n<html>\n<head>\n");
        html.push_str(&format!("<title>{}</title>\n", escape_html(title)));
        html.push_str("<style>\n");
        html.push_str(HTML_STYLES);
        html.push_str("</style>\n</head>\n<body>\n");
        html.push_str(&format!("<h1>{}</h1>\n", escape_html(title)));

        // Summary
        let pass = self.count_by_status(CheckStatus::Pass);
        let fail = self.count_by_status(CheckStatus::Fail);
        let skip = self.count_by_status(CheckStatus::Skip);
        let error = self.count_by_status(CheckStatus::Error);

        html.push_str("<div class=\"summary\">\n");
        html.push_str(&format!("<span class=\"pass\">{} passed</span> | ", pass));
        html.push_str(&format!("<span class=\"fail\">{} failed</span> | ", fail));
        html.push_str(&format!("<span class=\"skip\">{} skipped</span> | ", skip));
        html.push_str(&format!("<span class=\"error\">{} errors</span>\n", error));
        html.push_str("</div>\n");

        // Results table
        html.push_str("<table>\n<thead>\n<tr>\n");
        html.push_str("<th>Status</th><th>Severity</th><th>Check</th><th>Element</th><th>Message</th><th>Actual</th><th>Expected</th>\n");
        html.push_str("</tr>\n</thead>\n<tbody>\n");

        for result in &self.results {
            let status_class = result.status.as_str();
            html.push_str(&format!("<tr class=\"{}\">\n", status_class));
            html.push_str(&format!(
                "<td class=\"status-{}\">{}</td>\n",
                status_class,
                result.status.as_str().to_uppercase()
            ));
            html.push_str(&format!("<td>{}</td>\n", result.severity.as_str()));
            html.push_str(&format!(
                "<td>{}{}</td>\n",
                escape_html(&result.check_id),
                result
                    .suite
                    .as_ref()
                    .map(|s| format!(" ({})", escape_html(s)))
                    .unwrap_or_default()
            ));
            html.push_str(&format!(
                "<td>{} <code>{}</code></td>\n",
                escape_html(&result.element_type),
                escape_html(&result.element_id)
            ));
            html.push_str(&format!("<td>{}</td>\n", escape_html(&result.message)));
            html.push_str(&format!(
                "<td>{}</td>\n",
                result
                    .actual_value
                    .as_deref()
                    .map(escape_html)
                    .unwrap_or_default()
            ));
            html.push_str(&format!(
                "<td>{}</td>\n",
                result
                    .expected
                    .as_deref()
                    .map(escape_html)
                    .unwrap_or_default()
            ));
            html.push_str("</tr>\n");
        }

        html.push_str("</tbody>\n</table>\n");
        html.push_str("</body>\n</html>");
        html
    }

    /// Generate SARIF (Static Analysis Results Interchange Format) report.
    pub fn to_sarif(&self, tool_name: &str, tool_version: &str) -> String {
        let rules: Vec<SarifRule> = self
            .results
            .iter()
            .map(|r| &r.check_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .map(|id| {
                let result = self.results.iter().find(|r| &r.check_id == id).unwrap();
                SarifRule {
                    id: id.clone(),
                    short_description: SarifMessage {
                        text: result.message.clone(),
                    },
                }
            })
            .collect();

        let results: Vec<SarifResult> = self
            .results
            .iter()
            .filter(|r| r.status == CheckStatus::Fail || r.status == CheckStatus::Error)
            .map(|r| SarifResult {
                rule_id: r.check_id.clone(),
                level: match r.severity {
                    CheckSeverity::Error => "error",
                    CheckSeverity::Warning => "warning",
                    CheckSeverity::Info => "note",
                },
                message: SarifMessage {
                    text: format!(
                        "{}: {} `{}`{}{}",
                        r.message,
                        r.element_type,
                        r.element_id,
                        r.actual_value
                            .as_ref()
                            .map(|v| format!(" (actual: {})", v))
                            .unwrap_or_default(),
                        r.expected
                            .as_ref()
                            .map(|v| format!(" (expected: {})", v))
                            .unwrap_or_default(),
                    ),
                },
                locations: vec![SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        region: SarifRegion { start_line: r.line },
                    },
                }],
            })
            .collect();

        let sarif = SarifReport {
            version: "2.1.0",
            schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
            runs: vec![SarifRun {
                tool: SarifTool {
                    driver: SarifDriver {
                        name: tool_name.to_string(),
                        version: tool_version.to_string(),
                        rules,
                    },
                },
                results,
            }],
        };

        serde_json::to_string_pretty(&sarif).unwrap_or_default()
    }
}

/// Print check results to stderr in human-readable format.
/// `sources` holds one `(filename, source)` per file, indexed by
/// `CheckResult::file_id - 1`; `file_id` 0 uses the first entry.
pub fn print_check_results(report: &CheckReport, sources: &[(String, &str)]) {
    let lines: Vec<Vec<&str>> = sources.iter().map(|(_, s)| s.lines().collect()).collect();

    for result in &report.results {
        if result.status == CheckStatus::Skip {
            continue;
        }

        let (color, label) = match result.status {
            CheckStatus::Pass => ("\x1b[32m", "PASS"),
            CheckStatus::Fail => match result.severity {
                CheckSeverity::Error => ("\x1b[31m", "FAIL"),
                CheckSeverity::Warning => ("\x1b[33m", "WARN"),
                CheckSeverity::Info => ("\x1b[34m", "INFO"),
            },
            CheckStatus::Error => ("\x1b[31m", "ERROR"),
            CheckStatus::Skip => continue,
        };

        eprintln!(
            "{}[{}]\x1b[0m {}: {}",
            color, label, result.check_id, result.message
        );

        let file = (result.file_id as usize).saturating_sub(1);
        let filename = sources.get(file).map_or("<project>", |(f, _)| f.as_str());

        if !result.element_id.is_empty() {
            eprintln!(
                "  \x1b[36m-->\x1b[0m {}:{}  ({} `{}`)",
                filename, result.line, result.element_type, result.element_id
            );
        }

        // Show source line
        if result.line > 0
            && let Some(line_text) = lines.get(file).and_then(|l| l.get(result.line - 1))
        {
            eprintln!("   \x1b[90m|\x1b[0m");
            eprintln!(" \x1b[90m{} |\x1b[0m {}", result.line, line_text);
            eprintln!("   \x1b[90m|\x1b[0m");
        }

        if let Some(actual) = &result.actual_value {
            eprint!("   \x1b[90m= actual:\x1b[0m {}", actual);
            if let Some(expected) = &result.expected {
                eprintln!(", \x1b[90mexpected:\x1b[0m {}", expected);
            } else {
                eprintln!();
            }
        }

        if let Some(reference) = &result.reference {
            eprintln!("   \x1b[90m= ref:\x1b[0m {}", reference);
        }

        eprintln!();
    }
}

/// Print a summary of check results.
pub fn print_check_summary(report: &CheckReport) {
    let pass = report.count_by_status(CheckStatus::Pass);
    let fail = report.count_by_status(CheckStatus::Fail);
    let skip = report.count_by_status(CheckStatus::Skip);
    let error = report.count_by_status(CheckStatus::Error);
    let total = report.results.len();

    eprintln!();
    if report.all_passed() {
        eprintln!(
            "\x1b[32mAll {} checks passed\x1b[0m ({} skipped)",
            pass, skip
        );
    } else {
        eprintln!(
            "Check results: {} total, \x1b[32m{} passed\x1b[0m, \x1b[31m{} failed\x1b[0m, {} skipped, {} errors",
            total, pass, fail, skip, error
        );
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const HTML_STYLES: &str = r#"
body { font-family: system-ui, sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; }
h1 { color: #333; }
.summary { margin: 20px 0; font-size: 1.2em; }
.pass { color: #22863a; font-weight: bold; }
.fail { color: #cb2431; font-weight: bold; }
.skip { color: #6a737d; }
.error { color: #cb2431; }
table { width: 100%; border-collapse: collapse; margin-top: 20px; }
th { background: #f6f8fa; text-align: left; padding: 8px 12px; border-bottom: 2px solid #e1e4e8; }
td { padding: 8px 12px; border-bottom: 1px solid #e1e4e8; }
tr.fail { background: #ffeef0; }
tr.pass { background: #f0fff4; }
tr.skip { background: #f6f8fa; color: #6a737d; }
tr.error { background: #ffeef0; }
.status-pass { color: #22863a; font-weight: bold; }
.status-fail { color: #cb2431; font-weight: bold; }
.status-skip { color: #6a737d; }
.status-error { color: #cb2431; font-weight: bold; }
code { background: #f1f3f5; padding: 2px 6px; border-radius: 3px; }
"#;

// SARIF types for serialization
#[derive(serde::Serialize)]
struct SarifReport {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(serde::Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(serde::Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(serde::Serialize)]
struct SarifDriver {
    name: String,
    version: String,
    rules: Vec<SarifRule>,
}

#[derive(serde::Serialize)]
struct SarifRule {
    id: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifMessage,
}

#[derive(serde::Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
}

#[derive(serde::Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(serde::Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(serde::Serialize)]
struct SarifPhysicalLocation {
    region: SarifRegion,
}

#[derive(serde::Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine")]
    start_line: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_report() -> CheckReport {
        CheckReport {
            suites: vec!["Fire Safety".to_string()],
            results: vec![
                CheckResult {
                    check_id: "min_door_width".to_string(),
                    suite: Some("Fire Safety".to_string()),
                    status: CheckStatus::Fail,
                    severity: CheckSeverity::Error,
                    element_id: "D-01".to_string(),
                    element_type: "Door".to_string(),
                    message: "Door width must be >= 0.90 m".to_string(),
                    reference: Some("MBO §36(5)".to_string()),
                    actual_value: Some("0.85".to_string()),
                    expected: Some(">= 0.90".to_string()),
                    line: 15,
                    file_id: 0,
                },
                CheckResult {
                    check_id: "wall_fire_rating".to_string(),
                    suite: Some("Fire Safety".to_string()),
                    status: CheckStatus::Pass,
                    severity: CheckSeverity::Error,
                    element_id: "W-01".to_string(),
                    element_type: "Wall".to_string(),
                    message: "Wall has valid fire rating".to_string(),
                    reference: None,
                    actual_value: None,
                    expected: None,
                    line: 5,
                    file_id: 0,
                },
            ],
        }
    }

    #[test]
    fn test_report_json() {
        let report = make_test_report();
        let json = report.to_json().unwrap();
        assert!(json.contains("min_door_width"));
        assert!(json.contains("\"fail\""));
        assert!(json.contains("\"pass\""));
    }

    #[test]
    fn test_report_html() {
        let report = make_test_report();
        let html = report.to_html("Fire Safety Report");
        assert!(html.contains("<html>"));
        assert!(html.contains("min_door_width"));
        assert!(html.contains("FAIL"));
        assert!(html.contains("PASS"));
        assert!(html.contains("1 passed"));
        assert!(html.contains("1 failed"));
    }

    #[test]
    fn test_report_sarif() {
        let report = make_test_report();
        let sarif = report.to_sarif("buildingml", "0.1.0");
        assert!(sarif.contains("\"$schema\""));
        assert!(sarif.contains("min_door_width"));
        assert!(sarif.contains("\"error\""));
    }

    #[test]
    fn test_report_counts() {
        let report = make_test_report();
        assert_eq!(report.count_by_status(CheckStatus::Pass), 1);
        assert_eq!(report.count_by_status(CheckStatus::Fail), 1);
        assert!(!report.all_passed());
        assert_eq!(report.failures().len(), 1);
    }
}
