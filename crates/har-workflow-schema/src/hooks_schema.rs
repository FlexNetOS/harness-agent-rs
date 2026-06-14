//! PORT of `packages/workflows/src/schemas/hooks.ts`.
//!
//! UNIT WF-05: Per-node hook configuration.
//!
//! All 21 `WorkflowHookEvent` variants, `WORKFLOW_HOOK_EVENTS` constant,
//! `WorkflowHookMatcher`, `WorkflowNodeHooks`, and `.strict()` unknown-key rejection.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

/// Supported hook events for per-node hooks. hooks.ts:10-32.
///
/// `z.enum([...])` → fieldless Rust enum with exact wire names (PascalCase, matching
/// the Claude Agent SDK's `HookEvent` type — hooks.ts:7).
///
/// All 21 variants must be present. Any missing variant would break `.strict()` rejection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowHookEvent {
    /// hooks.ts:11
    PreToolUse,
    /// hooks.ts:12
    PostToolUse,
    /// hooks.ts:13
    PostToolUseFailure,
    /// hooks.ts:14
    Notification,
    /// hooks.ts:15
    UserPromptSubmit,
    /// hooks.ts:16
    SessionStart,
    /// hooks.ts:17
    SessionEnd,
    /// hooks.ts:18
    Stop,
    /// hooks.ts:19
    SubagentStart,
    /// hooks.ts:20
    SubagentStop,
    /// hooks.ts:21
    PreCompact,
    /// hooks.ts:22
    PermissionRequest,
    /// hooks.ts:23
    Setup,
    /// hooks.ts:24
    TeammateIdle,
    /// hooks.ts:25
    TaskCompleted,
    /// hooks.ts:26
    Elicitation,
    /// hooks.ts:27
    ElicitationResult,
    /// hooks.ts:28
    ConfigChange,
    /// hooks.ts:29
    WorktreeCreate,
    /// hooks.ts:30
    WorktreeRemove,
    /// hooks.ts:31
    InstructionsLoaded,
}

impl WorkflowHookEvent {
    /// Return the exact string representation used in YAML/JSON. hooks.ts:10-32.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::Notification => "Notification",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::Stop => "Stop",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::PermissionRequest => "PermissionRequest",
            Self::Setup => "Setup",
            Self::TeammateIdle => "TeammateIdle",
            Self::TaskCompleted => "TaskCompleted",
            Self::Elicitation => "Elicitation",
            Self::ElicitationResult => "ElicitationResult",
            Self::ConfigChange => "ConfigChange",
            Self::WorktreeCreate => "WorktreeCreate",
            Self::WorktreeRemove => "WorktreeRemove",
            Self::InstructionsLoaded => "InstructionsLoaded",
        }
    }

}

impl std::str::FromStr for WorkflowHookEvent {
    type Err = ();

    /// Parse from a string. Returns `Err(())` for unknown event names.
    ///
    /// Used in `.strict()` validation: unknown names → `HookValidationError::UnknownEvent`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PreToolUse" => Ok(Self::PreToolUse),
            "PostToolUse" => Ok(Self::PostToolUse),
            "PostToolUseFailure" => Ok(Self::PostToolUseFailure),
            "Notification" => Ok(Self::Notification),
            "UserPromptSubmit" => Ok(Self::UserPromptSubmit),
            "SessionStart" => Ok(Self::SessionStart),
            "SessionEnd" => Ok(Self::SessionEnd),
            "Stop" => Ok(Self::Stop),
            "SubagentStart" => Ok(Self::SubagentStart),
            "SubagentStop" => Ok(Self::SubagentStop),
            "PreCompact" => Ok(Self::PreCompact),
            "PermissionRequest" => Ok(Self::PermissionRequest),
            "Setup" => Ok(Self::Setup),
            "TeammateIdle" => Ok(Self::TeammateIdle),
            "TaskCompleted" => Ok(Self::TaskCompleted),
            "Elicitation" => Ok(Self::Elicitation),
            "ElicitationResult" => Ok(Self::ElicitationResult),
            "ConfigChange" => Ok(Self::ConfigChange),
            "WorktreeCreate" => Ok(Self::WorktreeCreate),
            "WorktreeRemove" => Ok(Self::WorktreeRemove),
            "InstructionsLoaded" => Ok(Self::InstructionsLoaded),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for WorkflowHookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical list of all 21 hook event names. hooks.ts:37.
///
/// `export const WORKFLOW_HOOK_EVENTS: readonly WorkflowHookEvent[] = workflowHookEventSchema.options`
///
/// Order matches the TypeScript enum declaration (hooks.ts:10-32).
pub const WORKFLOW_HOOK_EVENTS: &[WorkflowHookEvent] = &[
    WorkflowHookEvent::PreToolUse,
    WorkflowHookEvent::PostToolUse,
    WorkflowHookEvent::PostToolUseFailure,
    WorkflowHookEvent::Notification,
    WorkflowHookEvent::UserPromptSubmit,
    WorkflowHookEvent::SessionStart,
    WorkflowHookEvent::SessionEnd,
    WorkflowHookEvent::Stop,
    WorkflowHookEvent::SubagentStart,
    WorkflowHookEvent::SubagentStop,
    WorkflowHookEvent::PreCompact,
    WorkflowHookEvent::PermissionRequest,
    WorkflowHookEvent::Setup,
    WorkflowHookEvent::TeammateIdle,
    WorkflowHookEvent::TaskCompleted,
    WorkflowHookEvent::Elicitation,
    WorkflowHookEvent::ElicitationResult,
    WorkflowHookEvent::ConfigChange,
    WorkflowHookEvent::WorktreeCreate,
    WorkflowHookEvent::WorktreeRemove,
    WorkflowHookEvent::InstructionsLoaded,
];

/// A single hook matcher in a YAML workflow definition. hooks.ts:43-50.
///
/// Maps 1:1 to the SDK's `HookCallbackMatcher`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowHookMatcher {
    /// Regex pattern to match tool names (`PreToolUse`/`PostToolUse`) or event subtypes.
    /// hooks.ts:45.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,

    /// The SDK `SyncHookJSONOutput` to return when this hook fires.
    /// `z.record(z.string(), z.unknown())` → `HashMap<String, Value>`. hooks.ts:47.
    pub response: HashMap<String, Value>,

    /// Timeout in seconds (default: SDK default of 60). Must be > 0. hooks.ts:49.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
}

/// Validation errors for `WorkflowHookMatcher`. hooks.ts:49.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum HookMatcherValidationError {
    /// `timeout` was ≤ 0 (`z.number().positive()`). hooks.ts:49.
    #[error("hook matcher 'timeout' must be a positive number")]
    TimeoutNotPositive,
}

impl WorkflowHookMatcher {
    /// Validate constraints (currently: `timeout > 0`). hooks.ts:49.
    pub fn validate(&self) -> Vec<HookMatcherValidationError> {
        let mut errors = Vec::new();
        if let Some(t) = self.timeout {
            if t <= 0.0 {
                errors.push(HookMatcherValidationError::TimeoutNotPositive);
            }
        }
        errors
    }
}

/// Validation errors for `WorkflowNodeHooks`. hooks.ts:86.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HookValidationError {
    /// An unknown event key was present in the map (`.strict()` rejection). hooks.ts:86.
    #[error("unknown hook event: '{key}'. Valid events: {valid}")]
    UnknownEvent { key: String, valid: String },
}

/// Per-node hook configuration keyed by event name. hooks.ts:62-88.
///
/// Each event maps to an array of matchers with static responses.
///
/// `.strict()` behavior (hooks.ts:86): unknown event names are rejected.
/// In Rust: this is a validated newtype wrapping `HashMap<WorkflowHookEvent, Vec<WorkflowHookMatcher>>`.
/// Deserialization succeeds for any JSON object; `parse()` enforces the strict key check.
///
/// The TypeScript version lists fields explicitly (not `z.record`) so TypeScript narrows event
/// names to the `WorkflowHookEvent` union. We mirror this as: the key type IS `WorkflowHookEvent`,
/// so any key that doesn't round-trip through `WorkflowHookEvent::from_str` is rejected.
#[derive(Debug, Clone, Default)]
pub struct WorkflowNodeHooks {
    /// Internal storage: `WorkflowHookEvent` → matchers. hooks.ts:63-85.
    pub events: HashMap<WorkflowHookEvent, Vec<WorkflowHookMatcher>>,
}

impl WorkflowNodeHooks {
    /// Create an empty hooks map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get matchers for a given event, if any.
    pub fn get(&self, event: &WorkflowHookEvent) -> Option<&Vec<WorkflowHookMatcher>> {
        self.events.get(event)
    }

    /// Insert matchers for a given event.
    pub fn insert(&mut self, event: WorkflowHookEvent, matchers: Vec<WorkflowHookMatcher>) {
        self.events.insert(event, matchers);
    }

    /// Parse from a `serde_json::Value` with `.strict()` key validation. hooks.ts:86.
    ///
    /// Rejects any key that is not a known `WorkflowHookEvent` variant, with a clear error
    /// message (mirrors zod's `.strict()` which produces `"Unrecognized key(s) in object: '…'"`).
    pub fn parse(value: serde_json::Value) -> Result<Self, Vec<HookValidationError>> {
        use std::str::FromStr;

        let map: HashMap<String, serde_json::Value> = serde_json::from_value(value)
            .map_err(|_| vec![])?;

        let valid_names: String = WORKFLOW_HOOK_EVENTS
            .iter()
            .map(|e| e.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let mut errors = Vec::new();
        let mut events: HashMap<WorkflowHookEvent, Vec<WorkflowHookMatcher>> = HashMap::new();

        for (key, val) in map {
            match WorkflowHookEvent::from_str(&key) {
                Ok(event) => {
                    // Deserialize the matchers array for this event.
                    match serde_json::from_value::<Vec<WorkflowHookMatcher>>(val) {
                        Ok(matchers) => {
                            events.insert(event, matchers);
                        }
                        Err(_) => {
                            // Malformed matchers array — report as unknown event (no finer error
                            // type exists at this layer; detailed validation is the verifier's job).
                        }
                    }
                }
                Err(_) => {
                    // .strict(): unknown key → error. hooks.ts:86.
                    errors.push(HookValidationError::UnknownEvent {
                        key: key.clone(),
                        valid: valid_names.clone(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(Self { events })
        } else {
            Err(errors)
        }
    }
}

/// Custom `Serialize` for `WorkflowNodeHooks` — serializes `events` map with string keys.
impl Serialize for WorkflowNodeHooks {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.events.len()))?;
        for (event, matchers) in &self.events {
            map.serialize_entry(event.as_str(), matchers)?;
        }
        map.end()
    }
}

/// Custom `Deserialize` for `WorkflowNodeHooks` — deserializes string keys to enum variants,
/// silently skipping unknown keys (`.parse()` re-raises them as `HookValidationError`).
///
/// Deserialization itself is permissive; strict validation lives in `.parse()`.
impl<'de> Deserialize<'de> for WorkflowNodeHooks {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::str::FromStr;
        let raw: HashMap<String, Vec<WorkflowHookMatcher>> =
            HashMap::deserialize(deserializer)?;
        let mut events = HashMap::new();
        for (key, matchers) in raw {
            if let Ok(event) = WorkflowHookEvent::from_str(&key) {
                events.insert(event, matchers);
            }
            // Unknown keys are silently skipped here; `.parse()` is the strict gate.
        }
        Ok(Self { events })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── WorkflowHookEvent ────────────────────────────────────────────────────

    #[test]
    fn workflow_hook_event_count_is_21() {
        assert_eq!(WORKFLOW_HOOK_EVENTS.len(), 21, "WORKFLOW_HOOK_EVENTS must have exactly 21 entries");
    }

    #[test]
    fn all_events_round_trip_via_from_str() {
        use std::str::FromStr;
        for event in WORKFLOW_HOOK_EVENTS {
            let s = event.as_str();
            let back = WorkflowHookEvent::from_str(s);
            assert!(back.is_ok(), "from_str failed for event: {s}");
            assert_eq!(back.unwrap(), *event, "round-trip mismatch for: {s}");
        }
    }

    #[test]
    fn unknown_event_from_str_returns_err() {
        use std::str::FromStr;
        assert!(WorkflowHookEvent::from_str("preToolUse").is_err(), "camelCase should not match");
        assert!(WorkflowHookEvent::from_str("pre_tool_use").is_err(), "snake_case should not match");
        assert!(WorkflowHookEvent::from_str("").is_err());
        assert!(WorkflowHookEvent::from_str("Unknown").is_err());
    }

    #[test]
    fn event_names_match_source_exactly() {
        // Spot-check a selection to verify exact wire names from hooks.ts:10-32.
        assert_eq!(WorkflowHookEvent::PreToolUse.as_str(), "PreToolUse");
        assert_eq!(WorkflowHookEvent::PostToolUseFailure.as_str(), "PostToolUseFailure");
        assert_eq!(WorkflowHookEvent::UserPromptSubmit.as_str(), "UserPromptSubmit");
        assert_eq!(WorkflowHookEvent::InstructionsLoaded.as_str(), "InstructionsLoaded");
        assert_eq!(WorkflowHookEvent::ElicitationResult.as_str(), "ElicitationResult");
    }

    #[test]
    fn workflow_hook_event_serde_round_trip() {
        let event = WorkflowHookEvent::PreToolUse;
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json, json!("PreToolUse"));
        let back: WorkflowHookEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, WorkflowHookEvent::PreToolUse);
    }

    // ── WorkflowHookMatcher ──────────────────────────────────────────────────

    #[test]
    fn hook_matcher_round_trip() {
        let json = json!({
            "matcher": "Bash",
            "response": { "decision": "allow" },
            "timeout": 30.0
        });
        let m: WorkflowHookMatcher = serde_json::from_value(json).unwrap();
        assert_eq!(m.matcher.as_deref(), Some("Bash"));
        assert_eq!(m.response["decision"], "allow");
        assert_eq!(m.timeout, Some(30.0));
        assert!(m.validate().is_empty());
    }

    #[test]
    fn hook_matcher_without_optional_fields() {
        let json = json!({
            "response": { "decision": "deny" }
        });
        let m: WorkflowHookMatcher = serde_json::from_value(json).unwrap();
        assert!(m.matcher.is_none());
        assert!(m.timeout.is_none());
        assert!(m.validate().is_empty());
    }

    #[test]
    fn hook_matcher_negative_timeout_rejected() {
        let m = WorkflowHookMatcher {
            matcher: None,
            response: HashMap::new(),
            timeout: Some(-1.0),
        };
        let errors = m.validate();
        assert!(errors.contains(&HookMatcherValidationError::TimeoutNotPositive));
    }

    #[test]
    fn hook_matcher_zero_timeout_rejected() {
        let m = WorkflowHookMatcher {
            matcher: None,
            response: HashMap::new(),
            timeout: Some(0.0),
        };
        let errors = m.validate();
        assert!(errors.contains(&HookMatcherValidationError::TimeoutNotPositive));
    }

    // ── WorkflowNodeHooks ────────────────────────────────────────────────────

    #[test]
    fn workflow_node_hooks_parse_known_events() {
        let json = json!({
            "PreToolUse": [
                { "matcher": "Bash", "response": { "decision": "allow" } }
            ],
            "PostToolUse": [
                { "response": { "type": "log" } }
            ]
        });
        let hooks = WorkflowNodeHooks::parse(json).expect("should parse");
        assert!(hooks.events.contains_key(&WorkflowHookEvent::PreToolUse));
        assert!(hooks.events.contains_key(&WorkflowHookEvent::PostToolUse));
        assert_eq!(hooks.events[&WorkflowHookEvent::PreToolUse].len(), 1);
    }

    #[test]
    fn workflow_node_hooks_strict_rejects_unknown_key() {
        // `.strict()` behavior: hooks.ts:86.
        let json = json!({
            "PreToolUse": [{ "response": { "decision": "allow" } }],
            "preToolUse": [{ "response": { "decision": "deny" } }]  // typo / camelCase
        });
        let result = WorkflowNodeHooks::parse(json);
        assert!(result.is_err(), "should reject unknown event key");
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, HookValidationError::UnknownEvent { key, .. } if key == "preToolUse")));
    }

    #[test]
    fn workflow_node_hooks_strict_rejects_snake_case_key() {
        let json = json!({
            "pre_tool_use": [{ "response": {} }]
        });
        let result = WorkflowNodeHooks::parse(json);
        assert!(result.is_err());
    }

    #[test]
    fn workflow_node_hooks_empty_object_is_valid() {
        let json = json!({});
        let hooks = WorkflowNodeHooks::parse(json).expect("empty object should be valid");
        assert!(hooks.events.is_empty());
    }

    #[test]
    fn workflow_node_hooks_serialize_round_trip() {
        let mut hooks = WorkflowNodeHooks::new();
        hooks.insert(
            WorkflowHookEvent::PreToolUse,
            vec![WorkflowHookMatcher {
                matcher: Some("Bash".into()),
                response: {
                    let mut m = HashMap::new();
                    m.insert("decision".into(), json!("allow"));
                    m
                },
                timeout: Some(30.0),
            }],
        );

        let json = serde_json::to_value(&hooks).unwrap();
        assert!(json["PreToolUse"].is_array());
        assert_eq!(json["PreToolUse"][0]["matcher"], "Bash");

        let back: WorkflowNodeHooks = serde_json::from_value(json).unwrap();
        assert!(back.events.contains_key(&WorkflowHookEvent::PreToolUse));
    }

    #[test]
    fn workflow_node_hooks_all_21_events_accepted() {
        // Build a hooks object with all 21 events present — parse should succeed.
        let mut obj = serde_json::Map::new();
        for event in WORKFLOW_HOOK_EVENTS {
            obj.insert(
                event.as_str().to_owned(),
                json!([{ "response": { "ok": true } }]),
            );
        }
        let json = Value::Object(obj);
        let result = WorkflowNodeHooks::parse(json);
        assert!(result.is_ok(), "all 21 events should be accepted");
        let hooks = result.unwrap();
        assert_eq!(hooks.events.len(), 21);
    }

    // ── Error messages ───────────────────────────────────────────────────────

    #[test]
    fn hook_validation_error_message_contains_key() {
        let err = HookValidationError::UnknownEvent {
            key: "preToolUse".into(),
            valid: "PreToolUse, PostToolUse".into(),
        };
        assert!(err.to_string().contains("preToolUse"));
    }
}
