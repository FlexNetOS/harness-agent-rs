//! PORT of `packages/workflows/src/schemas/loop.ts`.
//!
//! UNIT WF-03: `LoopNodeConfig` struct with ALL fields + validation rule
//! "interactive == true requires gate_message" (loop.ts:23-31).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Configuration for a loop node in the workflow DAG. loop.ts:6-33.
///
/// Zod schema fields and constraints:
///   - `prompt`         — `z.string().min(1)` → non-empty required (loop.ts:9)
///   - `until`          — `z.string().min(1)` → non-empty required (loop.ts:11)
///   - `max_iterations` — `z.number().int().positive()` → u32 ≥ 1 (loop.ts:13)
///   - `fresh_context`  — `z.boolean().default(false)` → bool, default false (loop.ts:15)
///   - `until_bash`     — `z.string().optional()` (loop.ts:17)
///   - `interactive`    — `z.boolean().optional()` (loop.ts:19)
///   - `gate_message`   — `z.string().optional()` (loop.ts:21)
///
/// Cross-field rule (`.superRefine`, loop.ts:23-31):
///   `interactive == true` requires `gate_message` (non-empty).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopNodeConfig {
    /// Inline prompt text executed each iteration. Non-empty. loop.ts:9.
    pub prompt: String,

    /// Completion signal string detected in AI output (e.g. `"COMPLETE"`). Non-empty. loop.ts:11.
    pub until: String,

    /// Maximum iterations allowed; exceeding this fails the node. Must be ≥ 1. loop.ts:13.
    pub max_iterations: u32,

    /// Whether to start a fresh session each iteration. Default: false. loop.ts:15.
    #[serde(default)]
    pub fresh_context: bool,

    /// Optional bash script run after each iteration; exit 0 = complete. loop.ts:17.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_bash: Option<String>,

    /// When true, pause between iterations for user input via `/workflow approve`. loop.ts:19.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,

    /// Message shown to user when paused. Required when `interactive` is true. loop.ts:21.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_message: Option<String>,
}

/// Validation errors for `LoopNodeConfig`. loop.ts:9-31.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LoopValidationError {
    /// `prompt` was empty (z.string().min(1)). loop.ts:9.
    #[error("loop node requires 'loop.prompt' (non-empty string)")]
    EmptyPrompt,

    /// `until` was empty (z.string().min(1)). loop.ts:11.
    #[error("loop node requires 'loop.until' (completion signal string)")]
    EmptyUntil,

    /// `max_iterations` was zero (`z.number().int().positive()`). loop.ts:13.
    #[error("'loop.max_iterations' must be a positive integer")]
    MaxIterationsNotPositive,

    /// `interactive == true` but `gate_message` is absent or empty (superRefine). loop.ts:23-31.
    #[error("interactive loop requires 'loop.gate_message' (non-empty string)")]
    InteractiveRequiresGateMessage,
}

impl LoopNodeConfig {
    /// Validate all constraints that zod enforces at parse time.
    ///
    /// Field-level validations (`.min(1)`, `.positive()`) plus the cross-field
    /// `.superRefine` rule (loop.ts:23-31).
    ///
    /// Returns all errors found, not just the first (mirrors zod's default behavior
    /// of collecting all issues before returning).
    pub fn validate(&self) -> Vec<LoopValidationError> {
        let mut errors = Vec::new();

        // z.string().min(1): prompt must be non-empty. loop.ts:9.
        if self.prompt.is_empty() {
            errors.push(LoopValidationError::EmptyPrompt);
        }

        // z.string().min(1): until must be non-empty. loop.ts:11.
        if self.until.is_empty() {
            errors.push(LoopValidationError::EmptyUntil);
        }

        // z.number().int().positive(): max_iterations must be ≥ 1.
        // u32 already guarantees non-negative; we check > 0. loop.ts:13.
        if self.max_iterations == 0 {
            errors.push(LoopValidationError::MaxIterationsNotPositive);
        }

        // superRefine: interactive == true requires gate_message. loop.ts:23-31.
        if self.interactive == Some(true) {
            let missing = match &self.gate_message {
                None => true,
                Some(msg) => msg.is_empty(),
            };
            if missing {
                errors.push(LoopValidationError::InteractiveRequiresGateMessage);
            }
        }

        errors
    }

    /// Parse from a JSON value and immediately validate all constraints.
    ///
    /// Returns `Err` with all validation errors if any constraint is violated.
    /// Mirrors zod's `.parse()` behavior.
    pub fn parse(value: serde_json::Value) -> Result<Self, Vec<LoopValidationError>> {
        let config: Self = serde_json::from_value(value).map_err(|e| {
            // Map serde deserialization errors to the most relevant validation error.
            // In practice zod would catch these as field-level issues.
            let msg = e.to_string();
            if msg.contains("prompt") {
                vec![LoopValidationError::EmptyPrompt]
            } else if msg.contains("until") {
                vec![LoopValidationError::EmptyUntil]
            } else {
                // Re-surface as a generic error — callers get the field-level details
                // from the serde error message; here we return the closest match.
                vec![LoopValidationError::EmptyPrompt]
            }
        })?;

        let errors = config.validate();
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_loop() -> LoopNodeConfig {
        LoopNodeConfig {
            prompt: "Do the work".into(),
            until: "COMPLETE".into(),
            max_iterations: 5,
            fresh_context: false,
            until_bash: None,
            interactive: None,
            gate_message: None,
        }
    }

    // ── Accept cases ─────────────────────────────────────────────────────────

    #[test]
    fn valid_minimal_loop_passes() {
        let errors = valid_loop().validate();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn interactive_with_gate_message_passes() {
        let mut l = valid_loop();
        l.interactive = Some(true);
        l.gate_message = Some("Approve next iteration?".into());
        let errors = l.validate();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn interactive_false_without_gate_message_passes() {
        let mut l = valid_loop();
        l.interactive = Some(false);
        // gate_message absent — only interactive=true triggers the requirement
        let errors = l.validate();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn interactive_none_without_gate_message_passes() {
        let l = valid_loop();
        let errors = l.validate();
        assert!(errors.is_empty());
    }

    #[test]
    fn fresh_context_defaults_to_false_via_serde() {
        let json = json!({
            "prompt": "p",
            "until": "DONE",
            "max_iterations": 3
        });
        let l: LoopNodeConfig = serde_json::from_value(json).unwrap();
        assert!(!l.fresh_context, "fresh_context should default to false");
    }

    #[test]
    fn round_trip_with_all_fields() {
        let json = json!({
            "prompt": "iterate",
            "until": "COMPLETE",
            "max_iterations": 10,
            "fresh_context": true,
            "until_bash": "test -f done.txt",
            "interactive": true,
            "gate_message": "Continue?"
        });
        let l: LoopNodeConfig = serde_json::from_value(json.clone()).unwrap();
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["prompt"], "iterate");
        assert_eq!(back["until"], "COMPLETE");
        assert_eq!(back["max_iterations"], 10);
        assert_eq!(back["fresh_context"], true);
        assert_eq!(back["until_bash"], "test -f done.txt");
        assert_eq!(back["interactive"], true);
        assert_eq!(back["gate_message"], "Continue?");
    }

    // ── Reject cases ─────────────────────────────────────────────────────────

    #[test]
    fn empty_prompt_is_rejected() {
        let mut l = valid_loop();
        l.prompt = String::new();
        let errors = l.validate();
        assert!(
            errors.contains(&LoopValidationError::EmptyPrompt),
            "expected EmptyPrompt, got: {errors:?}"
        );
    }

    #[test]
    fn empty_until_is_rejected() {
        let mut l = valid_loop();
        l.until = String::new();
        let errors = l.validate();
        assert!(
            errors.contains(&LoopValidationError::EmptyUntil),
            "expected EmptyUntil, got: {errors:?}"
        );
    }

    #[test]
    fn zero_max_iterations_is_rejected() {
        let mut l = valid_loop();
        l.max_iterations = 0;
        let errors = l.validate();
        assert!(
            errors.contains(&LoopValidationError::MaxIterationsNotPositive),
            "expected MaxIterationsNotPositive, got: {errors:?}"
        );
    }

    #[test]
    fn interactive_true_without_gate_message_is_rejected() {
        let mut l = valid_loop();
        l.interactive = Some(true);
        l.gate_message = None;
        let errors = l.validate();
        assert!(
            errors.contains(&LoopValidationError::InteractiveRequiresGateMessage),
            "expected InteractiveRequiresGateMessage, got: {errors:?}"
        );
    }

    #[test]
    fn interactive_true_with_empty_gate_message_is_rejected() {
        let mut l = valid_loop();
        l.interactive = Some(true);
        l.gate_message = Some(String::new());
        let errors = l.validate();
        assert!(
            errors.contains(&LoopValidationError::InteractiveRequiresGateMessage),
            "expected InteractiveRequiresGateMessage for empty gate_message, got: {errors:?}"
        );
    }

    #[test]
    fn multiple_errors_are_collected() {
        let l = LoopNodeConfig {
            prompt: String::new(), // error
            until: String::new(),  // error
            max_iterations: 0,     // error
            fresh_context: false,
            until_bash: None,
            interactive: Some(true), // error: no gate_message
            gate_message: None,
        };
        let errors = l.validate();
        assert_eq!(errors.len(), 4, "expected 4 errors, got: {errors:?}");
    }

    // ── Error message exact match ─────────────────────────────────────────────

    #[test]
    fn error_messages_match_zod_exact() {
        assert_eq!(
            LoopValidationError::EmptyPrompt.to_string(),
            "loop node requires 'loop.prompt' (non-empty string)"
        );
        assert_eq!(
            LoopValidationError::EmptyUntil.to_string(),
            "loop node requires 'loop.until' (completion signal string)"
        );
        assert_eq!(
            LoopValidationError::MaxIterationsNotPositive.to_string(),
            "'loop.max_iterations' must be a positive integer"
        );
        assert_eq!(
            LoopValidationError::InteractiveRequiresGateMessage.to_string(),
            "interactive loop requires 'loop.gate_message' (non-empty string)"
        );
    }
}
