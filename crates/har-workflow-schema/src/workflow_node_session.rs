//! PORT of `packages/workflows/src/schemas/workflow-node-session.ts`.
//!
//! UNIT WF-08: WorkflowNodeSession — per-node provider session persistence record.
//!
//! NEEDS-HUMAN resolved: the actual workflow-node-session.ts was read. Resolved shape:
//!   - `workflow_name: String`      (z.string())
//!   - `node_id: String`            (z.string())
//!   - `scope_key: String`          (z.string())
//!   - `provider: String`           (z.string())
//!   - `provider_session_id: String` (z.string())
//!   - `last_run_id: Option<String>` (z.string().nullable()) — FK to workflow_runs, ON DELETE SET NULL
//!   - `created_at: String`         (z.string())
//!   - `updated_at: String`         (z.string())
//!
//! The composite primary key is `(workflow_name, node_id, scope_key, provider)` —
//! one row per unique combination. workflow-node-session.ts:15.
//!
//! Purpose: supports the `persist_session: true` cross-run feature. The executor loads a
//! stored `provider_session_id` at the start of a run and passes it as `resumeSessionId`
//! on a later run with the same scope. workflow-node-session.ts:7-11.
//!
//! Numeric audit: no numeric fields in workflow-node-session.ts. All fields are strings or
//! nullable strings. No zod `.number()` calls.
//!
//! Trim audit: no `.trim()` transforms in workflow-node-session.ts. All z.string() fields
//! stored verbatim.
//!
//! Distinct from `AgentRequestOptions.persistSession` (the Claude SDK on-disk transcript
//! flag) — this records the provider's session ID for cross-run reuse. workflow-node-session.ts:8-10.
//!
//! FIX-A/D4 (cycle 3): `last_run_id: z.string().nullable()` — zod v4 `.nullable()` means the
//! key MUST be present; an absent key is REJECTED. The previous implementation used plain
//! `Option<String>` which accepted absent keys. The fix uses `#[serde(deserialize_with)]`
//! with a custom deserializer that errors on a missing key but maps `null`→`None`.
//! Serialize: `None` serializes as explicit `null` (no `skip_serializing_if`), matching
//! the ON DELETE SET NULL DB behavior where the column is present (as NULL).

use serde::{Deserialize, Deserializer, Serialize};

/// Deserializes a field that is `.nullable()` in zod v4:
///   - Key MUST be present (absent → error, matching zod v4 required-present semantics)
///   - JSON `null` → `None`
///   - JSON string → `Some(String)`
///
/// Without this, plain `Option<String>` accepts absent keys (serde default), which diverges
/// from zod v4 `.nullable()` which rejects absent keys.
fn deser_required_nullable_string<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    // `Option<String>` serde deserializer handles null→None and string→Some.
    // Key presence is enforced by serde itself: this function is only called when the key
    // is present. If the key is absent and there is no `#[serde(default)]`, serde returns
    // a "missing field" error — exactly the zod `.nullable()` required-present behavior.
    let v: Option<String> = Option::deserialize(de)?;
    Ok(v)
}

/// Per-node provider session record for cross-run session persistence.
///
/// One row per `(workflow_name, node_id, scope_key, provider)` composite key.
/// Stored when a node opts in via `persist_session: true` and the resolved provider
/// supports session resume. workflow-node-session.ts:15-26.
///
/// Used by the DAG executor to:
///   1. Load `provider_session_id` before a node runs (if a prior session exists for this key)
///   2. Pass it as `resumeSessionId` in `AgentRequestOptions`
///   3. Upsert the new `provider_session_id` after the node completes
///
/// The `last_run_id` FK is ON DELETE SET NULL — deleting the originating run clears this
/// field without dropping the resumable session itself. workflow-node-session.ts:22-24.
///
/// FIX-A/D4: `last_run_id` is `z.string().nullable()` in zod v4, meaning the key is
/// REQUIRED to be present (as a string value or `null`). An absent key is rejected.
/// Plain `Option<String>` (old port) accepted absent — that was the wrong behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeSession {
    /// Name of the workflow owning this session. Part of the composite PK.
    /// workflow-node-session.ts:16.
    pub workflow_name: String,

    /// ID of the node owning this session. Part of the composite PK.
    /// workflow-node-session.ts:17.
    pub node_id: String,

    /// Scope discriminant allowing multiple sessions per (workflow, node) pair.
    /// Typically the run's codebase ID or conversation ID. Part of the composite PK.
    /// workflow-node-session.ts:18.
    pub scope_key: String,

    /// Provider identifier (e.g. "claude", "codex"). Part of the composite PK.
    /// workflow-node-session.ts:19.
    pub provider: String,

    /// The provider's opaque session token for the resumed session.
    /// Passed back as `resumeSessionId` in `AgentRequestOptions` on next run.
    /// workflow-node-session.ts:20.
    pub provider_session_id: String,

    /// ID of the most recent workflow run that wrote this session record.
    ///
    /// `z.string().nullable()` — required-present (FIX-A/D4):
    ///   - Key MUST be present in JSON (absent → REJECT, matching zod v4 .nullable())
    ///   - JSON `null` → `None` (ON DELETE SET NULL in DB)
    ///   - JSON string → `Some(String)`
    ///
    /// Serialize: `None` → explicit `null` (no skip_serializing_if), matching the DB
    /// ON DELETE SET NULL behavior where the column value is NULL (present, not missing).
    #[serde(deserialize_with = "deser_required_nullable_string")]
    pub last_run_id: Option<String>,

    /// ISO-8601 timestamp of record creation. workflow-node-session.ts:25.
    pub created_at: String,

    /// ISO-8601 timestamp of last update. workflow-node-session.ts:26.
    pub updated_at: String,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Accept cases ──────────────────────────────────────────────────────

    #[test]
    fn valid_minimal_session() {
        let v = json!({
            "workflow_name": "deploy",
            "node_id": "analyze",
            "scope_key": "repo-42",
            "provider": "claude",
            "provider_session_id": "sess-abc123",
            "last_run_id": "run-1",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        });
        let s: WorkflowNodeSession = serde_json::from_value(v).unwrap();
        assert_eq!(s.workflow_name, "deploy");
        assert_eq!(s.node_id, "analyze");
        assert_eq!(s.scope_key, "repo-42");
        assert_eq!(s.provider, "claude");
        assert_eq!(s.provider_session_id, "sess-abc123");
        assert_eq!(s.last_run_id.as_deref(), Some("run-1"));
        assert_eq!(s.created_at, "2024-01-01T00:00:00Z");
        assert_eq!(s.updated_at, "2024-01-01T00:00:00Z");
    }

    #[test]
    fn last_run_id_nullable() {
        // ON DELETE SET NULL — last_run_id can be present-as-null
        let v = json!({
            "workflow_name": "w",
            "node_id": "n",
            "scope_key": "s",
            "provider": "codex",
            "provider_session_id": "sess-xyz",
            "last_run_id": null,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z"
        });
        let s: WorkflowNodeSession = serde_json::from_value(v).unwrap();
        assert!(s.last_run_id.is_none());
    }

    /// FIX-A/D4 (cycle 3): `last_run_id` absent → REJECT.
    /// zod v4 `.nullable()` requires the key present. An absent key is invalid.
    /// Previously this test asserted ACCEPT with a "pragmatic wire parity" comment —
    /// that was an unflagged divergence from the source semantics. Flipped to REJECT.
    #[test]
    fn last_run_id_absent_rejected() {
        let v = json!({
            "workflow_name": "w",
            "node_id": "n",
            "scope_key": "s",
            "provider": "p",
            "provider_session_id": "sid",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        });
        // zod v4: .nullable() ≠ .optional(); absent key → REJECT
        assert!(serde_json::from_value::<WorkflowNodeSession>(v).is_err(),
            "absent last_run_id must be rejected (zod v4 .nullable() is required-present)");
    }

    /// FIX-A serialize: `last_run_id: None` must serialize as explicit `null`, not absent.
    #[test]
    fn last_run_id_none_serializes_as_null() {
        let s = WorkflowNodeSession {
            workflow_name: "w".to_string(),
            node_id: "n".to_string(),
            scope_key: "s".to_string(),
            provider: "claude".to_string(),
            provider_session_id: "sid".to_string(),
            last_run_id: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let v = serde_json::to_value(&s).unwrap();
        // None must serialize as explicit null (not absent key)
        assert!(v.get("last_run_id").is_some(), "last_run_id key must be present");
        assert!(v["last_run_id"].is_null(), "last_run_id None must be explicit null");
    }

    // ── Wire names (snake_case) ───────────────────────────────────────────

    #[test]
    fn wire_names_are_snake_case() {
        // TS source uses snake_case field names throughout (workflow-node-session.ts:16-26)
        // These are already snake_case — no renaming needed
        let s = WorkflowNodeSession {
            workflow_name: "w".to_string(),
            node_id: "n".to_string(),
            scope_key: "s".to_string(),
            provider: "claude".to_string(),
            provider_session_id: "sid".to_string(),
            last_run_id: Some("run-1".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let v = serde_json::to_value(&s).unwrap();
        // snake_case wire names (matches TS zod schema field names)
        assert!(v.get("workflow_name").is_some(), "missing workflow_name");
        assert!(v.get("node_id").is_some(), "missing node_id");
        assert!(v.get("scope_key").is_some(), "missing scope_key");
        assert!(v.get("provider").is_some(), "missing provider");
        assert!(v.get("provider_session_id").is_some(), "missing provider_session_id");
        assert!(v.get("last_run_id").is_some(), "missing last_run_id");
        assert!(v.get("created_at").is_some(), "missing created_at");
        assert!(v.get("updated_at").is_some(), "missing updated_at");
    }

    // ── Round-trip ────────────────────────────────────────────────────────

    #[test]
    fn round_trip_with_last_run_id() {
        let original = json!({
            "workflow_name": "ci-pipeline",
            "node_id": "build",
            "scope_key": "pr-123",
            "provider": "claude",
            "provider_session_id": "sess-deadbeef",
            "last_run_id": "run-abc",
            "created_at": "2024-06-01T10:00:00Z",
            "updated_at": "2024-06-02T11:00:00Z"
        });
        let s: WorkflowNodeSession = serde_json::from_value(original.clone()).unwrap();
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["workflow_name"], original["workflow_name"]);
        assert_eq!(back["node_id"], original["node_id"]);
        assert_eq!(back["scope_key"], original["scope_key"]);
        assert_eq!(back["provider"], original["provider"]);
        assert_eq!(back["provider_session_id"], original["provider_session_id"]);
        assert_eq!(back["last_run_id"], original["last_run_id"]);
        assert_eq!(back["created_at"], original["created_at"]);
        assert_eq!(back["updated_at"], original["updated_at"]);
    }

    #[test]
    fn round_trip_null_last_run_id() {
        let original = json!({
            "workflow_name": "w",
            "node_id": "n",
            "scope_key": "s",
            "provider": "codex",
            "provider_session_id": "sess-xyz",
            "last_run_id": null,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        });
        let s: WorkflowNodeSession = serde_json::from_value(original.clone()).unwrap();
        let back = serde_json::to_value(&s).unwrap();
        // null last_run_id: serde serializes Option::None as null by default
        assert!(back["last_run_id"].is_null());
    }

    // ── Different providers for same (workflow, node, scope) ─────────────

    #[test]
    fn different_providers_same_node() {
        // Composite PK includes provider — multiple providers can have sessions for same node
        let v1 = json!({
            "workflow_name": "deploy",
            "node_id": "analyze",
            "scope_key": "repo-1",
            "provider": "claude",
            "provider_session_id": "claude-sess-1",
            "last_run_id": "run-1",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        });
        let v2 = json!({
            "workflow_name": "deploy",
            "node_id": "analyze",
            "scope_key": "repo-1",
            "provider": "codex",
            "provider_session_id": "codex-sess-1",
            "last_run_id": "run-1",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        });
        let s1: WorkflowNodeSession = serde_json::from_value(v1).unwrap();
        let s2: WorkflowNodeSession = serde_json::from_value(v2).unwrap();
        assert_eq!(s1.workflow_name, s2.workflow_name);
        assert_eq!(s1.node_id, s2.node_id);
        assert_ne!(s1.provider, s2.provider);
        assert_ne!(s1.provider_session_id, s2.provider_session_id);
    }
}
