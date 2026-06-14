//! PORT of `packages/workflows/src/condition-evaluator.ts`.
//!
//! UNIT WF-12: Condition Evaluator — compound boolean expression evaluator for `when:` fields.
//!
//! # Supported syntax (condition-evaluator.ts:3-16)
//!
//! - `$nodeId.output == 'VALUE'`          — string equality
//! - `$nodeId.output != 'VALUE'`          — string inequality
//! - `$nodeId.output.field == 'VALUE'`    — dot notation (field on structured output)
//! - `$nodeId.field == 'VALUE'`           — shorthand (≡ `$nodeId.output.field`)
//! - `$nodeId.output > 80`                — numeric comparison (>, >=, <, <=)
//! - RHS may be quoted (`'VALUE'`) or unquoted (`0`, `true`, `false`, `-1.5`)
//! - Compound: `$a.output == 'X' && $b.output != 'Y'` — AND higher precedence;
//!   `$a.output == 'X' || $b.output == 'Y'` — OR lower precedence
//! - No parentheses.
//!
//! # Error asymmetry (load-bearing — must be preserved exactly)
//!
//! | Situation                                        | Result                                   |
//! |--------------------------------------------------|------------------------------------------|
//! | Malformed expression (bad syntax, bad shorthand) | `{result:false, parsed:false}` — **SKIP**|
//! | Unresolvable `$node.output.field` reference      | `Err(OutputRefError)` — node **FAILS**   |
//!
//! Parse failures are fail-closed (return `false`, do NOT propagate as an error).
//! Unresolvable field references propagate as `OutputRefError` — they are NOT caught here.
//!
//! # Short-circuit evaluation
//!
//! - AND (`&&`) short-circuits on first `false`.
//! - OR (`||`) short-circuits on first `true`.
//! - Evaluation order matches source (left-to-right).
//!
//! # Quote-aware splitting
//!
//! `split_outside_quotes` splits on `&&` or `||` separators but skips separators that occur
//! inside single-quoted regions. This prevents `'hello && world'` from being split.
//!
//! # Atom pattern (condition-evaluator.ts:117-118)
//!
//! ```text
//! ^\$([a-zA-Z_][a-zA-Z0-9_-]*)\.([a-zA-Z_][a-zA-Z0-9_]*)(?:\.([a-zA-Z_][a-zA-Z0-9_]*))?\s*(==|!=|<=|>=|<|>)\s*(?:'([^']*)'|(-?\d+(?:\.\d+)?|true|false))$
//! ```
//!
//! Capture groups:
//!   1. `nodeId`        — `$nodeId`
//!   2. `segment1`      — first path segment after the node (`output` or a shorthand field name)
//!   3. `segment2`      — optional second segment (the field name when segment1 is `output`)
//!   4. `operator`      — `== | != | <= | >= | < | >`
//!   5. `quotedValue`   — single-quoted RHS literal (may be empty string)
//!   6. `unquotedValue` — bare numeric (`-?\d+(?:\.\d+)?`) or boolean (`true` | `false`)

use std::collections::HashMap;

use har_workflow_schema::NodeOutput;

use crate::output_ref::{resolve_node_output_field, OutputRefError, FieldResolution};

// ---------------------------------------------------------------------------
// parse_float_js — JS parseFloat() semantics
// ---------------------------------------------------------------------------

/// Parse `s` as a floating-point number using JS `parseFloat()` semantics.
///
/// JS `parseFloat()` rules (ECMA-262):
/// 1. Skip leading ASCII whitespace (U+0009 TAB, U+000A LF, U+000B VT, U+000C FF,
///    U+000D CR, U+0020 SPACE — the same characters as `str::trim_start_matches`
///    on whitespace).
/// 2. If the remaining string starts with `"Infinity"` → `f64::INFINITY`
///    (JS: `parseFloat("Infinity")` → Infinity; the `is_finite()` guard in the caller
///    then maps that to `not-parsed`, matching existing oracle results).
/// 3. If the remaining string starts with `"-Infinity"` → `f64::NEG_INFINITY` (same guard).
/// 4. Parse the longest leading `[-+]? (\d+\.?\d* | \.\d+) ([eE][+-]?\d+)?` prefix.
///    Stop at the first character that does NOT extend this grammar.
///    - `"0x20"` → prefix is `"0"` (stops at `x`); result is `0.0` — NOT hex.
///    - `"20abc"` → prefix is `"20"`; result is `20.0`.
///    - `"5px"` → prefix is `"5"`; result is `5.0`.
///    - `"  20"` → whitespace stripped → `"20"`; result is `20.0`.
///    - `"\t20"` → whitespace stripped → `"20"`; result is `20.0`.
///    - `""`, `"abc"`, `" "` → no numeric prefix → `f64::NAN`.
///    - `".5"` → `0.5` (leading dot allowed when followed by digits).
///    - `"5."` → `5.0` (trailing dot allowed — it's the `\d+\.?\d*` branch with zero post-dot digits).
///    - `"+20"` → `20.0`.
///    - `"-3"` → `-3.0`.
///    - `"2e1"` → `20.0` (exponent parsed).
///    - `"NaN"` → `f64::NAN` (no numeric prefix → NaN).
///
/// Returns `f64::NAN` when no numeric prefix is found (caller uses `is_finite()` to gate).
fn parse_float_js(s: &str) -> f64 {
    // Step 1: skip leading ASCII whitespace.
    let s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());

    if s.is_empty() {
        return f64::NAN;
    }

    // Step 2/3: check for "Infinity" / "-Infinity" / "+Infinity" prefixes.
    // JS parseFloat recognises exactly "Infinity" (not "infinity" or "inf").
    if s.starts_with("Infinity") {
        return f64::INFINITY;
    }
    if s.starts_with("-Infinity") {
        return f64::NEG_INFINITY;
    }
    if s.starts_with("+Infinity") {
        return f64::INFINITY;
    }

    // Step 4: consume the longest valid numeric prefix.
    // Grammar: [-+]? ( \d+\.?\d* | \.\d+ ) ( [eE][+-]?\d+ )?
    let chars: &[u8] = s.as_bytes();
    let len = chars.len();
    let mut i = 0;

    // Optional leading sign.
    if i < len && (chars[i] == b'+' || chars[i] == b'-') {
        i += 1;
    }

    let digits_start = i;

    if i < len && chars[i] == b'.' {
        // Dot-first branch: `.digit+`
        i += 1;
        let frac_start = i;
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            // Lone `.` with no digits after — not a number.
            return f64::NAN;
        }
    } else {
        // Digit-first branch: `digit+ (. digit*)?`
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start {
            // No digits found at all.
            return f64::NAN;
        }
        // Optional decimal point + optional digits.
        if i < len && chars[i] == b'.' {
            i += 1;
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
    }

    // Optional exponent: [eE][+-]?\d+
    if i < len && (chars[i] == b'e' || chars[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < len && (chars[i] == b'+' || chars[i] == b'-') {
            i += 1;
        }
        let exp_digits_start = i;
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_digits_start {
            // `e`/`E` with no digits after it — roll back the exponent.
            i = exp_start;
        }
    }

    // Parse the extracted prefix.
    let prefix = &s[..i];
    prefix.parse::<f64>().unwrap_or(f64::NAN)
}

use once_cell::sync::Lazy;
use regex::Regex;

// ---------------------------------------------------------------------------
// atomPattern — mirrors condition-evaluator.ts:117-118 exactly.
// ---------------------------------------------------------------------------

/// Regex for a single atom condition expression.
///
/// Capture groups (1-indexed, matching the TS source comments):
///   1. nodeId        — `[a-zA-Z_][a-zA-Z0-9_-]*`  (allows hyphens)
///   2. segment1      — `[a-zA-Z_][a-zA-Z0-9_]*`   (no hyphens — field name)
///   3. segment2      — `[a-zA-Z_][a-zA-Z0-9_]*`   (optional, no hyphens — sub-field)
///   4. operator      — `==|!=|<=|>=|<|>`
///   5. quotedValue   — `[^']*`                     (inside single quotes)
///   6. unquotedValue — `-?\d+(?:\.\d+)?|true|false`
static ATOM_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        ^\$([a-zA-Z_][a-zA-Z0-9_-]*)        # 1: nodeId (allows hyphens)
        \.([a-zA-Z_][a-zA-Z0-9_]*)          # 2: segment1 (no hyphens)
        (?:\.([a-zA-Z_][a-zA-Z0-9_]*))?     # 3: optional segment2 (no hyphens)
        \s*(==|!=|<=|>=|<|>)\s*             # 4: operator
        (?:
            '([^']*)'                         # 5: quotedValue (inside single quotes)
          |(-?\d+(?:\.\d+)?|true|false)       # 6: unquotedValue
        )$",
    )
    .expect("ATOM_PATTERN is a valid regex")
});

// ---------------------------------------------------------------------------
// EvaluationResult
// ---------------------------------------------------------------------------

/// Result of evaluating a condition expression.
///
/// Mirrors `{ result: boolean; parsed: boolean }` in condition-evaluator.ts.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationResult {
    /// Whether the condition evaluates to true (run the node) or false (skip/fail-closed).
    pub result: bool,
    /// Whether the expression was successfully parsed. If `false`, the expression had
    /// a syntax error and `result` is always `false` (fail-closed).
    pub parsed: bool,
}

impl EvaluationResult {
    fn parsed_true() -> Self {
        Self { result: true, parsed: true }
    }

    fn parsed_false() -> Self {
        Self { result: false, parsed: true }
    }

    fn unparsed() -> Self {
        Self { result: false, parsed: false }
    }
}

// ---------------------------------------------------------------------------
// resolve_output_ref (internal — not exported from condition-evaluator)
// ---------------------------------------------------------------------------

/// Resolve a `$nodeId.output` or `$nodeId.output.field` reference to a string value.
///
/// - Unknown node → `''` (warn). condition-evaluator.ts:54-56.
/// - Whole-text `$node.output` (field is `None`) → output text ('' for failed/empty).
///   condition-evaluator.ts:58-63.
/// - `$node.output.field` → delegates to `resolve_node_output_field` (no-silent-drop contract).
///   Throws `OutputRefError` for the strict cases. condition-evaluator.ts:65-74.
///
/// The `OutputRefError` is intentionally NOT caught here — it must propagate to fail the node.
fn resolve_output_ref(
    node_id: &str,
    field: Option<&str>,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<String, OutputRefError> {
    let node_output = node_outputs.get(node_id);
    if node_output.is_none() {
        tracing::warn!(
            node_id = node_id,
            "condition_output_ref_unknown_node"
        );
        return Ok(String::new());
    }
    let node_output = node_output.unwrap();

    if field.is_none() {
        // Whole-text `$node.output` — structuredOutput shape is opaque; defer to output text.
        // Empty for failed/skipped/pending nodes that have no output.
        return Ok(node_output.output().to_string());
    }

    let field = field.unwrap();
    let resolution = resolve_node_output_field(node_output, node_id, field)?;

    match resolution {
        FieldResolution::Empty => Ok(String::new()),
        FieldResolution::Value(v) => {
            match &v {
                serde_json::Value::String(s) => Ok(s.clone()),
                serde_json::Value::Number(n) => Ok(n.to_string()),
                serde_json::Value::Bool(b) => Ok(b.to_string()),
                // Arrays, objects, AND null are JSON-stringified.
                // A present null on the lenient no-schema path stringifies to "null",
                // matching legacy structuredOutput-preference behavior.
                // condition-evaluator.ts:71-73.
                serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                    Ok(serde_json::to_string(&v).unwrap_or_default())
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// split_outside_quotes
// ---------------------------------------------------------------------------

/// Split a string on a separator, but only when not inside single-quoted regions.
///
/// Returns at least one element (the full trimmed string if no split occurs).
/// Mirrors `splitOutsideQuotes` in condition-evaluator.ts:81-100.
///
/// # Examples
///
/// ```text
/// split_outside_quotes("$a.output == 'X' && $b.output == 'Y'", "&&")
///   → ["$a.output == 'X'", "$b.output == 'Y'"]
///
/// split_outside_quotes("$a.output == 'X && Y'", "&&")
///   → ["$a.output == 'X && Y'"]   (no split: && is inside quotes)
/// ```
pub fn split_outside_quotes(expr: &str, sep: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let chars: Vec<char> = expr.chars().collect();
    let sep_chars: Vec<char> = sep.chars().collect();
    let sep_len = sep.len();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if chars[i] == '\'' {
            in_quote = !in_quote;
            current.push(chars[i]);
            i += 1;
        } else if !in_quote && i + sep_len <= n && chars[i..i + sep_len] == sep_chars[..] {
            parts.push(current.trim().to_string());
            current = String::new();
            i += sep_len;
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }
    parts.push(current.trim().to_string());
    parts
}

// ---------------------------------------------------------------------------
// evaluate_atom
// ---------------------------------------------------------------------------

/// Evaluate a single atomic condition expression against upstream node outputs.
///
/// Returns `EvaluationResult { result, parsed }`.
///
/// A parse failure (regex mismatch, unexpected None capture groups, shorthand-with-subfield)
/// returns `unparsed()` (fail-closed). An unresolvable `$node.output.field` propagates as
/// `Err(OutputRefError)`.
///
/// Mirrors `evaluateAtom` in condition-evaluator.ts:123-195.
fn evaluate_atom(
    expr: &str,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<EvaluationResult, OutputRefError> {
    let trimmed = expr.trim();
    let Some(caps) = ATOM_PATTERN.captures(trimmed) else {
        tracing::debug!(expr = expr, "condition_parse_failed");
        return Ok(EvaluationResult::unparsed());
    };

    // All 6 capture groups are present (regex guarantees it on a match).
    let node_id = match caps.get(1).map(|m| m.as_str()) {
        Some(v) => v,
        None => {
            tracing::debug!(expr = expr, "condition_parse_unexpected_undefined");
            return Ok(EvaluationResult::unparsed());
        }
    };
    let segment1 = match caps.get(2).map(|m| m.as_str()) {
        Some(v) => v,
        None => {
            tracing::debug!(expr = expr, "condition_parse_unexpected_undefined");
            return Ok(EvaluationResult::unparsed());
        }
    };
    let segment2 = caps.get(3).map(|m| m.as_str());
    let operator = match caps.get(4).map(|m| m.as_str()) {
        Some(v) => v,
        None => {
            tracing::debug!(expr = expr, "condition_parse_unexpected_undefined");
            return Ok(EvaluationResult::unparsed());
        }
    };
    let quoted_value = caps.get(5).map(|m| m.as_str());
    let unquoted_value = caps.get(6).map(|m| m.as_str());

    // Resolve the effective field, preserving canonical `$node.output[.field]` semantics
    // while accepting the `$node.field` shorthand.
    //   - `$node.output`        → bare output reference (field None)
    //   - `$node.output.field`  → field access on the output
    //   - `$node.field`         → shorthand, equivalent to `$node.output.field`
    // The shorthand form cannot carry a sub-field (`$node.field.sub` is rejected fail-closed).
    // condition-evaluator.ts:149-157.
    let field: Option<&str> = if segment1 == "output" {
        segment2 // may be None (bare `$node.output`) or Some("fieldname")
    } else {
        // shorthand: segment1 IS the field name
        if segment2.is_some() {
            // `$node.segment1.segment2` — only `$node.output.field` is legal with two segments
            tracing::debug!(expr = expr, "condition_parse_failed");
            return Ok(EvaluationResult::unparsed());
        }
        Some(segment1)
    };

    // Quoted RHS takes precedence; the unquoted alternative covers numbers and booleans.
    // condition-evaluator.ts:160-163.
    let expected = match (quoted_value, unquoted_value) {
        (Some(q), _) => q,
        (None, Some(u)) => u,
        (None, None) => {
            tracing::debug!(expr = expr, "condition_parse_unexpected_undefined");
            return Ok(EvaluationResult::unparsed());
        }
    };

    // resolve_output_ref may throw OutputRefError for an unresolvable `.field` ref.
    // It is deliberately NOT caught here — under the no-silent-drop contract it must
    // propagate to fail the node. condition-evaluator.ts:167-171.
    let actual = resolve_output_ref(node_id, field, node_outputs)?;

    let result: bool = match operator {
        "==" => actual == expected,
        "!=" => actual != expected,
        _ => {
            // Numeric comparison: both sides must parse as finite numbers.
            // Use JS parseFloat() semantics (lenient prefix parsing) to match
            // Archon's condition-evaluator.ts behavior exactly.
            let actual_num = parse_float_js(&actual);
            let expected_num = parse_float_js(expected);
            if actual_num.is_finite() && expected_num.is_finite() {
                match operator {
                    "<" => actual_num < expected_num,
                    ">" => actual_num > expected_num,
                    "<=" => actual_num <= expected_num,
                    ">=" => actual_num >= expected_num,
                    _ => {
                        tracing::debug!(
                            expr = expr,
                            actual = actual,
                            expected = expected,
                            "condition_numeric_parse_failed"
                        );
                        return Ok(EvaluationResult::unparsed());
                    }
                }
            } else {
                tracing::debug!(
                    expr = expr,
                    actual = actual,
                    expected = expected,
                    "condition_numeric_parse_failed"
                );
                return Ok(EvaluationResult::unparsed());
            }
        }
    };

    tracing::debug!(
        node_id = node_id,
        field = field.unwrap_or("(none)"),
        operator = operator,
        expected = expected,
        actual = actual,
        result = result,
        "condition_evaluated"
    );

    Ok(if result {
        EvaluationResult::parsed_true()
    } else {
        EvaluationResult::parsed_false()
    })
}

// ---------------------------------------------------------------------------
// evaluate_condition — public entry point
// ---------------------------------------------------------------------------

/// Evaluate a condition expression (possibly compound) against upstream node outputs.
///
/// Returns `EvaluationResult { result, parsed }`:
/// - `result = true`  → run this node
/// - `result = false` → skip this node (or the expression had a syntax error)
/// - `parsed = false` → expression could not be parsed (fail-closed: result is always false)
///
/// Propagates `OutputRefError` for unresolvable `$node.output.field` references
/// (those cause the consuming node to **fail**, not skip).
///
/// Mirrors `evaluateCondition` in condition-evaluator.ts:205-232.
///
/// # Precedence
///
/// AND (`&&`) binds tighter than OR (`||`). The expression is split on `||` first, then
/// each OR-clause is split on `&&`.
///
/// # Short-circuit
///
/// - AND: stops on first `false` (no further atoms in the AND-clause are evaluated)
/// - OR: stops on first `true` (no further OR-clauses are evaluated)
pub fn evaluate_condition(
    expr: &str,
    node_outputs: &HashMap<String, NodeOutput>,
) -> Result<EvaluationResult, OutputRefError> {
    let trimmed = expr.trim();

    // Split on || — OR has LOWER precedence.
    let or_clauses = split_outside_quotes(trimmed, "||");

    for or_clause in &or_clauses {
        // Split each OR clause on && — AND has HIGHER precedence.
        let and_atoms = split_outside_quotes(or_clause, "&&");
        let mut or_clause_result = true;

        for atom in &and_atoms {
            let eval = evaluate_atom(atom, node_outputs)?;
            if !eval.parsed {
                // Fail-closed on any parse error — stop immediately.
                // condition-evaluator.ts:221.
                return Ok(EvaluationResult::unparsed());
            }
            if !eval.result {
                or_clause_result = false;
                break; // short-circuit AND
            }
        }

        if or_clause_result {
            return Ok(EvaluationResult::parsed_true()); // short-circuit OR
        }
    }

    Ok(EvaluationResult::parsed_false())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_workflow_schema::NodeOutput;
    use serde_json::json;

    // ── Test helpers ──────────────────────────────────────────────────────

    fn outputs_with(node_id: &str, output: &str) -> HashMap<String, NodeOutput> {
        let mut m = HashMap::new();
        m.insert(
            node_id.to_string(),
            NodeOutput::Completed {
                output: output.to_string(),
                session_id: None,
                structured_output: None,
                declared_fields: None,
            },
        );
        m
    }

    fn outputs_with_structured(
        node_id: &str,
        output: &str,
        structured: serde_json::Value,
    ) -> HashMap<String, NodeOutput> {
        let mut m = HashMap::new();
        m.insert(
            node_id.to_string(),
            NodeOutput::Completed {
                output: output.to_string(),
                session_id: None,
                structured_output: Some(structured),
                declared_fields: None,
            },
        );
        m
    }

    fn outputs_with_declared(
        node_id: &str,
        output: &str,
        structured: serde_json::Value,
        declared: Vec<String>,
    ) -> HashMap<String, NodeOutput> {
        let mut m = HashMap::new();
        m.insert(
            node_id.to_string(),
            NodeOutput::Completed {
                output: output.to_string(),
                session_id: None,
                structured_output: Some(structured),
                declared_fields: Some(declared),
            },
        );
        m
    }

    fn eval(expr: &str, outputs: &HashMap<String, NodeOutput>) -> EvaluationResult {
        evaluate_condition(expr, outputs).expect("no OutputRefError expected")
    }

    // ── split_outside_quotes ──────────────────────────────────────────────

    #[test]
    fn split_no_separator_returns_one_element() {
        let parts = split_outside_quotes("$a.output == 'X'", "&&");
        assert_eq!(parts, vec!["$a.output == 'X'"]);
    }

    #[test]
    fn split_on_and_separator() {
        let parts = split_outside_quotes("$a.output == 'X' && $b.output == 'Y'", "&&");
        assert_eq!(parts, vec!["$a.output == 'X'", "$b.output == 'Y'"]);
    }

    #[test]
    fn split_does_not_split_inside_quotes() {
        let parts = split_outside_quotes("$a.output == 'X && Y'", "&&");
        assert_eq!(parts, vec!["$a.output == 'X && Y'"]);
    }

    #[test]
    fn split_on_or_separator() {
        let parts = split_outside_quotes("$a.output == 'X' || $b.output == 'Y'", "||");
        assert_eq!(parts, vec!["$a.output == 'X'", "$b.output == 'Y'"]);
    }

    #[test]
    fn split_or_inside_quotes_not_split() {
        let parts = split_outside_quotes("$a.output == 'X || Y'", "||");
        assert_eq!(parts, vec!["$a.output == 'X || Y'"]);
    }

    #[test]
    fn split_multiple_separators() {
        let parts = split_outside_quotes("$a.output == 'X' && $b.output == 'Y' && $c.output == 'Z'", "&&");
        assert_eq!(parts, vec!["$a.output == 'X'", "$b.output == 'Y'", "$c.output == 'Z'"]);
    }

    // ── evaluate_atom — string equality ───────────────────────────────────

    #[test]
    fn atom_equal_true() {
        let outputs = outputs_with("node1", "SUCCESS");
        let r = eval("$node1.output == 'SUCCESS'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_equal_false() {
        let outputs = outputs_with("node1", "FAIL");
        let r = eval("$node1.output == 'SUCCESS'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_not_equal_true() {
        let outputs = outputs_with("node1", "FAIL");
        let r = eval("$node1.output != 'SUCCESS'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_not_equal_false() {
        let outputs = outputs_with("node1", "SUCCESS");
        let r = eval("$node1.output != 'SUCCESS'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    // ── evaluate_atom — unquoted RHS ──────────────────────────────────────

    #[test]
    fn atom_unquoted_true_boolean() {
        let outputs = outputs_with("node1", "true");
        let r = eval("$node1.output == true", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_unquoted_false_boolean() {
        let outputs = outputs_with("node1", "false");
        let r = eval("$node1.output == false", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_unquoted_integer() {
        let outputs = outputs_with("node1", "42");
        let r = eval("$node1.output == 42", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_unquoted_zero() {
        let outputs = outputs_with("node1", "0");
        let r = eval("$node1.output == 0", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    // ── parse_float_js unit tests ────────────────────────────────────────

    #[test]
    fn parse_float_js_plain_integer() {
        assert_eq!(parse_float_js("20"), 20.0);
    }

    #[test]
    fn parse_float_js_trailing_chars() {
        // "20abc" → 20.0 (stops at 'a'). Matches JS parseFloat("20abc") === 20.
        assert_eq!(parse_float_js("20abc"), 20.0);
    }

    #[test]
    fn parse_float_js_leading_spaces() {
        // "   20" → 20.0 (leading ASCII whitespace stripped).
        assert_eq!(parse_float_js("   20"), 20.0);
    }

    #[test]
    fn parse_float_js_leading_tab() {
        // "\t20" → 20.0.
        assert_eq!(parse_float_js("\t20"), 20.0);
    }

    #[test]
    fn parse_float_js_hex_prefix() {
        // "0x20" → 0.0 (parses "0", stops at 'x'). JS parseFloat is NOT hex-aware.
        assert_eq!(parse_float_js("0x20"), 0.0);
    }

    #[test]
    fn parse_float_js_suffix_px() {
        // "5px" → 5.0.
        assert_eq!(parse_float_js("5px"), 5.0);
    }

    #[test]
    fn parse_float_js_plus_sign() {
        assert_eq!(parse_float_js("+20"), 20.0);
    }

    #[test]
    fn parse_float_js_negative() {
        assert_eq!(parse_float_js("-3"), -3.0);
    }

    #[test]
    fn parse_float_js_exponent() {
        assert_eq!(parse_float_js("2e1"), 20.0);
    }

    #[test]
    fn parse_float_js_leading_dot() {
        assert_eq!(parse_float_js(".5"), 0.5);
    }

    #[test]
    fn parse_float_js_trailing_dot() {
        assert_eq!(parse_float_js("5."), 5.0);
    }

    #[test]
    fn parse_float_js_empty_is_nan() {
        assert!(parse_float_js("").is_nan());
    }

    #[test]
    fn parse_float_js_nan_word_is_nan() {
        assert!(parse_float_js("NaN").is_nan());
    }

    #[test]
    fn parse_float_js_infinity_is_infinite() {
        assert!(parse_float_js("Infinity").is_infinite());
        assert!(parse_float_js("Infinity").is_sign_positive());
    }

    #[test]
    fn parse_float_js_neg_infinity_is_infinite() {
        assert!(parse_float_js("-Infinity").is_infinite());
        assert!(parse_float_js("-Infinity").is_sign_negative());
    }

    #[test]
    fn parse_float_js_lone_dot_is_nan() {
        // "." alone has no digits after → NaN (JS parseFloat(".") === NaN).
        assert!(parse_float_js(".").is_nan());
    }

    #[test]
    fn parse_float_js_abc_is_nan() {
        assert!(parse_float_js("abc").is_nan());
    }

    // ── evaluate_atom — numeric comparisons ───────────────────────────────

    #[test]
    fn atom_numeric_gt_true() {
        let outputs = outputs_with("n", "90");
        let r = eval("$n.output > 80", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_gt_false() {
        let outputs = outputs_with("n", "70");
        let r = eval("$n.output > 80", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_gte_boundary() {
        let outputs = outputs_with("n", "80");
        assert!(eval("$n.output >= 80", &outputs).result);
        assert!(!eval("$n.output > 80", &outputs).result);
    }

    #[test]
    fn atom_numeric_lt_true() {
        let outputs = outputs_with("n", "5");
        assert!(eval("$n.output < 10", &outputs).result);
    }

    #[test]
    fn atom_numeric_lte_boundary() {
        let outputs = outputs_with("n", "10");
        assert!(eval("$n.output <= 10", &outputs).result);
        assert!(!eval("$n.output < 10", &outputs).result);
    }

    #[test]
    fn atom_numeric_non_parseable_actual_returns_unparsed() {
        let outputs = outputs_with("n", "not-a-number");
        let r = eval("$n.output > 10", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    #[test]
    fn atom_numeric_negative_rhs() {
        let outputs = outputs_with("n", "-5");
        assert!(eval("$n.output > -10", &outputs).result);
        assert!(!eval("$n.output > 0", &outputs).result);
    }

    #[test]
    fn atom_numeric_decimal() {
        let outputs = outputs_with("n", "3.5");
        assert!(eval("$n.output > 3.0", &outputs).result);
        assert!(!eval("$n.output > 3.5", &outputs).result);
    }

    // ── FIX-1: JS parseFloat-semantics oracle cases (previously diverged) ─

    #[test]
    fn atom_numeric_trailing_chars_parsed_true() {
        // wf12-numeric-actual-trailing-chars: "20abc" > 10 → result=true, parsed=true.
        // JS parseFloat("20abc") = 20; Rust previously rejected with str::parse → FAIL.
        let outputs = outputs_with("n", "20abc");
        let r = eval("$n.output > 10", &outputs);
        assert!(r.result, "20abc parses as 20 via JS parseFloat semantics");
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_leading_whitespace_parsed_true() {
        // wf12-numeric-actual-leading-ws: "   20" > 10 → result=true, parsed=true.
        let outputs = outputs_with("n", "   20");
        let r = eval("$n.output > 10", &outputs);
        assert!(r.result, "leading spaces stripped, 20 > 10 = true");
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_tab_prefix_parsed_true() {
        // wf12-actual-whitespace-tab: "\t20" > 10 → result=true, parsed=true.
        let outputs = outputs_with("n", "\t20");
        let r = eval("$n.output > 10", &outputs);
        assert!(r.result, "leading tab stripped, 20 > 10 = true");
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_hex_prefix_parsed_false_but_is_parsed() {
        // wf12-numeric-actual-hex: "0x20" > 10 → result=false (0 > 10 = false), parsed=true.
        // JS parseFloat("0x20") = 0 (stops at 'x'), NOT hex. So 0 > 10 = false, but parsed=true.
        let outputs = outputs_with("n", "0x20");
        let r = eval("$n.output > 10", &outputs);
        assert!(!r.result, "0x20 parses as 0 (not hex), 0 > 10 = false");
        assert!(r.parsed, "parsed=true (numeric prefix 0 found)");
    }

    #[test]
    fn atom_numeric_both_sides_prefix_parsed_true() {
        // wf12-both-sides-prefix: "5px" >= 5 → result=true, parsed=true.
        let outputs = outputs_with("n", "5px");
        let r = eval("$n.output >= 5", &outputs);
        assert!(r.result, "5px parses as 5, 5 >= 5 = true");
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_expected_quoted_garbage_parsed_true() {
        // wf12-expected-quoted-garbage: "90" > '20abc' → result=true, parsed=true.
        // The expected value '20abc' is the quoted RHS; JS parseFloat("20abc") = 20.
        // 90 > 20 = true.
        let outputs = outputs_with("n", "90");
        let r = eval("$n.output > '20abc'", &outputs);
        assert!(r.result, "90 > 20abc (parseFloat=20) = true");
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_infinity_not_parsed() {
        // wf12-numeric-actual-inf: "Infinity" > 10 → result=false, parsed=false.
        // JS parseFloat("Infinity") = Infinity, which is non-finite. The is_finite() guard
        // returns unparsed(), matching the oracle.
        let outputs = outputs_with("n", "Infinity");
        let r = eval("$n.output > 10", &outputs);
        assert!(!r.result);
        assert!(!r.parsed, "Infinity is non-finite → not parsed");
    }

    #[test]
    fn atom_numeric_plus_prefix_parsed() {
        // wf12-numeric-actual-plus: "+20" > 10 → result=true, parsed=true.
        let outputs = outputs_with("n", "+20");
        let r = eval("$n.output > 10", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_exponent_parsed() {
        // wf12-numeric-actual-exp: "2e1" > 10 → result=true (2e1=20 > 10), parsed=true.
        let outputs = outputs_with("n", "2e1");
        let r = eval("$n.output > 10", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_leading_dot() {
        // wf12-numeric-actual-dot5: ".5" > 0 → result=true, parsed=true.
        let outputs = outputs_with("n", ".5");
        let r = eval("$n.output > 0", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_trailing_dot() {
        // wf12-numeric-actual-trailingdot: "5." > 4 → result=true, parsed=true.
        let outputs = outputs_with("n", "5.");
        let r = eval("$n.output > 4", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_numeric_nan_word_not_parsed() {
        // wf12-numeric-actual-nan-word: "NaN" > 10 → result=false, parsed=false.
        let outputs = outputs_with("n", "NaN");
        let r = eval("$n.output > 10", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    #[test]
    fn atom_numeric_empty_actual_not_parsed() {
        // wf12-numeric-actual-empty: "" > 10 → result=false, parsed=false.
        let outputs = outputs_with("n", "");
        let r = eval("$n.output > 10", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    // ── evaluate_atom — dot notation field access ─────────────────────────

    #[test]
    fn atom_dot_field_from_structured_output() {
        let outputs = outputs_with_structured(
            "classify",
            "",
            json!({"type": "BUG"}),
        );
        let r = eval("$classify.output.type == 'BUG'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_shorthand_field() {
        // $node.field == 'VALUE' is equivalent to $node.output.field == 'VALUE'
        let outputs = outputs_with_structured(
            "classify",
            "",
            json!({"type": "FEATURE"}),
        );
        let r = eval("$classify.type == 'FEATURE'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn atom_shorthand_with_sub_field_is_parse_fail() {
        // $node.field.sub is not supported (only $node.output.field.sub would be, but
        // the source also doesn't support it — segment1 must be 'output' for two segments).
        // Actually: $node.output.field.sub is also rejected (only two segments allowed).
        // Here we test the shorthand case: $classify.field.sub → parse fail.
        let outputs = outputs_with("classify", "x");
        let r = eval("$classify.field.sub == 'x'", &outputs);
        // segment1="field", segment2="sub" but segment1 != "output" → parse fail
        assert!(!r.result);
        assert!(!r.parsed);
    }

    // ── evaluate_atom — unknown node ──────────────────────────────────────

    #[test]
    fn atom_unknown_node_returns_empty_string() {
        // Unknown node → '' (warn). Comparison with 'X' → false.
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("$unknown.output == 'X'", &outputs);
        assert!(!r.result);
        assert!(r.parsed); // parsed successfully; just the value was ''
    }

    #[test]
    fn atom_unknown_node_equality_empty() {
        // Unknown node → '', so == '' should be true.
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("$unknown.output == ''", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    // ── evaluate_atom — parse failures ────────────────────────────────────

    #[test]
    fn atom_invalid_expression_parse_fail() {
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("not a condition", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    #[test]
    fn atom_missing_dollar_parse_fail() {
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("node1.output == 'X'", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    #[test]
    fn atom_missing_operator_parse_fail() {
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("$node1.output 'X'", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    #[test]
    fn atom_empty_string_parse_fail() {
        let outputs: HashMap<String, NodeOutput> = HashMap::new();
        let r = eval("", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    // ── compound AND ──────────────────────────────────────────────────────

    #[test]
    fn compound_and_both_true() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "X".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        outputs.insert(
            "b".to_string(),
            NodeOutput::Completed {
                output: "Y".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        let r = eval("$a.output == 'X' && $b.output == 'Y'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn compound_and_first_false_short_circuits() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "NOPE".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        // "b" is not in outputs — if AND short-circuits correctly, it should never
        // try to evaluate the second atom (which would resolve to '' anyway).
        let r = eval("$a.output == 'X' && $b.output == 'Y'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    #[test]
    fn compound_and_second_false() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "X".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        outputs.insert(
            "b".to_string(),
            NodeOutput::Completed {
                output: "WRONG".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        let r = eval("$a.output == 'X' && $b.output == 'Y'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    // ── compound OR ───────────────────────────────────────────────────────

    #[test]
    fn compound_or_first_true_short_circuits() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "X".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        // "b" not in outputs — if OR short-circuits, it's never evaluated.
        let r = eval("$a.output == 'X' || $b.output == 'Y'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn compound_or_first_false_second_true() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "NOPE".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        outputs.insert(
            "b".to_string(),
            NodeOutput::Completed {
                output: "Y".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        let r = eval("$a.output == 'X' || $b.output == 'Y'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    #[test]
    fn compound_or_both_false() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "a".to_string(),
            NodeOutput::Completed {
                output: "NOPE".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        outputs.insert(
            "b".to_string(),
            NodeOutput::Completed {
                output: "NOPE".to_string(),
                session_id: None, structured_output: None, declared_fields: None,
            },
        );
        let r = eval("$a.output == 'X' || $b.output == 'Y'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    // ── AND higher precedence than OR ─────────────────────────────────────
    //
    // "$a == 'X' || $b == 'Y' && $c == 'Z'" is parsed as:
    //   "$a == 'X'" || ("$b == 'Y'" && "$c == 'Z'")
    // Not as: ("$a == 'X'" || "$b == 'Y'") && "$c == 'Z'"

    #[test]
    fn and_higher_precedence_than_or() {
        let mut outputs = HashMap::new();
        // a=NOPE, b=Y, c=Z → false || (true && true) → true
        outputs.insert("a".to_string(), NodeOutput::Completed { output: "NOPE".to_string(), session_id: None, structured_output: None, declared_fields: None });
        outputs.insert("b".to_string(), NodeOutput::Completed { output: "Y".to_string(), session_id: None, structured_output: None, declared_fields: None });
        outputs.insert("c".to_string(), NodeOutput::Completed { output: "Z".to_string(), session_id: None, structured_output: None, declared_fields: None });
        let r = eval("$a.output == 'X' || $b.output == 'Y' && $c.output == 'Z'", &outputs);
        assert!(r.result, "AND higher precedence than OR");
        assert!(r.parsed);
    }

    #[test]
    fn and_higher_precedence_and_clause_false() {
        let mut outputs = HashMap::new();
        // a=NOPE, b=Y, c=NOPE → false || (true && false) → false
        outputs.insert("a".to_string(), NodeOutput::Completed { output: "NOPE".to_string(), session_id: None, structured_output: None, declared_fields: None });
        outputs.insert("b".to_string(), NodeOutput::Completed { output: "Y".to_string(), session_id: None, structured_output: None, declared_fields: None });
        outputs.insert("c".to_string(), NodeOutput::Completed { output: "NOPE".to_string(), session_id: None, structured_output: None, declared_fields: None });
        let r = eval("$a.output == 'X' || $b.output == 'Y' && $c.output == 'Z'", &outputs);
        assert!(!r.result);
        assert!(r.parsed);
    }

    // ── Quoted strings with && inside ──────────────────────────────────────

    #[test]
    fn quoted_string_with_and_inside_not_split() {
        let outputs = outputs_with("n", "hello && world");
        // The value contains "&&" but it's part of the actual output — should NOT be split.
        // The RHS is quoted, so we need to match the WHOLE string.
        let r = eval("$n.output == 'hello && world'", &outputs);
        assert!(r.result, "quoted && in value must match verbatim");
        assert!(r.parsed);
    }

    #[test]
    fn quoted_string_with_or_inside_not_split() {
        let outputs = outputs_with("n", "yes || no");
        let r = eval("$n.output == 'yes || no'", &outputs);
        assert!(r.result, "quoted || in value must match verbatim");
        assert!(r.parsed);
    }

    // ── Parse fail in compound → entire expression fails ─────────────────

    #[test]
    fn compound_with_parse_fail_returns_unparsed() {
        let outputs = outputs_with("a", "X");
        // Second atom is syntactically invalid → whole expression fails.
        let r = eval("$a.output == 'X' && not-valid", &outputs);
        assert!(!r.result);
        assert!(!r.parsed);
    }

    // ── OutputRefError propagation (unresolvable ref → node FAILS) ────────

    #[test]
    fn unresolvable_ref_propagates_as_error() {
        // Schemaless node output is not JSON → resolving .field THROWS.
        let outputs = outputs_with("n", "plain text");
        let result = evaluate_condition("$n.output.field == 'X'", &outputs);
        assert!(result.is_err(), "OutputRefError must propagate (not be swallowed)");
    }

    #[test]
    fn schemaless_missing_key_propagates_as_error() {
        let outputs = outputs_with("n", r#"{"other":"val"}"#);
        let result = evaluate_condition("$n.output.missing_field == 'X'", &outputs);
        assert!(result.is_err(), "missing key in schemaless JSON must propagate as error");
    }

    #[test]
    fn declared_schema_field_not_in_schema_propagates() {
        let outputs = outputs_with_declared(
            "n",
            "",
            json!({"foo":"val"}),
            vec!["foo".to_string()],
        );
        let result = evaluate_condition("$n.output.bad_field == 'val'", &outputs);
        assert!(result.is_err(), "not-in-schema ref must propagate as error");
    }

    #[test]
    fn skipped_producer_ref_propagates_as_error() {
        let mut outputs = HashMap::new();
        outputs.insert("n".to_string(), NodeOutput::Skipped { output: String::new() });
        let result = evaluate_condition("$n.output.field == 'X'", &outputs);
        assert!(result.is_err(), "producer-not-run must propagate as error");
    }

    // ── Special: skipped producer bare output ref → '' (no error) ────────

    #[test]
    fn skipped_producer_bare_output_ref_returns_empty() {
        // Bare `$node.output` (no field) on a skipped node → '' (no throw).
        // resolve_output_ref only calls resolve_node_output_field when field is Some.
        let mut outputs = HashMap::new();
        outputs.insert("n".to_string(), NodeOutput::Skipped { output: String::new() });
        let r = eval("$n.output == ''", &outputs);
        assert!(r.result, "bare output ref on skipped node returns '' which equals ''");
        assert!(r.parsed);
    }

    // ── Node IDs with hyphens ─────────────────────────────────────────────

    #[test]
    fn node_id_with_hyphens_accepted() {
        let outputs = outputs_with("my-node-1", "OK");
        let r = eval("$my-node-1.output == 'OK'", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    // ── Whitespace handling ───────────────────────────────────────────────

    #[test]
    fn whitespace_around_expr_ignored() {
        let outputs = outputs_with("n", "X");
        let r = eval("  $n.output == 'X'  ", &outputs);
        assert!(r.result);
        assert!(r.parsed);
    }

    // ── evaluate_condition — truth table ──────────────────────────────────

    #[test]
    fn truth_table_and_or() {
        // Tests all combinations of two boolean outputs through an AND then OR expression.
        let mk = |a: &str, b: &str| -> HashMap<String, NodeOutput> {
            let mut m = HashMap::new();
            m.insert("a".to_string(), NodeOutput::Completed {
                output: a.to_string(), session_id: None, structured_output: None, declared_fields: None,
            });
            m.insert("b".to_string(), NodeOutput::Completed {
                output: b.to_string(), session_id: None, structured_output: None, declared_fields: None,
            });
            m
        };

        // AND truth table
        assert!(eval("$a.output == 'T' && $b.output == 'T'", &mk("T","T")).result);
        assert!(!eval("$a.output == 'T' && $b.output == 'T'", &mk("T","F")).result);
        assert!(!eval("$a.output == 'T' && $b.output == 'T'", &mk("F","T")).result);
        assert!(!eval("$a.output == 'T' && $b.output == 'T'", &mk("F","F")).result);

        // OR truth table
        assert!(eval("$a.output == 'T' || $b.output == 'T'", &mk("T","T")).result);
        assert!(eval("$a.output == 'T' || $b.output == 'T'", &mk("T","F")).result);
        assert!(eval("$a.output == 'T' || $b.output == 'T'", &mk("F","T")).result);
        assert!(!eval("$a.output == 'T' || $b.output == 'T'", &mk("F","F")).result);
    }
}
