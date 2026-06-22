# Parity Verdict — UNIT WF-19 (Workflow Store Interface)

**Date:** 2026-06-21
**Verdict:** PASS
**Source (pristine, READ-ONLY, untouched):** `packages/workflows/src/store.ts`
**Rust port:** `crates/har-ledger/src/store.rs` (+ re-exports `crates/har-ledger/src/lib.rs`)
**Gate style:** QUALIFIED pure-INTERFACE (A=live differential, B=shape-fidelity, C=contract encodings)
**Symbols verified:** 2/2 — `IWorkflowStore`, `WORKFLOW_EVENT_TYPES` → both `- [x]`.

---

## (A) Live differential — WORKFLOW_EVENT_TYPES + event strings — PASS

Ran the LIVE source via bun 1.3.14 (throwaway oracle, imported `store.ts`, printed JSON, then DELETED).
Live array = 21 strings. Diffed against THREE independent Rust encodings:

| Encoding | Result vs live (count/order/spelling) |
|---|---|
| `WORKFLOW_EVENT_TYPES: [&str; 21]` const | IDENTICAL |
| `WorkflowEventType` enum via real `serde_json::to_value` (test `event_type_enum_serde_matches_const_list`) | IDENTICAL |
| `WorkflowEventType::as_str()` map | IDENTICAL |

21/21 strings match exactly. The serde proof is a *real run* (the in-module test serializes each variant and asserts against the const list + round-trips), not just a by-hand rename check. `#[serde(rename_all = "snake_case")]` over the PascalCase variants reproduces every source string.

## (B) Structural shape-fidelity — 20/20 methods — PASS

Both sides have **exactly 20** methods, 1:1 by name (TS camelCase ↔ Rust snake_case), none missing, none invented. Param/return mapping faithful throughout:
- `Promise<T|null>` → `Result<Option<T>, StoreError>`; throwing `Promise<T>` → `Result<T, StoreError>`; `Promise<void>` → `Result<(), StoreError>` (except createWorkflowEvent, see C).
- `string?`→`Option<String>`, `Date`→`DateTime<Utc>`, `Record<string,unknown>`→`Map<String,Value>`, inline objects → named structs.

Watch-list, all confirmed:
- `getActiveWorkflowRunByPath` `self?: {id, startedAt: Date}` → `Option<ActiveRunSelf{id: String, started_at: DateTime<Utc>}>`. ✓
- `updateWorkflowRun` `Partial<Pick<WorkflowRun,'status'|'metadata'>>` → `WorkflowRunUpdate{status: Option, metadata: Option}` (both optional). ✓
- `upsertWorkflowNodeSession` intersection → `UpsertNodeSessionParams` with all 6 fields (4 key flattened + provider_session_id + last_run_id: Option). ✓
- `deleteWorkflowNodeSessions` filter → `DeleteSessionsFilter{workflow_name: String, scope_key/node_id/provider: Option}`. ✓
- `WorkflowNodeSessionKey`: exactly 4 String fields (workflow_name, node_id, scope_key, provider). ✓
- Inline returns → `CancelResult{cancelled}`, `FailOrphanedRunsResult{count}`, `DeleteSessionsResult{deleted}`, `CodebaseRecord{id,name,repository_url: Option<String>,default_cwd}`. ✓

Referenced schema types (WorkflowRun, WorkflowRunStatus, ApprovalContext, WorkflowNodeSession) are RE-USED from `har-workflow-schema` (their own units), not re-defined/degraded here. ✓

## (C) Load-bearing contract encodings — PASS

- `createWorkflowEvent` "MUST NOT throw" → Rust `async fn create_workflow_event(&self, data) ` returns **`()`** (NOT Result). Contract encoded structurally. ✓
- `getCompletedDagNodeOutputs` "Throws on DB error" + "preserves insertion order" → `Result<IndexMap<String, String>, StoreError>` (fallible + **IndexMap**, not HashMap/BTreeMap). ✓ `getCodebaseEnvVars` likewise IndexMap.
- Long doc-contracts carried into Rust doc comments: getActiveWorkflowRunByPath tiebreaker/stale-pending paragraph ✓; deleteWorkflowNodeSessions provider-filter semantics ✓; createWorkflowEvent observable-only ✓.

## Build/clippy/test gate (re-run independently) — GREEN

- `cargo build -p har-ledger` ✓
- `cargo clippy -p har-ledger --all-targets -- -D warnings` ✓ (clean)
- `cargo test -p har-ledger` ✓ 14 passed / 0 failed
- Object-safe: `_assert_object_safe(_: &dyn WorkflowStore)` + `Option<Box<dyn WorkflowStore>>` compile. ✓
- Whole-workspace `cargo build` ✓ (har-dag-executor consumes har-ledger; not broken).

## Benign divergences (recorded)

- `- [≈]` TS `Record<string,unknown>` (unordered) → `serde_json::Map<String,Value>` for run metadata: benign (metadata is opaque JSON; ordering not contractual).
- `- [≈]` TS `Record<string,string>` returns → `IndexMap` (getCompletedDagNodeOutputs/getCodebaseEnvVars): chosen for deterministic insertion-order; STRICTER than source's unordered guarantee — superset, not downgrade. The completed-DAG-outputs doc explicitly requires insertion order, so IndexMap is mandatory there, not merely a choice.
- `- [≈]` row counts `{count}`/`{deleted}` TS `number` (f64) → Rust `u64`: benign (non-negative integer cardinalities; u64 is the correct domain).

**No downgrades. No missing/invented symbols. No disguised `[≠]`. VERDICT: PASS.**
