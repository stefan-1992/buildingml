use crate::ast::nodes::{ComparisonOp as AstCompOp, Value};
use crate::checker::computed;
use crate::semantic::model::*;

use super::model::*;

/// Convert an AST Value (from where/assert property) into a CheckExpression.
pub fn value_to_expression(value: &Value) -> CheckExpression {
    match value {
        Value::ComparisonExpr { left, op, right } => CheckExpression::Comparison {
            left: value_to_operand(left),
            op: ast_op_to_check_op(*op),
            right: value_to_operand(right),
        },
        Value::InExpr { left, values } => CheckExpression::In {
            left: value_to_operand(left),
            values: values.clone(),
        },
        Value::Boolean(b) => CheckExpression::Literal(*b),
        _ => {
            // Fallback: treat as a literal true (unknown expression form)
            CheckExpression::Literal(true)
        }
    }
}

fn value_to_operand(value: &Value) -> CheckOperand {
    match value {
        Value::Identifier(id) => CheckOperand::Property(id.clone()),
        Value::PathAccess(segments) => CheckOperand::Path(segments.clone()),
        Value::Number(n) => CheckOperand::Number(*n),
        Value::NumberWithUnit(n, u) => CheckOperand::NumberWithUnit(*n, u.clone()),
        Value::Boolean(b) => CheckOperand::Boolean(*b),
        Value::String(s) => CheckOperand::StringLiteral(s.clone()),
        Value::FunctionCall { name, args } => CheckOperand::FunctionCall {
            name: name.clone(),
            args: args.iter().map(value_to_operand).collect(),
        },
        _ => CheckOperand::StringLiteral(format!("{:?}", value)),
    }
}

fn ast_op_to_check_op(op: AstCompOp) -> ComparisonOp {
    match op {
        AstCompOp::Ge => ComparisonOp::Ge,
        AstCompOp::Le => ComparisonOp::Le,
        AstCompOp::Eq => ComparisonOp::Eq,
        AstCompOp::Ne => ComparisonOp::Ne,
        AstCompOp::Gt => ComparisonOp::Gt,
        AstCompOp::Lt => ComparisonOp::Lt,
    }
}

/// Evaluate a single check against all matching elements in the model.
pub fn evaluate_check(
    check: &Check,
    suite_name: Option<&str>,
    model: &SemanticModel,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    let assert_clause = match &check.assert_clause {
        Some(expr) => expr,
        None => {
            results.push(CheckResult {
                check_id: check.id.clone(),
                suite: suite_name.map(|s| s.to_string()),
                status: CheckStatus::Error,
                severity: check.severity,
                element_id: String::new(),
                element_type: String::new(),
                message: format!("check `{}` has no assert clause", check.id),
                reference: check.reference.clone(),
                actual_value: None,
                expected: None,
                line: check.span.start.line,
                file_id: check.span.file_id,
            });
            return results;
        }
    };

    // Collect all elements matching applies_to
    let target_type = check.applies_to.as_deref();
    let elements = collect_matching_elements(model, target_type);

    for elem_ctx in &elements {
        // Evaluate where clause (filter)
        if let Some(where_clause) = &check.where_clause {
            match eval_expression(where_clause, elem_ctx, model) {
                EvalResult::Bool(true) => {} // passes filter
                EvalResult::Bool(false) => {
                    results.push(CheckResult {
                        check_id: check.id.clone(),
                        suite: suite_name.map(|s| s.to_string()),
                        status: CheckStatus::Skip,
                        severity: check.severity,
                        element_id: elem_ctx.element.id.clone(),
                        element_type: elem_ctx.element.element_type.as_str().to_string(),
                        message: format!(
                            "skipped: where clause not satisfied for {} `{}`",
                            elem_ctx.element.element_type.as_str(),
                            elem_ctx.element.id
                        ),
                        reference: check.reference.clone(),
                        actual_value: None,
                        expected: None,
                        line: elem_ctx.element.span.start.line,
                        file_id: elem_ctx.element.span.file_id,
                    });
                    continue;
                }
                EvalResult::Error(msg) => {
                    results.push(CheckResult {
                        check_id: check.id.clone(),
                        suite: suite_name.map(|s| s.to_string()),
                        status: CheckStatus::Error,
                        severity: check.severity,
                        element_id: elem_ctx.element.id.clone(),
                        element_type: elem_ctx.element.element_type.as_str().to_string(),
                        message: format!("where clause error: {}", msg),
                        reference: check.reference.clone(),
                        actual_value: None,
                        expected: None,
                        line: elem_ctx.element.span.start.line,
                        file_id: elem_ctx.element.span.file_id,
                    });
                    continue;
                }
                _ => continue,
            }
        }

        // Evaluate assert clause
        match eval_expression(assert_clause, elem_ctx, model) {
            EvalResult::Bool(true) => {
                results.push(CheckResult {
                    check_id: check.id.clone(),
                    suite: suite_name.map(|s| s.to_string()),
                    status: CheckStatus::Pass,
                    severity: check.severity,
                    element_id: elem_ctx.element.id.clone(),
                    element_type: elem_ctx.element.element_type.as_str().to_string(),
                    message: check
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("check `{}` passed", check.id)),
                    reference: check.reference.clone(),
                    actual_value: None,
                    expected: None,
                    line: elem_ctx.element.span.start.line,
                    file_id: elem_ctx.element.span.file_id,
                });
            }
            EvalResult::Bool(false) => {
                let (actual, expected) = extract_comparison_details(assert_clause, elem_ctx, model);
                results.push(CheckResult {
                    check_id: check.id.clone(),
                    suite: suite_name.map(|s| s.to_string()),
                    status: CheckStatus::Fail,
                    severity: check.severity,
                    element_id: elem_ctx.element.id.clone(),
                    element_type: elem_ctx.element.element_type.as_str().to_string(),
                    message: check
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("check `{}` failed", check.id)),
                    reference: check.reference.clone(),
                    actual_value: actual,
                    expected,
                    line: elem_ctx.element.span.start.line,
                    file_id: elem_ctx.element.span.file_id,
                });
            }
            EvalResult::Error(msg) => {
                results.push(CheckResult {
                    check_id: check.id.clone(),
                    suite: suite_name.map(|s| s.to_string()),
                    status: CheckStatus::Error,
                    severity: check.severity,
                    element_id: elem_ctx.element.id.clone(),
                    element_type: elem_ctx.element.element_type.as_str().to_string(),
                    message: format!("assertion error: {}", msg),
                    reference: check.reference.clone(),
                    actual_value: None,
                    expected: None,
                    line: elem_ctx.element.span.start.line,
                    file_id: elem_ctx.element.span.file_id,
                });
            }
            _ => {}
        }
    }

    // If no elements matched, report that
    if elements.is_empty()
        && let Some(target) = target_type
    {
        results.push(CheckResult {
            check_id: check.id.clone(),
            suite: suite_name.map(|s| s.to_string()),
            status: CheckStatus::Skip,
            severity: check.severity,
            element_id: String::new(),
            element_type: target.to_string(),
            message: format!("no {} elements found in model", target),
            reference: check.reference.clone(),
            actual_value: None,
            expected: None,
            line: check.span.start.line,
            file_id: check.span.file_id,
        });
    }

    results
}

/// Context for evaluating expressions against a specific element.
pub struct ElementContext<'a> {
    pub element: &'a ResolvedElement,
    pub storey: Option<&'a ResolvedStorey>,
    pub space: Option<&'a ResolvedSpace>,
    pub building: Option<&'a ResolvedBuilding>,
    pub site: Option<&'a ResolvedSite>,
    /// Parent element (e.g., Wall for a Door/Window)
    pub host: Option<&'a ResolvedElement>,
}

/// Result of evaluating an expression.
#[derive(Debug)]
pub enum EvalResult {
    Number(f64),
    Bool(bool),
    Str(String),
    Error(String),
}

/// A stair flight (`ifc_class: "IfcStairFlight"`, e.g. one of the flights a
/// stair with a landing generates) is checked through its stair, so checks on
/// `Stair` see each stair once.
fn is_checked(elem: &ResolvedElement) -> bool {
    !string_or_ident(&elem.properties, "ifc_class")
        .is_some_and(|c| c.eq_ignore_ascii_case("IfcStairFlight"))
}

/// Collect all elements matching a target type from the model.
fn collect_matching_elements<'a>(
    model: &'a SemanticModel,
    target_type: Option<&str>,
) -> Vec<ElementContext<'a>> {
    let mut contexts = Vec::new();

    // Elements directly in a storey, building or site, then those in its spaces.
    for c in model.containers() {
        let in_spaces = c
            .spaces
            .iter()
            .flat_map(|sp| sp.elements.iter().map(move |e| (e, Some(sp))));
        for (elem, space) in c.elements.iter().map(|e| (e, None)).chain(in_spaces) {
            collect_element_contexts(
                elem,
                None,
                c.storey,
                space,
                c.building,
                Some(c.site),
                target_type,
                &mut contexts,
            );
        }
    }

    contexts
}

#[allow(clippy::too_many_arguments)]
fn collect_element_contexts<'a>(
    elem: &'a ResolvedElement,
    host: Option<&'a ResolvedElement>,
    storey: Option<&'a ResolvedStorey>,
    space: Option<&'a ResolvedSpace>,
    building: Option<&'a ResolvedBuilding>,
    site: Option<&'a ResolvedSite>,
    target_type: Option<&str>,
    contexts: &mut Vec<ElementContext<'a>>,
) {
    let type_str = elem.element_type.as_str();
    let matches = is_checked(elem) && (target_type.is_none() || target_type == Some(type_str));

    if matches {
        contexts.push(ElementContext {
            element: elem,
            storey,
            space,
            building,
            site,
            host,
        });
    }

    // Recurse into children (e.g., Window/Door inside Wall)
    for child in &elem.children {
        collect_element_contexts(
            child,
            Some(elem),
            storey,
            space,
            building,
            site,
            target_type,
            contexts,
        );
    }
}

/// Evaluate a check expression in the context of an element.
fn eval_expression(
    expr: &CheckExpression,
    ctx: &ElementContext,
    model: &SemanticModel,
) -> EvalResult {
    match expr {
        CheckExpression::Literal(b) => EvalResult::Bool(*b),
        CheckExpression::Comparison { left, op, right } => {
            let left_val = resolve_operand(left, ctx, model);
            let right_val = resolve_operand(right, ctx, model);
            eval_comparison(&left_val, *op, &right_val)
        }
        CheckExpression::In { left, values } => {
            let left_val = resolve_operand(left, ctx, model);
            match left_val {
                EvalResult::Str(s) => EvalResult::Bool(values.contains(&s)),
                EvalResult::Number(n) => {
                    let s = format_number(n);
                    EvalResult::Bool(values.contains(&s))
                }
                EvalResult::Error(e) => EvalResult::Error(e),
                _ => EvalResult::Error("cannot evaluate 'in' on boolean".into()),
            }
        }
        CheckExpression::And(a, b) => match eval_expression(a, ctx, model) {
            EvalResult::Bool(true) => eval_expression(b, ctx, model),
            EvalResult::Bool(false) => EvalResult::Bool(false),
            other => other,
        },
        CheckExpression::Or(a, b) => match eval_expression(a, ctx, model) {
            EvalResult::Bool(true) => EvalResult::Bool(true),
            EvalResult::Bool(false) => eval_expression(b, ctx, model),
            other => other,
        },
        CheckExpression::Forall {
            element_type,
            filter,
            assertion,
        } => eval_forall(element_type, filter.as_deref(), assertion, ctx, model),
        CheckExpression::Exists {
            element_type,
            filter,
        } => eval_exists(element_type, filter.as_deref(), ctx, model),
    }
}

/// Resolve an operand to a concrete value.
fn resolve_operand(
    operand: &CheckOperand,
    ctx: &ElementContext,
    model: &SemanticModel,
) -> EvalResult {
    match operand {
        CheckOperand::Number(n) => EvalResult::Number(*n),
        // The semantic model holds SI values, so compare in SI too
        CheckOperand::NumberWithUnit(n, unit) => EvalResult::Number(
            n * crate::semantic::unit_normalize::unit_to_si_factor(unit).unwrap_or(1.0),
        ),
        CheckOperand::Boolean(b) => EvalResult::Bool(*b),
        CheckOperand::StringLiteral(s) => EvalResult::Str(s.clone()),
        CheckOperand::Property(name) => resolve_property(name, ctx, model),
        CheckOperand::Path(segments) => resolve_path(segments, ctx, model),
        CheckOperand::FunctionCall { name, args } => eval_function(name, args, ctx, model),
    }
}

/// Resolve a simple property name on the current element.
fn resolve_property(name: &str, ctx: &ElementContext, model: &SemanticModel) -> EvalResult {
    // First check computed properties
    if let Some(val) = computed::compute_property(name, ctx, model) {
        return val;
    }

    // Then check element properties
    for prop in &ctx.element.properties {
        if prop.key == name {
            return resolved_value_to_eval(&prop.value);
        }
    }

    EvalResult::Error(format!(
        "property `{}` not found on {} `{}`",
        name,
        ctx.element.element_type.as_str(),
        ctx.element.id
    ))
}

/// Resolve a dotted path (e.g., host.space.usage).
fn resolve_path(segments: &[String], ctx: &ElementContext, model: &SemanticModel) -> EvalResult {
    if segments.is_empty() {
        return EvalResult::Error("empty path".into());
    }

    match segments[0].as_str() {
        "host" => {
            if let Some(host) = ctx.host {
                if segments.len() == 1 {
                    return EvalResult::Error("incomplete path: host".into());
                }
                // host.property or host.space.property
                match segments[1].as_str() {
                    "space" => {
                        if let Some(space) = ctx.space
                            && segments.len() >= 3
                        {
                            resolve_spatial_property(&segments[2], space.properties.as_slice())
                        } else {
                            EvalResult::Error("no parent space context".into())
                        }
                    }
                    "storey" => {
                        if let Some(storey) = ctx.storey
                            && segments.len() >= 3
                        {
                            resolve_spatial_property(&segments[2], storey.properties.as_slice())
                        } else {
                            EvalResult::Error("no parent storey context".into())
                        }
                    }
                    _ => {
                        // host.property_name
                        for prop in &host.properties {
                            if prop.key == segments[1] {
                                return resolved_value_to_eval(&prop.value);
                            }
                        }
                        EvalResult::Error(format!("property `{}` not found on host", segments[1]))
                    }
                }
            } else {
                EvalResult::Error("element has no host (not nested)".into())
            }
        }
        "space" => {
            if let Some(space) = ctx.space
                && segments.len() >= 2
            {
                resolve_spatial_property(&segments[1], space.properties.as_slice())
            } else {
                EvalResult::Error("no space context".into())
            }
        }
        "storey" => {
            if let Some(storey) = ctx.storey
                && segments.len() >= 2
            {
                resolve_spatial_property(&segments[1], storey.properties.as_slice())
            } else {
                EvalResult::Error("no storey context".into())
            }
        }
        "building" => {
            if let Some(building) = ctx.building
                && segments.len() >= 2
            {
                resolve_spatial_property(&segments[1], building.properties.as_slice())
            } else {
                EvalResult::Error("no building context".into())
            }
        }
        _ => {
            // Try as element property path
            resolve_property(&segments.join("."), ctx, model)
        }
    }
}

fn resolve_spatial_property(name: &str, props: &[ResolvedProperty]) -> EvalResult {
    for prop in props {
        if prop.key == name {
            return resolved_value_to_eval(&prop.value);
        }
    }
    EvalResult::Error(format!("property `{}` not found", name))
}

fn resolved_value_to_eval(val: &ResolvedValue) -> EvalResult {
    match val {
        ResolvedValue::Number(n) => EvalResult::Number(*n),
        ResolvedValue::Boolean(b) => EvalResult::Bool(*b),
        ResolvedValue::String(s) => EvalResult::Str(s.clone()),
        ResolvedValue::Identifier(s) => EvalResult::Str(s.clone()),
        ResolvedValue::EnumList(vals) => EvalResult::Str(vals.join(", ")),
        _ => EvalResult::Str(format!("{:?}", val)),
    }
}

/// Evaluate a comparison between two values.
fn eval_comparison(left: &EvalResult, op: ComparisonOp, right: &EvalResult) -> EvalResult {
    match (left, right) {
        (EvalResult::Number(a), EvalResult::Number(b)) => {
            let result = match op {
                ComparisonOp::Ge => a >= b,
                ComparisonOp::Le => a <= b,
                ComparisonOp::Eq => (a - b).abs() < f64::EPSILON,
                ComparisonOp::Ne => (a - b).abs() >= f64::EPSILON,
                ComparisonOp::Gt => a > b,
                ComparisonOp::Lt => a < b,
            };
            EvalResult::Bool(result)
        }
        (EvalResult::Bool(a), EvalResult::Bool(b)) => {
            let result = match op {
                ComparisonOp::Eq => a == b,
                ComparisonOp::Ne => a != b,
                _ => {
                    return EvalResult::Error(format!(
                        "cannot compare booleans with {}",
                        op.as_str()
                    ));
                }
            };
            EvalResult::Bool(result)
        }
        (EvalResult::Str(a), EvalResult::Str(b)) => {
            let result = match op {
                ComparisonOp::Eq => a == b,
                ComparisonOp::Ne => a != b,
                _ => {
                    return EvalResult::Error(format!(
                        "cannot compare strings with {}",
                        op.as_str()
                    ));
                }
            };
            EvalResult::Bool(result)
        }
        (EvalResult::Error(e), _) | (_, EvalResult::Error(e)) => EvalResult::Error(e.clone()),
        _ => EvalResult::Error("type mismatch in comparison".into()),
    }
}

/// Evaluate a function call.
fn eval_function(
    name: &str,
    args: &[CheckOperand],
    ctx: &ElementContext,
    model: &SemanticModel,
) -> EvalResult {
    // Built-in aggregation functions
    match name {
        "count" => {
            // count() — count elements of the same type in the storey
            if let Some(storey) = ctx.storey {
                let type_str = ctx.element.element_type.as_str();
                let count = storey
                    .elements
                    .iter()
                    .filter(|e| e.element_type.as_str() == type_str && is_checked(e))
                    .count();
                EvalResult::Number(count as f64)
            } else {
                EvalResult::Number(0.0)
            }
        }
        "sum" => eval_aggregation(args, ctx, |values| values.iter().sum()),
        "min" => eval_aggregation(args, ctx, |values| {
            values.iter().copied().fold(f64::INFINITY, f64::min)
        }),
        "max" => eval_aggregation(args, ctx, |values| {
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        }),
        _ => {
            // Try as a computed property
            if let Some(val) = computed::compute_property(name, ctx, model) {
                val
            } else {
                EvalResult::Error(format!("unknown function `{}`", name))
            }
        }
    }
}

/// Evaluate an aggregation function (sum, min, max) over elements of the same type in the storey.
///
/// Usage: `sum(property_name)` — aggregates the named property across all sibling elements.
fn eval_aggregation(
    args: &[CheckOperand],
    ctx: &ElementContext,
    aggregate: impl Fn(&[f64]) -> f64,
) -> EvalResult {
    // Extract the property name from the first argument
    let prop_name = match args.first() {
        Some(CheckOperand::Property(name)) => name.as_str(),
        Some(CheckOperand::StringLiteral(name)) => name.as_str(),
        None => {
            return EvalResult::Error(
                "aggregation function requires a property name argument".into(),
            );
        }
        _ => {
            return EvalResult::Error(
                "aggregation function argument must be a property name".into(),
            );
        }
    };

    let storey = match ctx.storey {
        Some(s) => s,
        None => return EvalResult::Number(0.0),
    };

    let type_str = ctx.element.element_type.as_str();

    // Collect numeric values of the named property from all elements of the same type
    let mut values: Vec<f64> = Vec::new();

    // Elements directly in storey
    collect_property_values(&storey.elements, type_str, prop_name, &mut values);

    // Elements in spaces
    for space in &storey.spaces {
        collect_property_values(&space.elements, type_str, prop_name, &mut values);
    }

    if values.is_empty() {
        return EvalResult::Error(format!(
            "no numeric values found for property `{}` on {} elements",
            prop_name, type_str
        ));
    }

    EvalResult::Number(aggregate(&values))
}

/// Collect numeric property values from elements matching a type.
fn collect_property_values(
    elements: &[ResolvedElement],
    type_str: &str,
    prop_name: &str,
    values: &mut Vec<f64>,
) {
    for elem in elements {
        if elem.element_type.as_str() == type_str && is_checked(elem) {
            for prop in &elem.properties {
                if prop.key == prop_name
                    && let ResolvedValue::Number(n) = &prop.value
                {
                    values.push(*n);
                }
            }
        }
        // Also check children (e.g., doors in walls)
        collect_property_values(&elem.children, type_str, prop_name, values);
    }
}

/// Evaluate a `forall` quantifier: true iff every matching element satisfies the assertion.
fn eval_forall(
    element_type: &str,
    filter: Option<&CheckExpression>,
    assertion: &CheckExpression,
    _ctx: &ElementContext,
    model: &SemanticModel,
) -> EvalResult {
    let elements = collect_matching_elements(model, Some(element_type));
    if elements.is_empty() {
        // Vacuous truth: forall over an empty set is true
        return EvalResult::Bool(true);
    }

    for elem_ctx in &elements {
        // Apply filter if present
        if let Some(f) = filter {
            match eval_expression(f, elem_ctx, model) {
                EvalResult::Bool(false) => continue, // doesn't match filter, skip
                EvalResult::Error(e) => return EvalResult::Error(e),
                _ => {} // passes filter
            }
        }
        // Evaluate assertion
        match eval_expression(assertion, elem_ctx, model) {
            EvalResult::Bool(true) => {} // passes
            EvalResult::Bool(false) => return EvalResult::Bool(false),
            EvalResult::Error(e) => return EvalResult::Error(e),
            _ => return EvalResult::Error("forall assertion did not return boolean".into()),
        }
    }
    EvalResult::Bool(true)
}

/// Evaluate an `exists` quantifier: true iff at least one matching element satisfies the filter.
fn eval_exists(
    element_type: &str,
    filter: Option<&CheckExpression>,
    _ctx: &ElementContext,
    model: &SemanticModel,
) -> EvalResult {
    let elements = collect_matching_elements(model, Some(element_type));
    if elements.is_empty() {
        return EvalResult::Bool(false);
    }

    for elem_ctx in &elements {
        if let Some(f) = filter {
            match eval_expression(f, elem_ctx, model) {
                EvalResult::Bool(true) => return EvalResult::Bool(true),
                EvalResult::Error(e) => return EvalResult::Error(e),
                _ => {} // doesn't match, continue
            }
        } else {
            // No filter — existence alone is enough
            return EvalResult::Bool(true);
        }
    }
    EvalResult::Bool(false)
}

/// Extract actual vs expected values from a comparison for error reporting.
fn extract_comparison_details(
    expr: &CheckExpression,
    ctx: &ElementContext,
    model: &SemanticModel,
) -> (Option<String>, Option<String>) {
    if let CheckExpression::Comparison { left, op, right } = expr {
        let left_val = resolve_operand(left, ctx, model);
        let right_val = resolve_operand(right, ctx, model);

        let actual = match left_val {
            EvalResult::Number(n) => Some(format_number(n)),
            EvalResult::Bool(b) => Some(b.to_string()),
            EvalResult::Str(s) => Some(s),
            EvalResult::Error(_) => None,
        };

        let expected = match right_val {
            EvalResult::Number(n) => Some(format!("{} {}", op.as_str(), format_number(n))),
            EvalResult::Bool(b) => Some(format!("{} {}", op.as_str(), b)),
            EvalResult::Str(s) => Some(format!("{} {}", op.as_str(), s)),
            EvalResult::Error(_) => None,
        };

        (actual, expected)
    } else if let CheckExpression::In { left, values } = expr {
        let left_val = resolve_operand(left, ctx, model);
        let actual = match left_val {
            EvalResult::Str(s) => Some(s),
            EvalResult::Number(n) => Some(format_number(n)),
            _ => None,
        };
        let expected = Some(format!("in [{}]", values.join(", ")));
        (actual, expected)
    } else {
        (None, None)
    }
}

fn format_number(n: f64) -> String {
    if n == n.floor() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{:.2}", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_model() -> SemanticModel {
        SemanticModel {
            asset_libraries: Vec::new(),
            sites: vec![ResolvedSite {
                name: "TestSite".to_string(),
                implicit: false,
                properties: vec![],
                spaces: Vec::new(),
                elements: Vec::new(),
                buildings: vec![ResolvedBuilding {
                    name: "TestBuilding".to_string(),
                    properties: vec![],
                    spaces: Vec::new(),
                    elements: Vec::new(),
                    storeys: vec![ResolvedStorey {
                        name: "GF".to_string(),
                        properties: vec![ResolvedProperty {
                            key: "elevation".to_string(),
                            value: ResolvedValue::Number(0.0),
                            span: crate::ast::span::Span {
                                start: crate::ast::span::Position {
                                    line: 1,
                                    column: 1,
                                    byte_offset: 0,
                                },
                                end: crate::ast::span::Position {
                                    line: 1,
                                    column: 1,
                                    byte_offset: 0,
                                },
                                file_id: 0,
                            },
                        }],
                        spaces: vec![ResolvedSpace {
                            name: "Corridor".to_string(),
                            properties: vec![ResolvedProperty {
                                key: "usage".to_string(),
                                value: ResolvedValue::Identifier("corridor".to_string()),
                                span: dummy_span(),
                            }],
                            elements: vec![make_test_wall()],
                            span: dummy_span(),
                            curves: Default::default(),
                        }],
                        elements: vec![],
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                }],
                span: dummy_span(),
            }],
            materials: vec![],
        }
    }

    fn dummy_span() -> crate::ast::span::Span {
        crate::ast::span::Span {
            start: crate::ast::span::Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
            end: crate::ast::span::Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
            file_id: 0,
        }
    }

    fn make_test_wall() -> ResolvedElement {
        ResolvedElement {
            element_type: ElementType::Wall,
            id: "W-01".to_string(),
            properties: vec![
                ResolvedProperty {
                    key: "height".to_string(),
                    value: ResolvedValue::Number(2.8),
                    span: dummy_span(),
                },
                ResolvedProperty {
                    key: "thickness".to_string(),
                    value: ResolvedValue::Number(0.24),
                    span: dummy_span(),
                },
                ResolvedProperty {
                    key: "is_external".to_string(),
                    value: ResolvedValue::Boolean(true),
                    span: dummy_span(),
                },
                ResolvedProperty {
                    key: "fire_rating".to_string(),
                    value: ResolvedValue::Identifier("REI90".to_string()),
                    span: dummy_span(),
                },
            ],
            children: vec![ResolvedElement {
                element_type: ElementType::Door,
                id: "D-01".to_string(),
                properties: vec![
                    ResolvedProperty {
                        key: "width".to_string(),
                        value: ResolvedValue::Number(0.85),
                        span: dummy_span(),
                    },
                    ResolvedProperty {
                        key: "height".to_string(),
                        value: ResolvedValue::Number(2.1),
                        span: dummy_span(),
                    },
                ],
                children: vec![],
                span: dummy_span(),
                curves: Default::default(),
            }],
            span: dummy_span(),
            curves: Default::default(),
        }
    }

    #[test]
    fn test_check_passes() {
        let model = make_test_model();
        let check = Check {
            id: "wall_height_check".to_string(),
            description: Some("Walls must be at least 2.5m".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Property("height".to_string()),
                op: ComparisonOp::Ge,
                right: CheckOperand::Number(2.5),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_fails() {
        let model = make_test_model();
        let check = Check {
            id: "min_door_width".to_string(),
            description: Some("Doors must be >= 0.90 m".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Door".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Property("width".to_string()),
                op: ComparisonOp::Ge,
                right: CheckOperand::Number(0.90),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        // Door width is 0.85, check requires >= 0.90
        assert_eq!(results[0].status, CheckStatus::Fail);
        assert!(results[0].actual_value.is_some());
    }

    #[test]
    fn test_check_number_with_unit_in_si() {
        let model = make_test_model();
        let check = Check {
            id: "min_door_width_mm".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Door".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Property("width".to_string()),
                op: ComparisonOp::Ge,
                right: CheckOperand::NumberWithUnit(800.0, "mm".to_string()),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        // Door width 0.85 m >= 800 mm
        let results = evaluate_check(&check, None, &model);
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_with_where_clause() {
        let model = make_test_model();
        let check = Check {
            id: "fire_rated_external_walls".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Property("is_external".to_string()),
                op: ComparisonOp::Eq,
                right: CheckOperand::Boolean(true),
            }),
            assert_clause: Some(CheckExpression::In {
                left: CheckOperand::Property("fire_rating".to_string()),
                values: vec![
                    "REI30".to_string(),
                    "REI60".to_string(),
                    "REI90".to_string(),
                    "REI120".to_string(),
                ],
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_in_expression() {
        let model = make_test_model();
        let check = Check {
            id: "wall_fire_rating".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Warning,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::In {
                left: CheckOperand::Property("fire_rating".to_string()),
                values: vec!["REI60".to_string(), "REI90".to_string()],
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass); // REI90 is in the list
    }

    // ====================================================================
    // Aggregation function tests (sum, min, max)
    // ====================================================================

    /// Build a model with multiple walls for aggregation testing.
    fn make_multi_wall_model() -> SemanticModel {
        let walls = vec![
            ResolvedElement {
                element_type: ElementType::Wall,
                id: "W-01".to_string(),
                properties: vec![
                    ResolvedProperty {
                        key: "height".to_string(),
                        value: ResolvedValue::Number(2.8),
                        span: dummy_span(),
                    },
                    ResolvedProperty {
                        key: "thickness".to_string(),
                        value: ResolvedValue::Number(0.24),
                        span: dummy_span(),
                    },
                ],
                children: vec![],
                span: dummy_span(),
                curves: Default::default(),
            },
            ResolvedElement {
                element_type: ElementType::Wall,
                id: "W-02".to_string(),
                properties: vec![
                    ResolvedProperty {
                        key: "height".to_string(),
                        value: ResolvedValue::Number(3.0),
                        span: dummy_span(),
                    },
                    ResolvedProperty {
                        key: "thickness".to_string(),
                        value: ResolvedValue::Number(0.30),
                        span: dummy_span(),
                    },
                ],
                children: vec![],
                span: dummy_span(),
                curves: Default::default(),
            },
            ResolvedElement {
                element_type: ElementType::Wall,
                id: "W-03".to_string(),
                properties: vec![
                    ResolvedProperty {
                        key: "height".to_string(),
                        value: ResolvedValue::Number(2.5),
                        span: dummy_span(),
                    },
                    ResolvedProperty {
                        key: "thickness".to_string(),
                        value: ResolvedValue::Number(0.20),
                        span: dummy_span(),
                    },
                ],
                children: vec![],
                span: dummy_span(),
                curves: Default::default(),
            },
        ];

        SemanticModel {
            asset_libraries: Vec::new(),
            sites: vec![ResolvedSite {
                name: "S".to_string(),
                implicit: false,
                properties: vec![],
                spaces: Vec::new(),
                elements: Vec::new(),
                buildings: vec![ResolvedBuilding {
                    name: "B".to_string(),
                    properties: vec![],
                    spaces: Vec::new(),
                    elements: Vec::new(),
                    storeys: vec![ResolvedStorey {
                        name: "GF".to_string(),
                        properties: vec![],
                        spaces: vec![],
                        elements: walls,
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                }],
                span: dummy_span(),
            }],
            materials: vec![],
        }
    }

    #[test]
    fn test_sum_aggregation() {
        let model = make_multi_wall_model();
        // sum(thickness) should be 0.24 + 0.30 + 0.20 = 0.74
        let check = Check {
            id: "sum_thickness".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Warning,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::FunctionCall {
                    name: "sum".to_string(),
                    args: vec![CheckOperand::Property("thickness".to_string())],
                },
                op: ComparisonOp::Gt,
                right: CheckOperand::Number(0.5),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        // All 3 walls should pass (sum is 0.74 > 0.5)
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(
                r.status,
                CheckStatus::Pass,
                "wall {} should pass",
                r.element_id
            );
        }
    }

    #[test]
    fn test_min_aggregation() {
        let model = make_multi_wall_model();
        // min(height) should be 2.5
        let check = Check {
            id: "min_height".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::FunctionCall {
                    name: "min".to_string(),
                    args: vec![CheckOperand::Property("height".to_string())],
                },
                op: ComparisonOp::Ge,
                right: CheckOperand::Number(2.5),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "min height should be >= 2.5");
        }
    }

    #[test]
    fn test_max_aggregation() {
        let model = make_multi_wall_model();
        // max(height) should be 3.0
        let check = Check {
            id: "max_height".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::FunctionCall {
                    name: "max".to_string(),
                    args: vec![CheckOperand::Property("height".to_string())],
                },
                op: ComparisonOp::Le,
                right: CheckOperand::Number(3.0),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "max height should be <= 3.0");
        }
    }

    #[test]
    fn test_max_aggregation_fails() {
        let model = make_multi_wall_model();
        // max(height) is 3.0, checking < 2.9 should fail
        let check = Check {
            id: "max_too_low".to_string(),
            description: Some("Max height must be < 2.9".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::FunctionCall {
                    name: "max".to_string(),
                    args: vec![CheckOperand::Property("height".to_string())],
                },
                op: ComparisonOp::Lt,
                right: CheckOperand::Number(2.9),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Fail, "max 3.0 is not < 2.9");
        }
    }

    // ====================================================================
    // Quantifier tests (forall, exists)
    // ====================================================================

    #[test]
    fn test_forall_all_pass() {
        let model = make_multi_wall_model();
        // forall Wall: height >= 2.0 — should be true (all heights are >= 2.0)
        let check = Check {
            id: "forall_height".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Forall {
                element_type: "Wall".to_string(),
                filter: None,
                assertion: Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(2.0),
                }),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        // Each wall gets checked, forall evaluated inside assert
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(
                r.status,
                CheckStatus::Pass,
                "forall height >= 2.0 should pass"
            );
        }
    }

    #[test]
    fn test_forall_some_fail() {
        let model = make_multi_wall_model();
        // forall Wall: height >= 2.9 — should fail (W-01 is 2.8, W-03 is 2.5)
        let check = Check {
            id: "forall_height_strict".to_string(),
            description: Some("All walls must be >= 2.9m".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Forall {
                element_type: "Wall".to_string(),
                filter: None,
                assertion: Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(2.9),
                }),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(
                r.status,
                CheckStatus::Fail,
                "forall height >= 2.9 should fail"
            );
        }
    }

    #[test]
    fn test_forall_with_filter() {
        let model = make_multi_wall_model();
        // forall Wall where thickness >= 0.24: height >= 2.8
        // Walls matching filter: W-01 (0.24, h=2.8), W-02 (0.30, h=3.0)
        // Both have height >= 2.8, so should pass
        let check = Check {
            id: "forall_filtered".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Forall {
                element_type: "Wall".to_string(),
                filter: Some(Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("thickness".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(0.24),
                })),
                assertion: Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(2.8),
                }),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass);
        }
    }

    #[test]
    fn test_exists_found() {
        let model = make_multi_wall_model();
        // exists Wall where height >= 3.0 — W-02 has height 3.0
        let check = Check {
            id: "exists_tall".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Exists {
                element_type: "Wall".to_string(),
                filter: Some(Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(3.0),
                })),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "should find a wall >= 3.0m");
        }
    }

    #[test]
    fn test_exists_not_found() {
        let model = make_multi_wall_model();
        // exists Wall where height >= 4.0 — none are that tall
        let check = Check {
            id: "exists_very_tall".to_string(),
            description: Some("Need at least one wall >= 4m".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Exists {
                element_type: "Wall".to_string(),
                filter: Some(Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(4.0),
                })),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Fail, "no wall is >= 4.0m");
        }
    }

    #[test]
    fn test_exists_no_filter() {
        let model = make_multi_wall_model();
        // exists Wall (no filter) — should be true since walls exist
        let check = Check {
            id: "exists_any_wall".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Exists {
                element_type: "Wall".to_string(),
                filter: None,
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "walls exist");
        }
    }

    #[test]
    fn test_exists_empty_type() {
        let model = make_multi_wall_model();
        // exists Column — no columns in model
        let check = Check {
            id: "exists_column".to_string(),
            description: Some("Need at least one column".to_string()),
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Exists {
                element_type: "Column".to_string(),
                filter: None,
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Fail, "no columns exist");
        }
    }

    #[test]
    fn test_forall_vacuous_truth() {
        let model = make_multi_wall_model();
        // forall Column: height >= 100 — no columns, so vacuously true
        let check = Check {
            id: "forall_no_columns".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Forall {
                element_type: "Column".to_string(),
                filter: None,
                assertion: Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(100.0),
                }),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "forall over empty set is true");
        }
    }

    // ====================================================================
    // Count function test
    // ====================================================================

    #[test]
    fn test_count_function() {
        let model = make_multi_wall_model();
        // count() should return 3 (three walls in storey)
        let check = Check {
            id: "wall_count".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::FunctionCall {
                    name: "count".to_string(),
                    args: vec![],
                },
                op: ComparisonOp::Eq,
                right: CheckOperand::Number(3.0),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        for r in &results {
            assert_eq!(r.status, CheckStatus::Pass, "should be 3 walls");
        }
    }

    // ====================================================================
    // And/Or logic tests
    // ====================================================================

    #[test]
    fn test_and_expression() {
        let model = make_test_model();
        let check = Check {
            id: "and_test".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::And(
                Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(2.5),
                }),
                Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("thickness".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(0.20),
                }),
            )),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_or_expression() {
        let model = make_test_model();
        // height >= 5.0 OR thickness >= 0.20 — first is false, second is true
        let check = Check {
            id: "or_test".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Or(
                Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("height".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(5.0),
                }),
                Box::new(CheckExpression::Comparison {
                    left: CheckOperand::Property("thickness".to_string()),
                    op: ComparisonOp::Ge,
                    right: CheckOperand::Number(0.20),
                }),
            )),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    // ====================================================================
    // Path resolution tests
    // ====================================================================

    #[test]
    fn test_space_usage_path() {
        let model = make_test_model();
        // The test wall is in a space with usage: corridor
        let check = Check {
            id: "space_usage".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Wall".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Path(vec!["space".to_string(), "usage".to_string()]),
                op: ComparisonOp::Eq,
                right: CheckOperand::StringLiteral("corridor".to_string()),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }

    #[test]
    fn test_host_property_path() {
        let model = make_test_model();
        // The door's host wall has height 2.8
        let check = Check {
            id: "door_host_height".to_string(),
            description: None,
            reference: None,
            severity: CheckSeverity::Error,
            applies_to: Some("Door".to_string()),
            where_clause: None,
            assert_clause: Some(CheckExpression::Comparison {
                left: CheckOperand::Path(vec!["host".to_string(), "height".to_string()]),
                op: ComparisonOp::Ge,
                right: CheckOperand::Number(2.5),
            }),
            depends_on: vec![],
            span: dummy_span(),
        };

        let results = evaluate_check(&check, None, &model);
        assert!(!results.is_empty());
        assert_eq!(results[0].status, CheckStatus::Pass);
    }
}
