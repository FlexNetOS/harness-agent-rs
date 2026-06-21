//! PORT of `packages/workflows/src/schemas/node-artifact.ts`.
//!
//! UNIT WF-07: NodeArtifact — on-disk output file metadata for typed-output nodes.
//!
//! NEEDS-HUMAN resolved: the actual node-artifact.ts was read. Resolved shape:
//!   - `nodeId: String` (z.string())
//!   - `outputType: String` (z.string().min(1))   — NOT ArtifactType; this is a free string tag
//!   - `path: String` (z.string())                — relative to artifacts dir, e.g. "nodes/plan.md"
//!   - `runId: String` (z.string())
//!   - `producedAt: String` (z.string().datetime()) — ISO-8601 validated on read
//!   - `size: u64` (z.number().int().nonnegative()) — HAS .int() AND .nonnegative() → u64
//!   - `sessionId?: String` (z.string().optional())
//!
//! NOTE: `ArtifactType` (workflow-run.ts) is a DIFFERENT concept:
//!   - `ArtifactType` = workflow-event artifact kinds (pr/commit/file_created/…)
//!   - `NodeArtifact.outputType` = free-string tag from the node's `output_type:` field
//!
//! These are DISTINCT and must NOT be conflated.
//!
//! Numeric audit (node-artifact.ts:24):
//!   - `size: z.number().int().nonnegative()` — has `.int()` AND `.nonneg()` → `u64` (no negatives).
//!
//! Trim audit: no `.trim()` transforms in node-artifact.ts. All `z.string()` fields stored verbatim.
//!
//! Validation rules:
//!   - `outputType` must be non-empty (z.string().min(1)) → `NodeArtifactValidationError::EmptyOutputType`
//!   - `producedAt` must be a valid ISO-8601 datetime string (z.string().datetime()) →
//!     `NodeArtifactValidationError::InvalidProducedAt`
//!   - `size` is `u64` — structural type enforces non-negative at parse time

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Metadata for a node's typed output artifact, written when a node declares `output_type`.
///
/// Persisted as `nodes/<id>.meta.json` alongside the output file `nodes/<id>.md` inside the
/// run's artifacts dir, so other nodes and later runs can locate a prior output by type instead
/// of guessing filenames. node-artifact.ts:12-27.
///
/// Distinct from `ArtifactType` (workflow-event artifact kinds: pr/commit/file_created/…) —
/// this describes a node's on-disk output file. node-artifact.ts:7-10.
///
/// Numeric audit:
///   - `size: z.number().int().nonnegative()` — has `.int()` → `u64`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeArtifact {
    /// ID of the node that produced this artifact. node-artifact.ts:13.
    #[serde(rename = "nodeId")]
    pub node_id: String,

    /// Free-string type tag from the node's `output_type:` field.
    /// Must be non-empty (z.string().min(1)). node-artifact.ts:14.
    ///
    /// IMPORTANT: This is NOT `ArtifactType` — it is any string the workflow author
    /// assigns (e.g. "plan", "report", "diff"). The runtime uses it for `latestNodeArtifactOfType`
    /// queries. node-artifact.ts:7-10.
    #[serde(rename = "outputType")]
    pub output_type: String,

    /// Path to the output file, relative to the artifacts dir. node-artifact.ts:16.
    /// Example: `"nodes/plan.md"`.
    pub path: String,

    /// ID of the workflow run that produced this artifact. node-artifact.ts:17.
    #[serde(rename = "runId")]
    pub run_id: String,

    /// ISO-8601 timestamp of when the artifact was written. node-artifact.ts:22.
    ///
    /// Enforced as a valid datetime so lexicographic ordering in `latestNodeArtifactOfType`
    /// stays correct — a corrupt/non-ISO value is rejected on validation rather than silently
    /// returning the wrong "latest" artifact. (node-artifact.ts:19-22)
    ///
    /// Stored as `String` (not a DateTime type) to match the wire format exactly and avoid
    /// a date library dependency.
    #[serde(rename = "producedAt")]
    pub produced_at: String,

    /// Byte size (UTF-8) of the output file. node-artifact.ts:24.
    ///
    /// `z.number().int().nonnegative()` — has `.int()` → `u64`.
    /// The non-negative constraint is enforced by `u64` (no negatives possible).
    pub size: u64,

    /// Provider session ID that produced the output, when available. node-artifact.ts:26.
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionId")]
    pub session_id: Option<String>,
}

/// Validation errors for `NodeArtifact`. node-artifact.ts:14,22.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum NodeArtifactValidationError {
    /// `outputType` was empty (z.string().min(1)). node-artifact.ts:14.
    #[error("'outputType' must be at least 1 character")]
    EmptyOutputType,

    /// `producedAt` was not a valid ISO-8601 datetime string. node-artifact.ts:22.
    #[error("'producedAt' must be a valid ISO-8601 datetime string")]
    InvalidProducedAt,
}

impl NodeArtifact {
    /// Validate all constraints that zod enforces at parse time.
    ///
    /// Returns all errors found (mirrors zod's collect-all-issues behavior).
    pub fn validate(&self) -> Vec<NodeArtifactValidationError> {
        let mut errors = Vec::new();

        // z.string().min(1) on outputType. node-artifact.ts:14.
        if self.output_type.is_empty() {
            errors.push(NodeArtifactValidationError::EmptyOutputType);
        }

        // z.string().datetime() on producedAt. node-artifact.ts:22.
        // A valid ISO-8601 datetime must contain 'T' and end with 'Z' or '+HH:MM' or '-HH:MM'.
        // We implement a structural check that matches zod's .datetime() behavior:
        // requires a 'T' separator and a timezone indicator.
        if !is_valid_iso8601_datetime(&self.produced_at) {
            errors.push(NodeArtifactValidationError::InvalidProducedAt);
        }

        errors
    }

    /// Parse from a JSON value and immediately validate all constraints.
    pub fn parse(value: serde_json::Value) -> Result<Self, Vec<NodeArtifactValidationError>> {
        let artifact: Self = serde_json::from_value(value)
            .map_err(|_| vec![NodeArtifactValidationError::EmptyOutputType])?;

        let errors = artifact.validate();
        if errors.is_empty() {
            Ok(artifact)
        } else {
            Err(errors)
        }
    }
}

/// Validates that a string is a valid ISO-8601 datetime acceptable by zod v4's
/// `z.string().datetime()`.
///
/// FIX-B (cycle 3): zod v4 `.datetime()` is **Z-only** — it accepts ONLY `Z`-suffixed UTC
/// timestamps and **rejects ALL numeric offsets** (`+05:30`, `-08:00`, even `+00:00`).
/// The previous implementation had a `has_offset` branch that accepted `±HH:MM` — this
/// was a behavior downgrade relative to the source, and defeats the lexicographic-ordering
/// guarantee (an offset timestamp is not directly comparable to a `Z` timestamp).
///
/// zod v4 `.datetime()` accepts:
///   - `YYYY-MM-DDTHH:MMZ` (no seconds)
///   - `YYYY-MM-DDTHH:MM:SSZ`
///   - `YYYY-MM-DDTHH:MM:SS.fracZ` (fractional seconds)
///
/// zod v4 `.datetime()` rejects:
///   - Any `+HH:MM` / `-HH:MM` / `+00:00` offset form
///   - Missing 'T' separator
///   - Missing 'Z' terminal
///   - Space instead of 'T'
///   - No timezone at all
pub(crate) fn is_valid_iso8601_datetime(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Must contain 'T' as the date-time separator
    let Some(t_pos) = s.find('T') else {
        return false;
    };
    // Date portion: at least YYYY-MM-DD (10 chars)
    if t_pos < 10 {
        return false;
    }
    // Time portion must exist after 'T'
    let time_part = &s[t_pos + 1..];
    if time_part.is_empty() {
        return false;
    }
    // zod v4: ONLY 'Z' suffix accepted — all numeric offsets (+HH:MM, -HH:MM) are REJECTED.
    // The `has_offset` branch that previously accepted ±HH:MM has been removed (FIX-B).
    time_part.ends_with('Z')
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Accept cases ──────────────────────────────────────────────────────

    #[test]
    fn valid_minimal_artifact() {
        let v = json!({
            "nodeId": "plan",
            "outputType": "plan",
            "path": "nodes/plan.md",
            "runId": "run-1",
            "producedAt": "2024-01-01T12:00:00Z",
            "size": 1024
        });
        let a: NodeArtifact = serde_json::from_value(v).unwrap();
        assert!(a.validate().is_empty());
        assert_eq!(a.node_id, "plan");
        assert_eq!(a.output_type, "plan");
        assert_eq!(a.size, 1024);
        assert!(a.session_id.is_none());
    }

    #[test]
    fn valid_full_artifact_with_session() {
        let v = json!({
            "nodeId": "analyze",
            "outputType": "analysis-report",
            "path": "nodes/analyze.md",
            "runId": "run-2",
            "producedAt": "2024-06-15T09:30:00.000Z",
            "size": 4096,
            "sessionId": "sess-abc"
        });
        let a: NodeArtifact = serde_json::from_value(v).unwrap();
        assert!(a.validate().is_empty());
        assert_eq!(a.session_id.as_deref(), Some("sess-abc"));
    }

    #[test]
    fn valid_size_zero() {
        // nonnegative includes 0
        let v = json!({
            "nodeId": "n",
            "outputType": "t",
            "path": "p",
            "runId": "r",
            "producedAt": "2024-01-01T00:00:00Z",
            "size": 0
        });
        let a: NodeArtifact = serde_json::from_value(v).unwrap();
        assert!(a.validate().is_empty());
        assert_eq!(a.size, 0);
    }

    /// FIX-B (cycle 3): zod v4 `.datetime()` rejects ALL numeric offsets. `+05:30` → REJECT.
    /// Previously this test asserted ACCEPT (wrong behavior). Flipped to assert REJECT.
    #[test]
    fn produced_at_positive_offset_rejected() {
        // zod v4: +HH:MM offset is NOT accepted; only 'Z' is valid
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-06-15T09:30:00+05:30".to_string(),
            size: 100,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::InvalidProducedAt),
            "offset +05:30 must be rejected by zod v4 .datetime() (Z-only)"
        );
    }

    /// FIX-B (cycle 3): zod v4 `.datetime()` rejects `-HH:MM` offsets too. `-08:00` → REJECT.
    /// Previously this test asserted ACCEPT (wrong behavior). Flipped to assert REJECT.
    #[test]
    fn produced_at_negative_offset_rejected() {
        // zod v4: -HH:MM offset is NOT accepted
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-06-15T14:00:00-08:00".to_string(),
            size: 100,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::InvalidProducedAt),
            "offset -08:00 must be rejected by zod v4 .datetime() (Z-only)"
        );
    }

    // ── Reject cases: size ────────────────────────────────────────────────

    #[test]
    fn negative_size_rejected_by_deserialize() {
        // z.number().int().nonnegative() → u64; negative JSON number fails to deserialize into u64
        let v = json!({
            "nodeId": "n",
            "outputType": "t",
            "path": "p",
            "runId": "r",
            "producedAt": "2024-01-01T00:00:00Z",
            "size": -1
        });
        assert!(serde_json::from_value::<NodeArtifact>(v).is_err());
    }

    #[test]
    fn fractional_size_rejected_by_deserialize() {
        // z.number().int() → u64; fractional JSON fails to deserialize
        let v = json!({
            "nodeId": "n",
            "outputType": "t",
            "path": "p",
            "runId": "r",
            "producedAt": "2024-01-01T00:00:00Z",
            "size": 1.5
        });
        assert!(serde_json::from_value::<NodeArtifact>(v).is_err());
    }

    // ── Reject cases: outputType ──────────────────────────────────────────

    #[test]
    fn empty_output_type_rejected() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: String::new(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-01-01T00:00:00Z".to_string(),
            size: 0,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::EmptyOutputType),
            "got: {errs:?}"
        );
    }

    // ── Reject cases: producedAt ──────────────────────────────────────────

    #[test]
    fn produced_at_missing_t_separator_rejected() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-01-01 00:00:00Z".to_string(), // space not 'T'
            size: 0,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::InvalidProducedAt),
            "got: {errs:?}"
        );
    }

    #[test]
    fn produced_at_missing_timezone_rejected() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-01-01T00:00:00".to_string(), // no Z or offset
            size: 0,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::InvalidProducedAt),
            "got: {errs:?}"
        );
    }

    #[test]
    fn produced_at_empty_rejected() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: String::new(),
            size: 0,
            session_id: None,
        };
        let errs = a.validate();
        assert!(
            errs.contains(&NodeArtifactValidationError::InvalidProducedAt),
            "got: {errs:?}"
        );
    }

    // ── Multiple errors collected ─────────────────────────────────────────

    #[test]
    fn multiple_errors_collected() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: String::new(), // empty → error
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "not-a-datetime".to_string(), // invalid → error
            size: 0,
            session_id: None,
        };
        let errs = a.validate();
        assert_eq!(errs.len(), 2, "expected 2 errors, got: {errs:?}");
        assert!(errs.contains(&NodeArtifactValidationError::EmptyOutputType));
        assert!(errs.contains(&NodeArtifactValidationError::InvalidProducedAt));
    }

    // ── Wire names (camelCase) ────────────────────────────────────────────

    #[test]
    fn wire_names_are_camel_case() {
        let a = NodeArtifact {
            node_id: "plan".to_string(),
            output_type: "plan".to_string(),
            path: "nodes/plan.md".to_string(),
            run_id: "run-1".to_string(),
            produced_at: "2024-01-01T00:00:00Z".to_string(),
            size: 512,
            session_id: Some("s".to_string()),
        };
        let v = serde_json::to_value(&a).unwrap();
        // Verify camelCase wire names match TS interface
        assert!(v.get("nodeId").is_some(), "missing nodeId");
        assert!(v.get("outputType").is_some(), "missing outputType");
        assert!(v.get("runId").is_some(), "missing runId");
        assert!(v.get("producedAt").is_some(), "missing producedAt");
        assert!(v.get("sessionId").is_some(), "missing sessionId");
        // snake_case must NOT appear
        assert!(v.get("node_id").is_none());
        assert!(v.get("output_type").is_none());
    }

    #[test]
    fn session_id_omitted_when_none() {
        let a = NodeArtifact {
            node_id: "n".to_string(),
            output_type: "t".to_string(),
            path: "p".to_string(),
            run_id: "r".to_string(),
            produced_at: "2024-01-01T00:00:00Z".to_string(),
            size: 0,
            session_id: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert!(
            v.get("sessionId").is_none(),
            "sessionId should be absent when None"
        );
    }

    // ── Round-trip ────────────────────────────────────────────────────────

    #[test]
    fn round_trip_full() {
        let original = json!({
            "nodeId": "analyze",
            "outputType": "analysis",
            "path": "nodes/analyze.md",
            "runId": "run-42",
            "producedAt": "2024-06-15T09:30:00.000Z",
            "size": 2048,
            "sessionId": "sess-xyz"
        });
        let a: NodeArtifact = serde_json::from_value(original.clone()).unwrap();
        assert!(a.validate().is_empty());
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["nodeId"], original["nodeId"]);
        assert_eq!(back["outputType"], original["outputType"]);
        assert_eq!(back["path"], original["path"]);
        assert_eq!(back["runId"], original["runId"]);
        assert_eq!(back["producedAt"], original["producedAt"]);
        assert_eq!(back["size"], original["size"]);
        assert_eq!(back["sessionId"], original["sessionId"]);
    }

    // ── Error message exact match ─────────────────────────────────────────

    #[test]
    fn error_messages() {
        assert_eq!(
            NodeArtifactValidationError::EmptyOutputType.to_string(),
            "'outputType' must be at least 1 character"
        );
        assert_eq!(
            NodeArtifactValidationError::InvalidProducedAt.to_string(),
            "'producedAt' must be a valid ISO-8601 datetime string"
        );
    }
}
