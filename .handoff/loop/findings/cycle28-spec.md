# Cycle 28 session spec — complete the DB backend + land `impl WorkflowStore` (7 task cards)

> Owner process rule (2026-06-21): spec work as tracked task cards BEFORE implementing.
> Owner directive (2026-06-22): "/harness:rust-port-merge resume and pick 7 new task for this session."
> dest_repo = (none) → straight Archon→Rust port; merge-into-Y gates N/A.
> This session = 7 units, dependency-ordered. Goal: a COMPLETE, differentially-verified
> `impl WorkflowStore` over the SQL adapters — the exact dependency that unblocks the WF-09 keystone.

## Why these 7 (the WorkflowStore closure)
The WF-19 `WorkflowStore` trait (cycle 25, har-ledger/src/store.rs) has 20 methods spanning the source
db modules: `workflows.ts` (run CRUD + resume CAS + getCompletedDagNodeOutputs), `workflow-events.ts`
(createWorkflowEvent), `workflow-node-sessions.ts` (session CRUD), and `codebases.ts`
(getCodebase + getCodebaseEnvVars). Implementing it needs a working pg adapter + connection auto-detect
+ pg bundled schema first. So the 7 = 3 foundation + 4 store-impl modules. Conversations (CO-07) is
orchestrator-facing (NOT in the store trait) → deferred to next session.

## Driver / verification tooling
- sqlx 0.9.0 (already a dep, cycle 27). ADD the `postgres` feature for T2.
- SQLite gate = in-process **bun:sqlite 1.3.14** (as cycle 27).
- Postgres gate = a **docker `postgres:16-alpine`** throwaway container (docker IS available; no local pg
  binary, no DATABASE_URL). The verifier spins it up, runs the SAME battery through the live TS
  `PostgresAdapter` (via `pg` from bun) and the Rust sqlx-postgres adapter, diffs, then tears it down.
  If docker pull is blocked at verify time → pg gate is STRUCTURAL + sqlite-differential, pg rows
  `- [~]` with the exact blocker (no silent downgrade; record as `- [!]` if truly unrunnable).

## Tasks (dependency order — foundation sequential, store-modules parallelizable after)

### T1 = CO-03 — Postgres bundled schema (`getSchemaSQL`)
- Source: `core/src/db/bundled-schema.ts` (+ `bundled-schema.generated.ts`, `migrations/000_combined.sql`).
- Rust: `crates/har-db/src/schema.rs` (or `bundled_schema.rs`). `include_str!` the 482-line combined.sql
  (the PG-dialect schema, 17 `remote_agent_*` tables) → `get_schema_sql() -> &'static str`. Binary-build
  embed AND source-build disk-read collapse to one `include_str!` (compile-time embed = faithful, the TS
  branch is a bun packaging artifact; document as `- [≈]`). SQLite keeps its c27 inlined schema (NOT this).
- Gate: byte-equal the embedded const vs `migrations/000_combined.sql` on disk; the pg adapter init (T2)
  exercises it live.

### T2 = CO-01b — PostgresAdapter + installNotifyTrigger + DbNotificationListener (PgListener)
- Source: `core/src/db/adapters/postgres.ts` (261 lines).
- Rust: `crates/har-db/src/postgres.rs`. sqlx `PgPool` (max=10, idle/connect timeouts), `Database` trait
  impl (query/with_transaction/close/dialect/sql) over the c27 trait. `schemaInitPromise` → init runs in
  ctor (advisory `pg_advisory_xact_lock(1796)` in a txn, run getSchemaSQL, COMMIT; rollback-on-error
  logged `db.postgres_schema_init_*`). `installNotifyTrigger` (advisory lock 1797, WORKFLOW_EVENT_NOTIFY_SQL,
  best-effort/non-fatal warn). `DbNotificationListener::listen` via sqlx `PgListener` on a held connection
  (channel-name validated `^[a-z_][a-z0-9_]*$/i`, destroy-not-recycle on unsubscribe/error, returns
  unsubscribe closure). pg binds `$N` natively (no convertPlaceholders). ADD sqlx `postgres` feature.
- Gate: docker pg differential (schema init idempotency on double-construct; query rows/rowCount; RETURNING;
  with_transaction commit+rollback; LISTEN/NOTIFY round-trip via pg_notify trigger fires on workflow_events
  insert; invalid channel name throws exact message).

### T3 = CO-02 — Connection auto-detect
- Source: `core/src/db/connection.ts` (132 lines).
- Rust: `crates/har-db/src/connection.rs`. Singleton `getDatabase` (DATABASE_URL→Postgres else
  SQLite at `getArchonHome()/archon.db`; ARCHON_DOCKER=true SQLite-path warn `db.docker_using_sqlite`),
  `getDialect`, `getDatabaseType()->'postgresql'|'sqlite'` (env-only, no init), `getDbNotificationListener`
  (null unless pg + listen-capable), `closeDatabase`, `resetDatabase`, legacy `pool` forwarder
  (query/end). Rust idiom: `OnceCell`/`Mutex<Option<Arc<dyn Database>>>` singleton (mirror the TS module
  singleton; reset for tests). async ctor seam (sqlx pools are async to build) — document the ctor-async
  `- [≈]` vs TS sync `new`.
- Gate: getDatabaseType env matrix (set/unset DATABASE_URL); sqlite selection writes archon.db at
  ARCHON_HOME; docker-warn fires; getDbNotificationListener null for sqlite; singleton identity + reset.

### T4 = CO-04 — Workflow DB operations (the behavior-rich core, 1088 lines)
- Source: `core/src/db/workflows.ts`. Rust: `crates/har-db/src/workflows.rs` (or `core` db module).
- The bulk of `impl WorkflowStore`: createWorkflowRun, getWorkflowRun, getActiveWorkflowRunByPath
  (self-tiebreaker), findResumableRun, failOrphanedRuns, **resumeWorkflowRun (CAS on status)**,
  updateWorkflowRun, updateWorkflowActivity, getWorkflowRunStatus, completeWorkflowRun, failWorkflowRun,
  pauseWorkflowRun, cancelWorkflowRun, **getCompletedDagNodeOutputs** (insertion-ordered IndexMap, throws).
  Dialect-parameterized SQL (uses c26 SqlDialect builders for json_merge/now/now_minus_days/etc.).
- Gate: differential vs live bun over BOTH backends — esp. the **resume CAS** (concurrent resume → exactly
  one wins; cf. source `workflows.resume-cas.integration.test.ts`), orphan-run sweep, completed-DAG-outputs
  ordering + throw-on-unparseable. row-count f64→u64 `- [≈]` carried.

### T5 = CO-05 — Workflow Events DB
- Source: `core/src/db/workflow-events.ts` (222 lines). Rust: `crates/har-db/src/workflow_events.rs`.
- `createWorkflowEvent` (fire-and-forget insert; MUST-NOT-THROW contract from WF-19 — swallow+log on
  error, return `()`), `getWorkflowEventsSince(runId, since)` (SSE catch-up, ordered). Event-type strings
  must match WORKFLOW_EVENT_TYPES (already pinned c25). pg insert fires the notify trigger (T2).
- Gate: differential insert+readback ordering; createWorkflowEvent never propagates a DB error; since-cursor
  filtering byte-identical.

### T6 = CO-06 — Workflow Node Sessions DB
- Source: `core/src/db/workflow-node-sessions.ts` (121 lines). Rust: `crates/har-db/src/workflow_node_sessions.rs`.
- `getWorkflowNodeSession`, `upsertWorkflowNodeSession` (composite PK
  (workflow_name,node_id,scope_key,provider) ON CONFLICT upsert), `deleteWorkflowNodeSessions`
  (provider-filter doc-contract). persist_session feature backing.
- Gate: upsert idempotency (insert then update same PK → one row, updated session id); delete filter matrix;
  null last_run_id round-trip.

### T7 = CO-08 — Codebases DB → WIRE the full `impl WorkflowStore`
- Source: `core/src/db/codebases.ts` (183 lines). Rust: `crates/har-db/src/codebases.rs`.
- The store's remaining methods: `getCodebase(id)`, `getCodebaseEnvVars(codebaseId)` (+ the CRUD the
  module exports). THEN assemble `struct SqlWorkflowStore { db: Arc<dyn Database> }` and
  `impl WorkflowStore for SqlWorkflowStore` wiring T4/T5/T6/T7 — a COMPLETE object-safe store.
- Gate: getCodebase/getCodebaseEnvVars differential; the assembled `impl WorkflowStore` compiles
  object-safe (`Box<dyn WorkflowStore>`) and passes an end-to-end store smoke battery (create run →
  event → node session → complete → read back) on SQLite live-diffed vs bun.

## Acceptance (every task)
- Workspace `cargo build` + `clippy --all-targets -D warnings` + `fmt` clean; full suite green.
- Differential gate PASS (zero divergences, or only benign `- [≈]` T→Value / rowCount u64 / ctor-async).
- No stubs / todo!() / "simplified" / dropped branch. Co-author trailer. Local commit per unit (push deferred).
- Ledger CO items flipped `- [x]` with cycle-28 annotations; loop_state + HANDOFF updated; ICM stored.
- pg gate: if docker unavailable at verify → pg rows `- [~]`/`- [!]` with exact blocker, sqlite proven; NO silent downgrade.

## Session budget
This session raises the effective unit budget to **7** per the owner directive (default cycle_budget=3).
Foundation T1→T2→T3 sequential; store-modules T4..T7 may overlap (siblings over the same adapter), each
gated + committed independently.
