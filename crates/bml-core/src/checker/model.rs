use crate::ast::span::Span;

/// A compliance check suite grouping related checks.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckSuite {
    pub name: String,
    pub description: Option<String>,
    pub checks: Vec<Check>,
    pub regulation: Option<Regulation>,
    pub span: Span,
}

/// A single compliance check rule.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Check {
    pub id: String,
    pub description: Option<String>,
    pub reference: Option<String>,
    pub severity: CheckSeverity,
    /// Element type this check applies to (e.g., "Door", "Wall", "Space")
    pub applies_to: Option<String>,
    /// Optional filter condition
    pub where_clause: Option<CheckExpression>,
    /// The assertion to evaluate
    pub assert_clause: Option<CheckExpression>,
    /// Other checks this depends on
    pub depends_on: Vec<String>,
    pub span: Span,
}

/// Severity level for check results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckSeverity {
    Error,
    Warning,
    Info,
}

impl CheckSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// A check expression (conditions used in where/assert clauses).
#[derive(Debug, Clone, serde::Serialize)]
pub enum CheckExpression {
    /// Comparison: left op right
    Comparison {
        left: CheckOperand,
        op: ComparisonOp,
        right: CheckOperand,
    },
    /// Set membership: left in [values]
    In {
        left: CheckOperand,
        values: Vec<String>,
    },
    /// Logical AND
    And(Box<CheckExpression>, Box<CheckExpression>),
    /// Logical OR
    Or(Box<CheckExpression>, Box<CheckExpression>),
    /// Boolean literal (for trivially true/false conditions)
    Literal(bool),
    /// Universal quantifier: forall elements of `element_type` [where filter] assert condition.
    /// True iff every matching element satisfies the assertion.
    Forall {
        element_type: String,
        filter: Option<Box<CheckExpression>>,
        assertion: Box<CheckExpression>,
    },
    /// Existential quantifier: exists element of `element_type` [where filter].
    /// True iff at least one matching element satisfies the filter (or exists at all).
    Exists {
        element_type: String,
        filter: Option<Box<CheckExpression>>,
    },
}

/// An operand in a check expression.
#[derive(Debug, Clone, serde::Serialize)]
pub enum CheckOperand {
    /// Simple property name (e.g., "width")
    Property(String),
    /// Dotted path (e.g., ["host", "space", "usage"])
    Path(Vec<String>),
    /// Numeric literal
    Number(f64),
    /// Number with unit (value already in SI)
    NumberWithUnit(f64, String),
    /// Boolean literal
    Boolean(bool),
    /// String literal
    StringLiteral(String),
    /// Function call (e.g., count(), sum(area))
    FunctionCall {
        name: String,
        args: Vec<CheckOperand>,
    },
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ComparisonOp {
    Ge, // >=
    Le, // <=
    Eq, // ==
    Ne, // !=
    Gt, // >
    Lt, // <
}

impl ComparisonOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Gt => ">",
            Self::Lt => "<",
        }
    }
}

/// Result of evaluating a single check against a single element.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckResult {
    pub check_id: String,
    pub suite: Option<String>,
    pub status: CheckStatus,
    pub severity: CheckSeverity,
    pub element_id: String,
    pub element_type: String,
    pub message: String,
    pub reference: Option<String>,
    pub actual_value: Option<String>,
    pub expected: Option<String>,
    pub line: usize,
    /// `Span::file_id` of `line` (0 for a single file).
    #[serde(skip)]
    pub file_id: u32,
}

/// Status of a check evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Fail,
    Skip,
    Error,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Error => "error",
        }
    }
}

/// Regulation metadata for traceability.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Regulation {
    pub jurisdiction: Option<String>,
    pub version: Option<String>,
    pub sections: Vec<String>,
}
