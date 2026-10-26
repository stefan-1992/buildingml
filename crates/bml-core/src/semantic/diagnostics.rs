use crate::ast::span::Span;

/// Diagnostic severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A diagnostic message from semantic analysis.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
    /// File path for multi-file error attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Related code locations for multi-location diagnostics.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedInformation>,
}

/// A related code location referenced by a diagnostic.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RelatedInformation {
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
            help: None,
            file: None,
            related: Vec::new(),
        }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
            help: None,
            file: None,
            related: Vec::new(),
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.related.push(RelatedInformation {
            span,
            message: message.into(),
        });
        self
    }

    pub fn severity_str(&self) -> &'static str {
        match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// Structured diagnostic codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum DiagnosticCode {
    // E001: Syntax errors (handled by parser)
    // E100-E103: Type errors
    /// Unknown element type
    E100,
    /// Missing required property
    E101,
    /// Wrong property value type
    E102,
    /// Unknown property for element type
    E103,

    // E200-E202: Containment errors
    /// Heading level does not match spatial type
    E200,
    /// Element in wrong spatial context
    E201,
    /// Invalid element nesting (e.g., Window not inside Wall)
    E202,

    // E300-E307: Reference errors
    /// Duplicate element ID
    E300,
    /// Unresolved material reference
    E301,
    /// Duplicate material ID
    E302,
    /// Unresolved storey reference
    E303,
    /// Opening overflow (offset + width > wall length)
    E304,
    /// Coordinate out of bounds
    E305,
    /// Layered material inconsistent (layer list, or element thickness vs layer sum)
    E306,
    /// `system:` names no `System:` element
    E307,

    // E308-E313: Curve geometry errors
    /// Curve with too few points, or a smooth run through a repeated point
    E308,
    /// Invalid `nurbs` segment (degree, knots, weights)
    E309,
    /// Curved outline crosses itself
    E310,
    /// Wall bends tighter than half its thickness
    E311,
    /// Both `path:` and `from:` / `to:` given
    E312,
    /// Rectangular duct or cable tray on a smooth run
    E313,

    // E400-E405: Unit and lint errors
    /// Invalid unit for property
    E400,
    /// Wall thickness out of range
    E401,
    /// Door/window dimension out of range
    E402,
    /// Storey height out of range
    E403,
    /// Column aspect ratio warning
    E404,
    /// Beam span warning
    E405,
    /// Space has no door
    E406,
    /// Coincident or overlapping walls
    E407,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E100 => "E100",
            Self::E101 => "E101",
            Self::E102 => "E102",
            Self::E103 => "E103",
            Self::E200 => "E200",
            Self::E201 => "E201",
            Self::E202 => "E202",
            Self::E300 => "E300",
            Self::E301 => "E301",
            Self::E302 => "E302",
            Self::E303 => "E303",
            Self::E304 => "E304",
            Self::E305 => "E305",
            Self::E306 => "E306",
            Self::E307 => "E307",
            Self::E308 => "E308",
            Self::E309 => "E309",
            Self::E310 => "E310",
            Self::E311 => "E311",
            Self::E312 => "E312",
            Self::E313 => "E313",
            Self::E400 => "E400",
            Self::E401 => "E401",
            Self::E402 => "E402",
            Self::E403 => "E403",
            Self::E404 => "E404",
            Self::E405 => "E405",
            Self::E406 => "E406",
            Self::E407 => "E407",
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
