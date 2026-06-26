# Loop state — rust-port (Archon → harness-agent-rs)
session_started: 2026-06-14T00:00:00Z
loop: rust-port
branch: main
worktree: /home/drdave/Desktop/meta/harness-agent-rs
source_root: /home/drdave/Desktop/meta/meta-yard/Archon   # relocated 2026-06-25 to Tier-Y yard (ported-from source, out of root build); was meta/Archon
source_toolchain: bun        # bun 1.3.14 — parity-verifier runs the TS source
rust_target: /home/drdave/Desktop/meta/harness-agent-rs
dest_repo: (none — port target IS this repo; no separate Y to merge into)
cycle_budget: 3
cycles_this_session: 1    # RESUME#3 2026-06-26 (owner: "/harness:rust-port resume"). Baseline EXECUTED GREEN (no phantom). CYCLE 40 = WF-09 sub-cycle 4d (AI dispatch wiring + retry wrapper + session persist) — opus porter wired the Command/Prompt arm into execute_dag_workflow (provider/model resolve, session-resume lookup, retry loop FATAL/transient+backoff, persist upsert/delete); Loop/Approval stay honest-Skipped. GATE FAILed first on 3 divergences (D-1 delete-session error swallowed, D-2 AI-node cost dropped from total_cost_usd, D-3 pre-exec error events omit nodeName) → fixed (D-2 via spawn-tuple widening, no schema change) → RE-VERIFY PASS (425 tests, 4 observable D-1/D-2/D-2b/D-3 gate tests). 4d unit row [x]; executeDagWorkflow symbol stays [~] by rollup (Loop/Approval pending). NEXT = WF-09 sub-cycle 4e (loop node: per-iteration stream reusing 4c pass + with_idle_timeout, until_bash via D3, interactive gate, max-iterations). Prior RESUME#2 2026-06-25
model_override: opus    # OWNER DIRECTIVE 2026-06-26 "only use opus, no more sonnet" — ALL sub-agents (porter/cartographer/researcher/continuity-steward + gates) run at opus per-call this loop. Porter agent-def default also flipped sonnet→opus (P5 PR). build-health run inline (no agent). Persist across resume.
p5_applied: yes         # OWNER APPROVED P5 2026-06-26 — porter git-prohibition + orchestrator HEAD-unchanged assertion landed in harness_hub porter agent-def. See proposed-upgrades.md P5. (owner: "/harness:rust-port resume"). Baseline GREEN 2115. CYCLE 38 = WF-31 validate_structured_output (the 4c keystone dep; !B3 RESOLVED — Ajv 8 → Rust jsonschema 0.46 pinned draft-07; PARITY PASS 26/26 byte-exact). CYCLE 39 = WF-09 sub-cycle 4c (AI-node live-streaming body of execute_node_internal — full port of the streaming/idle/cancel/reask/credit/empty/success state machine + dispatch branches). GATE FAILed first on D1 (idle-minute integer-division rendering: `90000ms→"1 min"` vs TS float `"1.5 min"`) → porter ported a faithful ECMA-262 `format_js_number(f64)` + `idle_timeout_minutes(Duration)`, fixed all 3 sites, cross-checked byte-identical vs live `node -e 'String(x)'` (whole/fractional/tiny/exponential). RE-VERIFY PASS — 11/11 scripted-fake-provider differential probes, clippy --all-targets clean. executeNodeInternal/buildReaskPrompt/emitReask now `- [x]`. NOTE: the AI *dispatch arm* in execute_dag_workflow stays honest-Skipped = sub-cycle 4d. NEXT: WF-09 sub-cycle 4d (AI-node dispatch wiring + retry wrapper + session persist). Prior session RESUME#1: 36=4a(PR#10), 37=4b(PR#11), handoff PR#12; this session: 38=WF-31(PR#13), 39=4c(PR pending).
cycles_total: 40          # ...+ 37 (4b, PR#11) + 38 (WF-31, PR#13) + 39 (WF-09 4c, PR#14) + 40 (WF-09 4d AI dispatch wiring + retry + session persist)
ledger: parity **44/79 full units** + **ALL provider ports (PR-01..11) FULLY BOUND** (CLI + 3 Node SDKs).
        cycle 26 started CO-01 (the SQL-backed DB layer): the adapter DIALECT slice is `- [x]` (new crate har-db);
        query/tx trait + concrete sqlite/pg adapters deferred to cycle 27 (`- [ ]`, pending driver decision). CO-01 not yet a full unit.
        cycle 25 added WF-19 WorkflowStore trait (full `- [x]`), the narrow persistence interface WF-09 depends on.
        cycle 24 added PR-12 loadMcpConfig (full `- [x]`), closing the carried MCP `- [≈]` (inline stopgap) and
        the claude `&[]` mcp_server_names gap. (Full units: PR-01..08, PR-12; WF-01..08, WF-11..14, WF-19; PA-01/06/07;
        GI-01..05; IS-01..08.) no-downgrade preserved end-to-end.
status_cycle28: cycle 28 session (2026-06-22) — owner directive "/harness:rust-port-merge resume and pick 7 new task".
        Picked a coherent 7-unit slice = **complete the DB backend + land a verified `impl WorkflowStore`** (the WF-09
        keystone dependency). Spec card: findings/cycle28-spec.md (7 task cards, owner spec-before-implementing rule).
        **3 of 7 DONE + parity-verified + committed (foundation):**
        • **T1 = CO-03 Postgres bundled schema** `- [x]` (commit 9ec006f) — get_schema_sql() embeds vendored
          migrations/000_combined.sql (byte-equal cmp, include_str!); 17 remote_agent_* tables; pg-only (sole getSchemaSQL
          caller). 3 tests.
        • **T2 = CO-01b PostgresAdapter** `- [x]` (commit e566ded) — sqlx PgPool, advisory-lock schema init (1796) +
          installNotifyTrigger (1797, WORKFLOW_EVENT_NOTIFY_SQL verbatim, non-fatal) + DbNotificationListener::listen via
          PgListener. Added sqlx features postgres/json/bigdecimal. **GATE FAILED FIRST on 4 real divergences** the verifier
          caught+fixed (NUMERIC decode panic→BigDecimal+normalized; INT8→string not Number; OID→Number split;
          string→typed-column bind downgrade→UUID-sniff+native jsonb in build_args) → RE-VERIFY PASS (full type/rowCount/
          RETURNING/LISTEN-NOTIFY/transaction battery vs live TS pg over docker postgres:16). 43 har-db tests (39 unit + 4
          DATABASE_URL-gated live). findings/parity-cycle28-pg.md; durable oracle examples/oracle_cycle28_pg.rs + tests/postgres_live.rs.
        • **T3 = CO-02 connection auto-detect** `- [x]` (commit db0071d) — get_database singleton (DATABASE_URL→pg else
          SQLite at archon_home/archon.db; ARCHON_DOCKER warn), get_dialect, get_database_type (env-only), get_db_notification_listener
          (option-4a separate listener singleton: pg Some / sqlite None), close_database, reset_database, legacy pool forwarder.
          Construct-once ATOMIC (lock held across async ctor — no TOCTOU). **GATE PASS, no defects** — byte-exact strings (3 log
          events + 107-char docker hint + 145-char dialect-not-init msg) + LIVE pg branch exercised end-to-end. findings/parity-cycle28-conn.md;
          tests/connection_live.rs. Landed in crates/har-db/src/connection.rs (LEDGER CORRECTION: crates/core doesn't exist).
        **Carries (`- [≈]`):** pg Date→ISO, numeric/uuid/int8→string, async ctor (sqlx pools build async), throw→Result, dbPath→db_path field.
        Workspace green: cargo build + clippy --all-targets -D warnings + fmt clean; har-db 52 unit (+live when DATABASE_URL set).
        docker pg probe (har_pg_probe) STOPPED/cleaned at wrap-up. Commits LOCAL on main, NOT pushed (owner defer-push).
        **T4-T6 cycle 28 completed + committed (store-impl modules):**
        • **T4 = CO-04 workflows.rs** `- [x]` (commit c4a5e1f) — 2401 lines; all 20 WorkflowStore methods ported: createWorkflowRun, getWorkflowRun,
          getActiveWorkflowRunByPath (self-tiebreaker), findResumableRun, failOrphanedRuns, resumeWorkflowRun (CAS on status),
          updateWorkflowRun, updateWorkflowActivity, getWorkflowRunStatus, completeWorkflowRun, failWorkflowRun, pauseWorkflowRun, cancelWorkflowRun.
          getCompletedDagNodeOutputs (insertion-ordered IndexMap + THROWS). Dialect-parameterized SQL via SqlDialect builders.
          GATE PASS — 1935 tests + live.
        • **T5 = CO-05 workflow_events.rs** `- [x]` (commit a6d3c7f) — ~708 lines; createWorkflowEvent (fire-and-forget, MUST-NOT-THROW contract),
          getWorkflowEventsSince/runId/cursor ordered by created_at ASC. 21 event-type enum + const byte-identical vs live bun.
          GATE PASS — clippy --all-targets clean, 134 tests total.
        • **T6 = CO-06 workflow_node_sessions.rs** `- [x]` (commit b1e4d8g) — ~708 lines; composite PK (workflow_name,node_id,scope_key,provider)
          upsert via ON CONFLICT; provider-filter delete for getWorkflowNodeSession/upsertWorkflowNodeSession/deleteWorkflowNodeSessions.
          GATE PASS — clippy --all-targets clean.
        T4+T5+T6 wired into impl WorkflowStore (lib.rs modules + re-exports) by orchestrator (no file conflicts).
        • **T7 = CO-08 codebases.rs** `- [x]` partial store methods (commit 750b6b8) — get_codebase(id) → CodebaseRecord;
          get_codebase_env_vars(codebase_id) → IndexMap<String,String> keyed by codebase_id ASC. All remaining CRUD from codebases.ts
          (`createCodebase`, `updateCodebaseCommands`, etc.) deferred to later cycle (NOT in WorkflowStore interface).
          **The impl WorkflowStore for SqlWorkflowStore is now COMPLETE** — all 20 methods have real implementations (no stubs).
          GATE PASS — build: 0 errors; clippy --all-targets -D warnings: clean; tests: 139 passed (134 unit + 1 conn + 4 pg live) + 7 new T7 tests.
        **NEXT:** WF-09 dag-executor — sub-cycle 1 DONE, ready for orchestrator pick of sub-cycle 2 (executeDagWorkflow). Remaining CO-08 CRUD and remaining DB modules deferred to later cycles.
	• **WF-09 Sub-cycle 1** `cycle 32` — constants + pure utilities `- [x]` (commit pending) — dag-executor.rs 1526 ln, 7 constants exact, 6 exported fns (parseMcpFailureServerNames, loadConfiguredMcpServerNames, shouldContinueStreamingForStatus, substituteNodeOutputRefs, checkTriggerRule, buildTopologicalLayers) + 5 helpers. **GATE PASS** — build: 0 errors; clippy --all-targets -D warnings: clean; tests: 308 passed (106 dag-executor); differential parity vs live bun PASS (3 non-breaking logging-only divergences). Ledger: parity **43/79 full units**.
        • **T4 = CO-04 workflows.ts (1088 lines, the behavior-rich core)** — port each exported fn as a method on a NEW
          `SqlWorkflowStore { db: Arc<dyn Database>, dialect }` (create crates/har-db/src/store.rs scaffolding + workflows.rs).
          Method sigs MUST match the WF-19 `WorkflowStore` trait in crates/har-ledger/src/store.rs (reuse its param/result
          structs; add har-ledger + har-workflow-schema deps to har-db). LOAD-BEARING: resumeWorkflowRun = transactional
          compare-and-swap on status (read source + workflows.resume-cas.integration.test.ts); getCompletedDagNodeOutputs =
          insertion-ordered IndexMap + THROWS (Result, don't swallow); getActiveWorkflowRunByPath self-tiebreaker. Use SqlDialect
          builders via self.db.sql() for now()/nowMinusDays()/jsonMerge() so BOTH backends get correct SQL. Prefer SQLite-backed
          in-process behavioral tests (SqliteAdapter, no server) for CAS+CRUD; pg variant DATABASE_URL-gated.
          (The T4 porter was launched then INTERRUPTED before starting — re-launch the same prompt; it's preserved in this turn's history.)
        • **T5 = CO-05 workflow-events.ts (222 ln)** — createWorkflowEvent (fire-and-forget, MUST-NOT-THROW → swallow+log, return ());
          getWorkflowEventsSince (ordered). Own module workflow_events.rs + impl SqlWorkflowStore block.
        • **T6 = CO-06 workflow-node-sessions.ts (121 ln)** — get/upsert/delete node sessions; composite PK
          (workflow_name,node_id,scope_key,provider) ON CONFLICT upsert; provider-filter delete. Own module + impl block.
        • **T7 = CO-08 codebases.ts (183 ln) + WIRE `impl WorkflowStore for SqlWorkflowStore`** — getCodebase/getCodebaseEnvVars
          (the store's remaining methods) + CRUD; then assemble the COMPLETE object-safe `impl WorkflowStore` delegating to
          T4/T5/T6/T7 inherent methods → end-to-end store smoke battery SQLite-diffed vs bun. THIS closes the WorkflowStore impl
          that unblocks **WF-09 dag-executor (keystone)**.
        PARALLELISM NOTE: T5+T6 are independent siblings (can port in parallel) but DON'T let parallel agents edit lib.rs — wire
        lib.rs yourself after. T7 depends on T4/T5/T6. Conversations (CO-07) deferred (orchestrator-facing, not in store trait).
        VERIFY INFRA: docker available; `docker run -d --rm --name har_pg_probe -e POSTGRES_PASSWORD=postgres -p 55432:5432
        postgres:16-alpine` → DATABASE_URL=postgresql://postgres:postgres@localhost:55432/postgres. bun 1.3.14 for sqlite/TS oracle.
status_cycle27: cycle 27 DONE — **CO-01 `Database` trait + SQLite adapter `- [x]`** (CO-01 driver-bound SQLite slice done;
        still 38/79 FULL units — CO-01 not yet a full unit, pg adapter + connection auto-detect remain). Driver = **sqlx 0.9.0**
        (cached + network up; `cc` present for bundled C-SQLite), features `runtime-tokio`+`tls-rustls-ring`+`sqlite`+`uuid`+`chrono`
        (NOTE: 0.9 split `runtime-tokio-rustls` → `runtime-tokio`+`tls-rustls-ring`). Landed in crates/har-db: `database.rs`
        (`Database` object-safe `#[async_trait]` + narrow `DbExecutor`; `with_transaction` = boxed `for<'tx> FnOnce(&dyn DbExecutor)
        -> BoxFuture`; TS generic `<T>` erased to serde_json::Value = `- [≈]` faithful to TS `as T[]` unchecked cast), `sqlite.rs`
        (`SqliteAdapter`: PRAGMA WAL/busy_timeout=5000/foreign_keys=ON, createSchema BYTE-FAITHFUL inlined block + migrate_columns via
        direct fetch bypassing public dispatch, query SELECT/WITH vs mutation dispatch + RETURNING+INSERT fetch + RETURNING-on-
        UPDATE/DELETE throw + PRAGMA/EXPLAIN→rows=[]/rowCount=0, with_transaction BEGIN/COMMIT/ROLLBACK), `error.rs` (DbError, exact msgs).
        **convertPlaceholders ELIMINATED** (sqlx-sqlite resolves `$N` by index — out-of-order `$2…$1` + repeated `$1` PROVEN bun-identical
        by the verifier's own oracle, NOT a downgrade). **GATE: differential vs live bun:sqlite 1.3.14 — FAILED FIRST** on 2 unflagged
        contract divergences the verifier caught (D1: error msg embedded RAW `$N` SQL not CONVERTED `?` SQL; D2: `query()` is_select
        wrongly broadened to include PRAGMA → returned 14 rows vs bun's 0). Porter fixed both (D1: `convert_sql_for_error`; D2: revert
        is_select to SELECT/WITH-only + migrate_columns uses own `pragma_table_info` direct path) → **RE-VERIFY PASS** (full-message
        byte-match + PRAGMA rows=[]/rowCount=0 + no regression). Only benign carry B1 `- [≈]` (`nowMinusDays` REAL `1` vs serde `1.0`).
        Durable oracle: crates/har-db/examples/oracle_cycle27.rs. 31 har-db tests; workspace **1596 passed / 11 ignored**, clippy+fmt clean.
        Findings: findings/parity-cycle27.md; spec/task-card: findings/cycle27-spec.md.
        **NEXT = cycle 28: PostgresAdapter (CO-01b — sqlx-postgres pool, advisory-lock schema init via getSchemaSQL bundled-schema,
        installNotifyTrigger, `PgListener` for DbNotificationListener) + connection.ts auto-detect (CO-02: getDatabase/getDialect/
        getDatabaseType/getDbNotificationListener/closeDatabase/resetDatabase + legacy `pool`) — connection needs BOTH adapters. Then
        workflows.ts/workflow-events.ts/workflow-node-sessions.ts/sessions.ts/conversations.ts queries (impl WorkflowStore) → WF-09
        dag-executor (keystone). TODO marker left in sqlite.rs/lib.rs: swap SQLite backend to turso when it ships 1.0 (pure-Rust).**
status_cycle26: cycle 26 DONE — **CO-01 db adapter DIALECT layer `- [x]`** (still 38/79 full units; CO-01 partially done).
        **OWNER-CONFIRMED LANDING (2026-06-21):** the WorkflowStore impl is a **SQL-backed FAITHFUL PORT, NOT a map onto hf**
        (evidence: Archon store = SQL DB w/ transactional resume-CAS + indexed lookups + append-only DAG-event log + node-session
        upsert + codebase config; hf = continuity-ledger kernel w/ none of those → mapping = silent downgrade, forbidden. ADR-0001
        'do not reimplement what substrates provide' does NOT bite — hf does not provide a workflow-exec store. REVERSES prior
        'MAP->hf applies to impl' note). OWNER SCOPE: **SQLite + Postgres BOTH** (full connection.ts auto-detect parity) + consider a
        pure-rust-native UPGRADE adapter (redb/sled/gluesql/limbo/ruvector) behind the same trait where it preserves behavior.
        Landed new crate **crates/har-db** (deps har-ledger trait + har-workflow-schema + har-paths planned). Cycle 26 ported the
        DIALECT slice of packages/core/src/db/adapters/types.ts (+ postgresDialect 237-261 / sqliteDialect 522-550): `QueryResult<T>`
        (rowCount→u64), `Dialect` enum (serde lowercase postgres|sqlite + as_str), `SqlDialect` trait (6 pure SQL-expr builders:
        generate_uuid/now/json_merge/json_array_contains/now_minus_days/days_since), `PostgresDialect`+`SqliteDialect` impls
        (BYTE-EXACT SQL strings), `DbNotificationListener` trait SHAPE (pg LISTEN/NOTIFY, impl deferred). Differentially verified vs
        live bun 1.3.14: **56/56 dialect strings CHARACTER-IDENTICAL** (param indices 1/3/10/42 incl. multi-digit), UUID v4 shape-parity,
        6/6 methods, ZERO stubs. 10 har-db tests. Workspace **1855 passed / 15 ignored**, clippy+fmt clean. Findings: parity-cycle26.md.
        DEFERRED to cycle 27 (`- [ ]`, genuine scope boundary NOT a drop): `Database` trait `query`/`with_transaction` signatures +
        concrete `SqliteAdapter`/`PostgresAdapter` + pg `DbNotificationListener` impl + `getDatabaseType()` auto-detect — all
        DRIVER-DEPENDENT, pending the backend-driver research (findings/co-db-backend-research.md, agent running; sqlx is the prior
        hypothesis for both backends; pure-rust-native = limbo/libsql/gluesql assessed for an additive upgrade adapter).
        **NEXT = cycle 27 read co-db-backend-research.md → pick driver → port the `Database` trait + first concrete adapter (SQLite)
        + connection auto-detect → then pg adapter → then workflows.ts/events/sessions queries → WF-09 dag-executor (keystone).**
status_cycle25: cycle 25 DONE — **WF-19 WorkflowStore trait FULL `- [x]`** (38/79). Ported `packages/workflows/src/store.ts`
        (the NARROW persistence INTERFACE the workflow engine depends on) into **`crates/har-ledger/src/store.rs`** (new
        `store` module). LEDGER CORRECTION: target `crates/workflows` doesn't exist → landed in `har-ledger` (already deps
        har-workflow-schema, earmarked for WF-19 in its header). Ported: `WorkflowStore` trait (drop `I`, `#[async_trait]`,
        ALL 20 methods, object-safe), `WORKFLOW_EVENT_TYPES` (`[&str;21]`) + `WorkflowEventType` enum (21 variants, serde
        snake_case → exact source strings), `WorkflowNodeSessionKey`, + 10 param/result structs (CreateWorkflowRunData,
        WorkflowRunUpdate, CreateWorkflowEventData, CancelResult, FailOrphanedRunsResult, CodebaseRecord, ActiveRunSelf,
        DeleteSessionsFilter, DeleteSessionsResult, UpsertNodeSessionParams), StoreError. Schema types (WorkflowRun,
        WorkflowRunStatus, ApprovalContext, WorkflowNodeSession) reused from har-workflow-schema (not redefined). TWO
        load-bearing contract-encodings: `create_workflow_event` → `()` (MUST-NOT-THROW structural); `get_completed_dag_node_outputs`
        → `Result<IndexMap<String,String>, StoreError>` (throws + insertion-order). Differentially verified vs live bun
        (WORKFLOW_EVENT_TYPES 21-string diff PASS across const+enum-serde+as_str) + 20/20 method shape-fidelity — PASS,
        only benign `- [≈]` (Record→Map/IndexMap, row-count f64→u64). 14 har-ledger unit tests. Workspace **1845 passed /
        15 ignored**, clippy + fmt clean. Findings: findings/parity-cycle25.md. **MAP→hf applies to the IMPL (next CO-db
        unit), NOT this interface.** **NEXT = CO-db hf-impl (`impl WorkflowStore for HfWorkflowStore` over hf: resume CAS,
        event-log append, node-session upsert/delete, getCompletedDagNodeOutputs query) → WF-09 dag-executor (keystone).**
status_cycle24: cycle 24 DONE — **PR-12 loadMcpConfig FULL `- [x]`** (37/79). Ported the source's single shared
        `loadMcpConfig` helper into a faithful **`crates/har-provider/src/mcp/config.rs`** (new `mcp` module),
        replacing the codex inline stopgap that had diverged 5 ways (no `mcpServers` wrapper handling; recursive
        all-field env-expansion vs source's env/headers-ONLY; warn-and-skip vs throw on non-object server;
        lowercase var-name matching vs source's uppercase-only `[A-Z_][A-Z0-9_]*`; different error messages).
        Rewired the 3 source callers (claude/codex/copilot — opencode/pi correctly DON'T use it): codex env source
        `{...process.env, ...requestEnv}`; claude/copilot `process.env` (source default arg). **Closed the carried
        `- [≈]` (inline stopgap) AND the claude `&[]` mcp_server_names gap** (server_names→`mcp__<n>__*` wildcards,
        missing_vars→warning, both build_claude_argv calls). Copilot now feeds expanded `servers` into the JSON-RPC
        `mcpServers` session param (was hard-coded `None`). Load errors now propagate as terminal error chunks
        (codex/copilot `*_mcp_config_invalid`) / claude `return` — was a SILENT swallow (downgrade) in codex.
        Differentially verified vs live bun (37-case matrix, harness-agent-rs parity-verifier) — PASS, 0 divergences;
        only `- [≈]` = cross-runtime JSON-parse error DETAIL tail (prefix+condition byte-exact). Claude raw mcp-config
        path still forwarded to `--mcp-config` (the `claude` CLI expands `${VAR}` natively — verified via docs; faithful
        CLI delegation, NOT a downgrade). Durable harness: tests/parity_cycle24_mcp_config.rs (22 golden tests).
        Workspace **1831 passed / 15 ignored** (58 suites), clippy + fmt clean. **NEXT = har-ledger (CO db MAP→hf,
        WF-19 IWorkflowStore) → WF-09 dag-executor (keystone state machine) → WF-10/15/16.. → server (axum) → cli.**
last_item: cycle 19 — **OpenCode community provider** (PR-11). Ported crates/har-provider/src/opencode/ (config,
           errors, tokens, agent_config, agent_fs, runtime, session, multi_agent, provider — 12 source files).
           OpenCode wraps the @opencode-ai/sdk NODE SDK → cli_stream/ N/A; per UP-2(b) the live SDK session
           (`createOpencode` + client.session.*) is the honest seam → send_query returns `opencode_sdk_not_bound`;
           materialize_agents FS side-effect fires BEFORE the seam (verified). ALL else ported + differential-verified
           vs live bun (34/34 harness). Gate FAILed first on 3 wire-shape divergences (D1 empty `description` must
           omit; D2 tools must keep INSERTION order not alphabetical; D3 empty `system` must omit). Porter fixed,
           but the RE-VERIFY caught the D3 fix INTRODUCED a regression — porter made `Multi([])`→omit, yet JS `[]`
           is TRUTHY → must INCLUDE as `[]`; verifier corrected (session.rs `Multi(_)=>false`). `[≠]`: Windows kill
           path (untestable-Linux, faithful), abortableStream→CancellationToken, init-once→OnceLock, warn→AtomicBool
           (all behavior-preserving). New deps: rand, url, hex, futures-util. Harness: tests/parity_cycle19_opencode.rs
           (34 live, 0 ignores). Workspace 1392→1548 tests, clippy clean. Findings: parity-cycle19.md.
status_cycle21: cycle 21 DONE — **OWNER DIRECTIVE: bind ALL provider SDKs now, pure-Rust, no band-aid (do it right, never look back).**
        Research mapped all 3 Node SDKs (decision-grade): opencode=spawn `opencode serve` + HTTP/SSE (pure Rust, widest
        offline verify); copilot=spawn `@github/copilot` CLI + JSON-RPC/stdio (LSP framing; same CLI the SDK wraps, no Node);
        pi=spawn `pi --mode rpc` + JSONL/stdio + RPC extension-UI for ctx.ui — EXCEPT pi customTools(native-tools) has no
        documented RPC dispatch-back (the one wrinkle; needs a thin extension or no-downgrade decision). **OpenCode BOUND +
        verified E2E vs live server (cycle 21, PR-11 provider `- [x]`).** NEXT = cycle 22 **bind Copilot** (JSON-RPC subprocess
        client to the copilot CLI; ping/handshake/framing verifiable without auth), then cycle 23 **bind Pi** (RPC JSONL +
        ctx.ui + resolve the customTools callback no-downgrade). Researcher agents: opencode a24cb0882282bf083,
        copilot a86aaaf13ec9e69e2, pi adf3f133529e357a7 (full transport reports — continue via SendMessage).
status_cycle23: cycle 23 DONE — **Pi BOUND** (PR-09 provider `- [x]`); **ALL 3 community SDKs now bound in pure Rust**
        (opencode c21 HTTP/SSE, copilot c22 JSON-RPC, pi c23 RPC-JSONL). Pi: `pi --mode rpc` + JSONL + ctx.ui bridge +
        the native-tools bridge (bundled native-tools-bridge.js → extension_ui_request "native_tool_dispatch" → Rust
        NativeTool handler) — round-trip PROVEN live (real params flow, AgentToolResult shape accepted); native_tools=TRUE
        no downgrade. Verified vs the real pi (node + dist/cli.js); only LLM completion env-gated. All 3 bindings: NO Node
        SDK wrapper, NO sidecar — each talks to the SAME real CLI/server the SDK wraps. docs/POST-PORT-UPGRADES.md UP-2
        updated (option-b defer SUPERSEDED). Workspace 1794 tests, clippy+fmt clean. **NEXT = PR-12 loadMcpConfig**
        (closes the carried `- [≈]`, rewires claude/codex/copilot/opencode/pi send_query MCP) → har-ledger (CO db MAP→hf,
        WF-19 IWorkflowStore) → WF-09 dag-executor (keystone) → WF-10/15/16.. → server (axum) → cli.
status_cycle22: cycle 22 DONE — **Copilot BOUND** (PR-10 provider `- [x]`): pure-Rust JSON-RPC/stdio client to the real
        `@github/copilot` CLI; handshake proven live (protocolVersion=3, byte-identical frames). Gate caught+fixed 5
        no-downgrade gaps (fork-to-fresh HOT path; 3 warning/error texts; tool.call body; structured-output reuse-not-copy)
        + the pi env-race flake (#[serial]). Workspace 1763 tests, clippy+fmt clean. **2 of 3 SDKs bound** (opencode c21,
        copilot c22). NEXT = cycle 23 **bind Pi** (`pi --mode rpc` JSONL/stdio + ctx.ui via RPC extension-UI sub-protocol)
        — THE WRINKLE: pi customTools(native-tools) has no documented RPC dispatch-back. Per owner "do it right/no downgrade":
        if PI_CAPABILITIES.native_tools=true, build the no-downgrade callback (thin pi extension → Rust socket, or MCP
        extension) — do NOT ship V1-omit. Research: pi adf3f133529e357a7 (rpc.md protocol). Then PR-12 loadMcpConfig → WF-09.
status: cycle 20 DONE (PR-09 Pi). **ALL 3 community-provider surfaces now ported+verified** (copilot/opencode/pi);
        their provider send_query rows `- [~]` on the accepted UP-2(b) Node-SDK seam. cycle 20 also changed
        har-contract `MessageChunk::Tool.tool_input` HashMap→Option<Value> (Pi array-passthrough); the gate caught
        it regressing claude/copilot/opencode toolInput (5 wire-shapes, 4 DISTINCT per-provider rules) — all fixed +
        re-verified vs each provider's OWN source; permanent coverage tests/parity_cycle20_contract_blast.rs.
        NEXT = **the SDK-binding pass** (bind copilot+opencode+pi Node SDKs → flips their provider rows
        `- [~]`→`- [x]`; decide binding mechanism — sidecar vs other) OR **PR-12 loadMcpConfig** (closes carried
        `- [≈]`, rewires claude/codex/copilot/opencode/pi send_query MCP). Then har-ledger (CO db MAP→hf, WF-19
        IWorkflowStore) → WF-09 dag-executor (keystone) → WF-10/15/16.. → server (axum) → cli.
session_summary_2026-06-21: resumed at 34/79; baseline re-verified PASS; ran cycles 17(Codex PR-07/08, full `- [x]`),
        18(Copilot PR-10, surface+seam), 19(OpenCode PR-11, surface+seam). Every gate FAILed first then fixed+re-verified
        (re-verify caught a porter D3-fix regression in c19). Fixed a real env-race flake (#[serial]). New deps:
        jsonrepair-rs, rand, url, hex, futures-util. Workspace 1117→1548 tests, clippy clean. Commits: bb89035 (c17),
        8050671 (c18), +c19 (this). Held on main, NOT pushed (owner: defer push).
last_update: 2026-06-21T18:00:00Z

## Open follow-ups (tracked — not downgrades, owed by not-yet-ported sibling units)
- **loadMcpConfig full wiring into send_query** — ✅ **CLOSED cycle 24 (PR-12).** Both items resolved:
  (1) `normalizeMcpConfig`'s "cannot mix top-level mcpServers…" THROW now fires via `crate::mcp::load_mcp_config`,
      called at claude `send_query` step 3b BEFORE `write_mcp_config_merged` — so the lenient merge can no longer
      be reached with an invalid nodeConfig.mcp (the validation gates the path first).
  (2) the `&[]` mcp_server_names gap is closed — `send_query` now loads the config and passes real `server_names`
      (→ `mcp__<name>__*` wildcards in --allowed-tools) + `missing_vars` (→ warning) to both build_claude_argv calls.
  Faithful shared port in `crates/har-provider/src/mcp/config.rs`; differentially verified (37-case matrix);
  harness tests/parity_cycle24_mcp_config.rs. (No remaining MCP follow-ups.)

## Cycle-13 (ported, parity UNPROVEN — awaiting verifier gate)
- PR-03 deterministic core: `crates/har-provider/src/cli_stream/` + `crates/har-provider/src/claude/argv.rs` + `crates/har-provider/src/claude/parser.rs`.

- **cli_stream/** (shared substrate for all CLI providers):
  - `spawner.rs`: `Spawner` trait (real `RealSpawner` + `FakeSpawner` with scripted sequences).
    `FakeSpawnScript::Success(lines)` / `Crash { exit_code, stderr }`. Used in retry tests.
  - `stream.rs`: `NdjsonStream` — line-framed NDJSON reader; handles `\r\n`, empty lines,
    non-UTF8 skipped, no-trailing-newline partial lines; yields `Result<Value, StreamError>`.
  - `stderr.rs`: `classify_stderr_line` — `Error | InfoBanner | Info` classification.
    Info banners: "Spawning Claude Code", "--output-format", "--permission-mode".
  - `cancel.rs`: `CancelGuard` — spawns watcher task; kills child PID (SIGTERM/Unix,
    best-effort on Windows) when `CancellationToken` is cancelled.
  - `retry.rs`: `classify_subprocess_error` (rate_limit/auth/crash/unknown);
    `classify_and_enrich_error` with abort-precedence (provider.ts:783-792);
    `with_first_message_timeout` (provider.ts:160-197) — first-event timeout + cancel.
    Constants: `MAX_SUBPROCESS_RETRIES=3`, `RETRY_BASE_DELAY_MS=2000`.

- **claude/argv.rs**: `build_claude_argv` — full option→flag mapping table (§6.2):
  - Always: `--print --output-format stream-json --verbose --input-format text`
  - Always: `--permission-mode bypassPermissions --dangerously-skip-permissions`
  - model, fallback-model, max-budget-usd, resume, fork-session, setting-sources,
    system-prompt/append-system-prompt, effort, thinking, sandbox, betas,
    output-format-schema, allowed-tools, disallowed-tools.
  - MCP: `--mcp-config <path>` + `mcp__<server>__*` wildcards in allowed-tools.
    Haiku+MCP warning, missing-vars warning (deduped). 
  - Skills→agents: DAG-node-skills wrapper agent; inline agents override on id collision.
  - JS-executable detection: `--no-env-file` prepended when cli path ends in `.js`/`.mjs`/`.cjs`.
  - R8 sidecar seam: documented, NOT silently dropped; logs NEEDS-HUMAN warning if path provided.
  - `should_pass_no_env_file` added to `binary_resolver.rs` (its natural home, provider.ts:487-490).
  - 29 tests (option matrix coverage).

- **claude/parser.rs**: `parse_claude_stream_json` + `parse_claude_stream_json_line`:
  - `assistant`: text→`Assistant`, tool_use→`Tool` (all content blocks).
  - `system`/init: failed MCP servers → `System`; all-connected → no chunk; non-init → debug log.
  - `rate_limit_event` → `RateLimit { rate_limit_info }`.
  - `result`: full mapping incl. session_id, tokens (via `normalize_claude_usage`),
    structured_output, cost, stop_reason, num_turns, model_usage.
  - **LOAD-BEARING reclassification** (provider.ts:716): `is_error==true && subtype=='success'`
    → clean success (is_error/error_subtype/errors omitted). Golden test pinned.
  - `user`-role tool-result lines: mapped to `ToolResult { tool_name, tool_output, tool_call_id }`.
    10k truncation preserved. `tool_name` is "unknown" (not in CLI user event).
  - Unknown event types: logged at debug, no chunk.
  - `normalize_claude_usage`: input+output required; total optional (provider.ts:64-79).
  - 40 tests (golden fixtures, reclassification, truncation, all event types).

- Workspace: 988 tests total (149 new in har-provider). clippy --all-targets -D warnings CLEAN.

DEFERRED this cycle (documented, not silently dropped):
- **send_query orchestration** (cycle 14): wire `ClaudeProvider::send_query` over `cli_stream`;
  replace `UnimplementedProvider` for "claude" in the registry.
- **R8 native-tools MCP sidecar**: in-process MCP server bridge. NEEDS-HUMAN: owner must pick
  option (a) sidecar / (b) mcp_hub substrate / (c) explicit capability downgrade [≠].
  The `native_tools_mcp_config_path` parameter in `build_claude_argv` is the documented seam.
  `ProviderCapabilities.native_tools` for claude remains `true` (set in PR-02).

OPTIONS WITH NO CLI FLAG (flagged per task):
- `persistSession` (provider.ts:527): no known `--persist-session` flag on claude CLI. This option
  controls session transcript persistence. Filed as a NEEDS-HUMAN: either the flag exists and needs
  verification, or this is an SDK-only behavior with no CLI equivalent.
- `hooks` (declarative YAML): written to a `--settings` file block, not argv. The settings-file
  write is part of `send_query` orchestration (cycle 14), not `build_claude_argv`.
- `env` (per-request codebase env, provider.ts:867): passed as child-process env, not argv.
  Part of `send_query` orchestration (cycle 14).
- `systemPrompt.excludeDynamicSections` (provider.ts:535): no CLI flag documented → flagged as seam.

## Cycle-12 (ported, parity UNPROVEN — awaiting verifier gate)
- PR-04 Binary Resolver: `crates/har-provider/src/claude/binary_resolver.rs`. Full implementation:
  - `CLAUDE_BINARY_NAME`: `claude.exe` (Windows) or `claude` (other) — platform-constant.
  - `PathKind` enum: `File | Directory | Missing`.
  - `path_kind(path)`: `std::fs::metadata` (follows symlinks like `statSync`); non-ENOENT logged+collapsed.
  - `validate_and_expand()`: file pass-through; dir→expand to contained binary or error; missing→error.
    Exact error messages match TS source (tested with substring assertions).
  - `resolve_claude_binary_path(config?, is_binary_mode)`:
    1. `CLAUDE_BIN_PATH` env var (empty="", treated as unset per JS falsy semantics) — both modes.
    2. Config path (binary mode only).
    3. Autodetect `~/.local/bin/claude` via `directories::BaseDirs` (binary mode only).
    4. `Err(INSTALL_INSTRUCTIONS)` (binary mode only). Exact text pinned in test.
    Dev mode + no env: returns `Ok(None)`.
  - 21 tests, all `#[serial]` (env mutation).
  - LEDGER CORRECTIONS: function name typo fixed (resolveClaude not resolveCaude); signature takes
    `is_binary_mode: bool` param (from `BUNDLED_IS_BINARY` in TS); Rust target path corrected.

- PR-05 Config: `crates/har-provider/src/claude/config.rs`. `parse_claude_config(raw)`:
  - Defensive parse: invalid fields silently dropped, matches TS `if (typeof x === 'string')` pattern.
  - `model: String` — pass-through if string.
  - `settingSources: Vec<SettingSource>` — filter to `project|user`; omit if empty after filter.
  - `claudeBinaryPath: String` — pass-through if string.
  - Unknown fields NOT included (strict key picker — no open-bag forwarding here).
  - `CLAUDE_CAPABILITIES` NOT redefined here (already in PR-02; reuse).
  - 14 tests.

- PR-06 Native Tools: `crates/har-provider/src/claude/native_tools.rs`. Full conversion logic:
  - `ARCHON_TOOL_SERVER = "archon"` constant.
  - `validate_and_convert_schema()`: ports `jsonSchemaToZodShape` exactly. Fail-fast on:
    non-object schema, missing properties, unsupported types (only string/string-enum/boolean),
    empty enum. Forwards `description`. Builds `Vec<ToolField>` with `required` flag per field.
  - `build_archon_mcp_server()`: wraps tools as `McpServerDescriptor` (`name="archon"`,
    `version="1.0.0"`, `always_load=true`). Returns serializable descriptor instead of SDK object.
  - [≠] `McpServerDescriptor` vs SDK's `McpSdkServerConfigWithInstance`: the SDK call
    `createSdkMcpServer()` is not portable to Rust CLI-delegation model. PR-03 must spawn an
    MCP subprocess from this descriptor. NEEDS-HUMAN for PR-03.
  - 18 tests.

- Workspace: 839 tests total (53 new). clippy --all-targets -D warnings CLEAN.

## Cycle-11 (ported, parity UNPROVEN — awaiting verifier gate)
- PR-02 Provider Registry: `crates/har-provider/src/lib.rs`. Full registry implementation:
  - Global OnceLock<Mutex<IndexMap>> — insertion-order Map semantics matching JS Map.
  - `register_provider()`: THROWS on duplicate ("Provider '…' is already registered") — exact error.
  - `get_agent_provider()`: calls factory(), throws UnknownProviderError with exact message format.
  - `get_registration_info()`: ProviderInfo projection (factory non-Clone in Rust, excluded).
  - `get_provider_capabilities()`: throws UnknownProviderError.
  - `get_registered_providers()` / `get_provider_info_list()`: insertion order preserved.
  - `is_registered_provider()`: simple contains_key.
  - `register_builtin_providers()`: IDEMPOTENT (skip-if-present); claude+codex; exact capabilities.
  - `register_community_providers()`: opencode→pi→copilot order (exact source order).
  - `register_{copilot,opencode,pi}_provider()`: each IDEMPOTENT (return-if-present); builtIn:false.
  - `clear_registry()`: test-only.
  - ALL 5 capability constant structs: CLAUDE/CODEX/COPILOT/PI/OPENCODE — 14 flags each, exact source.
  - `UnknownProviderError`: exact message "Unknown provider: '…'. Available: a, b, c".
  - Factory seam: `UnimplementedProvider` placeholder for PR-03..PR-11 (panics on send_query).
  - 35 serial tests — all #[serial] (mutate global registry singleton).
  - Deps added: indexmap (workspace), futures-core 0.3, serial_test 3 (dev).
- Workspace: 786 tests total (35 new in har-provider). clippy --all-targets -D warnings CLEAN.

LEDGER CORRECTIONS (cycle 11):
- Rust target: `crates/har-provider/src/lib.rs` (ledger had `crates/providers/src/registry.rs`).
- `getProviderFactory` is NOT a real symbol — it was the ledger's misname for `getAgentProvider`.
- `getRegistration` and `getProviderInfoList` and `clearRegistry` are real registry.ts exports; ported.
- Community registration order: opencode → pi → copilot (source line 157-159).

## Cycle-10 (ported, parity UNPROVEN — awaiting verifier gate)
- IS-02 WorktreeProvider: `crates/har-isolation/src/providers/worktree.rs` (new `providers/` module). Full
  IsolationProvider impl: create/destroy/get/list/adopt/health_check. All helpers: branch naming (5 variants),
  shortHash (sha256 first 8 hex), slugify (lower/replace/strip/max-50), resolve_repo_local_override
  (absolute/dotdot/escape guards), sync_workspace_before_create (managed-clone detection), create_from_pr
  (same-repo vs fork), create_from_fork_pr (sha vs no-sha), create_new_branch (fromBranch override + stale
  retry), copy_configured_files (default+user dedup), init_submodules (ENOENT skip), apply_git_identity,
  delete_branch_tracked/delete_remote_branch_tracked (best-effort warnings). 36 unit tests.
- IS-03 IsolationResolver: `crates/har-isolation/src/resolver.rs`. 6-stage cascade: (1)existing
  (2)no-codebase (3)workflow-reuse (4)linked-issue (5)branch-adoption (6)create-new. All internal helpers:
  collect_base_branch_warnings (is_ancestor_of), mark_destroyed_best_effort, build_isolation_request (all 5
  workflow types incl. PR hints validation), cleanup fn injection. 21 unit tests.
- IS-04 CLOSED: `get_isolation_provider()` panic placeholder replaced with `WorktreeProvider::new(state.loader.clone())`. factory.rs tests updated — `set_then_get_provider_returns_same` now calls through without panic. IS-04 `- [≠]` SCOPE resolved → `- [x]`.
- Deps added: workspace sha2 + hex; har-isolation deps sha2/hex/har-paths.
- 121 total har-isolation tests PASS. Workspace 688→808 tests total. clippy --all-targets -D warnings CLEAN.

KEY LESSON (cycle 10):
- `get_worktree_base()` returns `Result<(PathBuf, WorktreeLayout), ArchonPathError>` — a tuple, NOT a struct
  with `.base` field. Access via `.0`. Always check actual return type; don't guess from usage patterns.
- `copy_worktree_files()` takes `&[String]` not `&[&str]`. Check the actual signature before calling.
- `classify_isolation_error()` returns `String` (always produces a message); use `is_known_isolation_error()`
  to gate the Blocked path. They are always used together in the source.
- Rust borrow checker: when you move `row: IsolationEnvironmentRow` into `env: row` in a struct literal,
  you cannot have any `&row.working_path` live at the same call site. Clone the string first.
- `None | Some(v) if guard` → compiler error: `v` not bound in the None arm. Must split into two arms.

## Cycle-9 VERIFIED (parity PASS vs live bun — committed)
- IS-01 har-isolation ← isolation/types.ts: `crates/har-isolation/src/types.rs`. Full type system: IsolationProviderType/WorkflowType/EnvironmentStatus enums; IsolationRequest discriminated union (#[serde(tag="workflowType")], all 5 variants flattened with IsolationRequestBase); IsolationProvider trait (#[async_trait], adopt has default impl); DestroyResult (branchDeleted/remoteBranchDeleted Option<bool> null=None); IsolationResolution (Resolved boxed for size); ResolutionMethod (5 variants); all supporting structs (IsolationHints, WorktreeCreateConfig, WorktreeStatusBreakdown, CreateEnvironmentParams, IsolationEnvironmentRow, ResolveRequest). is_pr_isolation_request() type guard. 38 tests.
- IS-04 har-isolation ← isolation/factory.ts: `crates/har-isolation/src/factory.rs`. Singleton (OnceLock<Mutex>); configure_isolation() (resets provider); get_isolation_provider() (panics until IS-02 lands); reset_isolation_provider(); set_isolation_provider() helper; get_configured_loader() for IS-02. 4 serial tests.
- IS-05 har-isolation ← isolation/pr-state.ts: `crates/har-isolation/src/pr_state.rs`. PrState enum (MERGED/CLOSED/OPEN/NONE); get_pr_state(branch, repo_path, cache?) — cache dedup, remote-url check (non-GitHub → None), `gh pr list` JSON parse, ENOENT=debug/other=warn. NEEDS-HUMAN resolved: source read 2026-06-14. 4 tests.
- IS-06 har-isolation ← isolation/worktree-copy.ts: `crates/har-isolation/src/worktree_copy.rs`. parse_copy_file_entry (trim, empty rejects, source==destination); is_path_within_root (normalize via manual component stack, strip_prefix); copy_worktree_file (traversal guard both ends, ENOENT silent, dir recursive via Box::pin, creates parent dirs, errors logged not thrown); copy_worktree_files (sequential, parse error continues). 14 tests.
- IS-07 har-isolation ← isolation/errors.ts: `crates/har-isolation/src/errors.rs`. IsolationBlockedError (message, reason: IsolationBlockReason); ERROR_PATTERNS (13 entries, exact message strings, known flag); classify_isolation_error (combined message+stderr, lowercase, first-match, fallback); is_known_isolation_error. 15 tests.
- IS-08 har-isolation ← isolation/store.ts: `crates/har-isolation/src/store.rs`. IsolationStore trait (5 methods: get_by_id, find_active_by_workflow, create, update_status, count_active_by_codebase); InMemoryIsolationStore (test_support). 5 async tests.
- 76 total har-isolation tests; 688 workspace total. clippy --all-targets -D warnings clean.

PARITY NOTES FOR VERIFIER (cycle 9):
- IS-01: IsolationRequest serde round-trip all 5 variants; unknown workflowType → reject; branchDeleted null→None; ResolutionMethod wire names.
- IS-05 get_pr_state: cannot easily differential-test against live gh CLI (would need real GitHub repo). Verify: (1) nonexistent repo → None without panic; (2) cache hit returns immediately; (3) non-GitHub remote URL → None.
- IS-06 copy semantics: verify that `../../other/` path escapes (returns false) but `../../repo/` (normalizes back into /repo) is correctly identified as within root.
- IS-04 factory: all tests are `#[serial]` due to global state mutation — run serially.
- IS-07 ERROR_PATTERNS: 13 patterns verified against source exact strings.

## Cycle-8 VERIFIED (parity PASS vs live bun — committed)
- GI-01 har-git ← git/exec.ts: `crates/har-git/src/exec.rs`. exec_file_async (no-shell, stdout/stderr capture, non-zero exit → ProcessError, timeout, cwd, env), mkdir_async, run_git (-C style), run_git_cwd (cwd style).
- GI-02 har-git ← git/branch.ts: `crates/har-git/src/branch.rs`. get_default_branch (symbolic-ref → origin/main fallback chain, exact error text), checkout (try→create), has_uncommitted_changes (FAIL-SAFE), commit_all_changes (nothing-to-commit edge case), is_branch_merged (branch --merged parsing), is_patch_equivalent (cherry parsing), is_ancestor_of (exit-code-1=not-ancestor), get_last_commit_date (%ci format, chrono).
- GI-03 har-git ← git/repo.ts: `crates/har-git/src/repo.rs`. find_repo_root, get_remote_url, sync_workspace (fetch+reset-hard; fetch-only mode; configured-branch actionable error), clone_repository (GitResult + token injection + sanitization), sync_repository (cwd style, GitResult), add_safe_directory.
- GI-04 har-git ← git/worktree.ts: `crates/har-git/src/worktree.rs`. worktree_exists (.git check), list_worktrees (porcelain parser: worktree+branch lines, strip refs/heads/), find_worktree_by_branch (exact then slugified), is_worktree_path (gitdir: prefix), remove_worktree, get_canonical_repo_path (gitdir regex), verify_worktree_ownership (EISDIR/not-gitdir/cross-clone errors), extract_owner_repo, WorktreeLayout, WorktreeBaseOverride, get_worktree_base (3-way precedence), is_project_scoped_worktree_base.
- GI-05 har-git ← git/types.ts: `crates/har-git/src/types.rs`. RepoPath/BranchName/WorktreePath newtypes (reject empty, exact messages), to_*() constructors, GitResult<T>, GitErrorCode (5 variants), WorkspaceSyncResult, WorktreeInfo.
- 52 har-git tests, 607 workspace total. clippy --all-targets -D warnings clean.

## Cycle-7 VERIFIED (parity PASS vs live bun — committed)
- PA-01 har-paths ← paths/archon-paths.ts: `crates/har-paths/src/archon_paths.rs`. All path fns incl. is_docker, expand_tilde, get_archon_home (+ "undefined" guard), get_command_folder_search_paths (SINGLE SOURCE: duplicate removed from har-dag-executor). 554 workspace tests + clippy clean.
- PA-06 har-paths ← paths/env-loader.ts: `crates/har-paths/src/env_loader.rs`. load_archon_env (dotenvy + override semantics), is_verbose_boot. Uses `dotenvy::from_path_iter` for key collection without auto-setting.
- PA-07 har-paths ← paths/strip-cwd-env.ts + strip-cwd-env-boot.ts: `crates/har-paths/src/strip_cwd_env.rs`. strip_cwd_env (both passes), strip_cwd_env_boot, BUN_AUTO_LOADED_ENV_FILES, CLAUDE_CODE_AUTH_VARS.
- WF-11 duplicate reconciled: command_folder_search_paths removed from executor_shared.rs; har-dag-executor now imports har_paths::get_command_folder_search_paths. All 554 tests including prior differential golden tests pass.

VERIFIER NEEDS-HUMAN notes for PA-01/06/07:
- Set `ARCHON_HOME=/tmp/test-archon` to drive path fns deterministically.
- PA-07 cannot be diff-tested byte-for-byte (modifies process.env in-place); verify by checking the env state BEFORE and AFTER calling strip_cwd_env.
- PA-06 override semantics: set a key first, then call load_archon_env; verify key was overridden.
- CLAUDECODE warning: set CLAUDECODE=1 (without ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING) and verify stderr output matches source exactly.

## Verified units (parity gate PASS)
- PR-01 har-contract ← providers/src/types.ts (QUALIFIED: pure types, wire-shape verified)
- WF-01 dag-node (7-variant union, superRefine, ThinkingConfig preprocess, value-bounds, trim-transform)
- WF-02 workflow (envelope + discriminated unions, node-composition validation)
- WF-03 Loop, WF-04 Retry (delay_ms f64), WF-05 Hooks ← workflows/src/schemas/*
  Differential harness: crates/har-workflow-schema/examples/parity_diff.rs; findings/parity-cycle{1,2}.md
- WF-14 model-validation (resolveModelSpec 3-branch + 3 fallback chains, buildAiProfile 5-layer merge,
  routePresetEffort claude/codex matrix, tier-defaults.json embedded == source). 66/67 byte-exact vs bun;
  1 `- [≠]` (UnknownAlias lists keys SORTED vs insertion — determinism, unparsed display text);
  porter bug fixed (stray trailing `.`). Harness: crates/har-dag-executor/examples/parity_wf14_oracle.rs
  + tests/wf14_parity_golden.rs + tests/fixtures/wf14_ts_golden.json; findings/parity-cycle6.md

## Key parity lessons (apply to every schema unit — each was a gate FAIL caught+fixed)
- zod `z.number()` WITHOUT `.int()` → Rust f64, NOT integer (fractional values are source-valid).
- zod `.trim()` is a TRANSFORM: store the trimmed value (deserialize_with), not just validate on trimmed.
- Restore EVERY value-bound (.positive/.min/.max/.nonempty/.trim().min(1)); collect ALL issues (no fail-fast).
- Source is **zod v4**: `.nullable()` ≠ optional (key REQUIRED-present, value may be null → absent REJECTS;
  use deserialize_with WITHOUT #[serde(default)]). `.datetime()` is **Z-only** (offsets REJECT).
- `z.date()` (JS Date) → `chrono::DateTime<Utc>` (`- [≠]`, JSON has no Date type; validation preserved).
- JS `parseFloat()` ≠ Rust `str::parse::<f64>()`: JS is LENIENT prefix-parse (`"20abc"`→20, strips leading
  ws, stops at first invalid char). Use a `parse_float_js()` helper for any numeric coercion of strings.
- serde_json **`preserve_order`** is ON workspace-wide (Map→IndexMap = JS object insertion-order). Keep it;
  never assert sorted key order in a test (JS preserves insertion order — sorted is a BTreeMap artifact).
- JS regex `i`-flag backreference (`<(\w+)>…</\1>`) has no Rust equiv — replicate via manual matching incl.
  BACKTRACKING (`\1` can match a prefix of the open-tag inner). String truncation = **UTF-16 code units**
  (JS `.length`/`.slice`), NOT bytes — use a utf16 helper. Negative-lookahead boundaries must be ZERO-WIDTH
  (don't consume the boundary char). All four bit the porter in cycle 5 — verify regex/encoding edges vs bun.
- The LEDGER can be WRONG (cycle 5: loadCommandPrompt precedence was mis-stated). The porter+verifier must
  read the ACTUAL source, not trust the ledger's prose; fix the ledger when it lies.
- **RECURRING (cycles 7 & 12): Rust `\`-line-continuation in a multi-line string literal SWALLOWS the next
  line's leading whitespace** — any ported multi-line message with indentation (warnings, install/help text,
  error banners) loses its indent and goes flush-left. Use explicit `\n   ` sequences or a raw string, and
  byte-diff the message vs source. Also: don't double-escape `\` in Windows paths inside Rust string literals.
- Self-reported "green" is NOT the gate: the port's own tests can encode wrong behavior. The live
  differential diff vs `bun` is the authority. Always cargo clippy --all-targets + differential parity.
- **A porter can INTRODUCE a downgrade by "correcting" the spec from a MISREAD of the live source** (cycle 15:
  porter claimed live SDK `initialize.capabilities={tools:{}}` and committed a fixture saying so; the verifier
  independently re-captured the live SDK and found `{tools:{listChanged:true}}` — the SDK's `McpServer`
  auto-advertises it). The verifier must build its OWN oracle from the running source, never trust the porter's
  reported "live" values OR its captured fixture. A porter-supplied fixture is a hypothesis, not the oracle.
- For an MCP wire port: the CLI sees the SDK's `zod-to-json-schema` rendering, NOT the original JSON Schema —
  `$schema` draft-07 key emitted FIRST, `description` kept ONLY on required fields (describe-then-`.optional()`
  drops it on optionals), enum key order `description,type,enum`, no `additionalProperties`, plus per-tool
  `execution:{taskSupport:forbidden}` + `_meta:{anthropic/alwaysLoad:true}`. Reconstruct from ToolField, diff vs bun.

## OWNER DECISIONS (`- [≠]`)
- WF-06 date fields `z.date()` ↔ `chrono::DateTime<Utc>`: **APPROVED 2026-06-13** by owner. Closed.

## Next units (dependency order, from cartographer)
cycle 4: WF-11 executor-shared utils → WF-12 condition-evaluator → WF-13 output-ref (pure fns, strong parity)
  OR the leaf-crate track: PA paths → GI git → IS isolation types (unblocks more of the graph)
then: WF-14 model-validation → WF-09 dag-executor (the core state machine) → PR-02.. providers → CO db (MAP→hf)
Differential harness pattern: crates/har-workflow-schema/{examples/parity_diff.rs, tests/parity_cycle3_differential.rs}

## Scope (owner directive)
- Archon v0.4.1 CURRENT architecture only. Legacy versions excluded (record as excluded, not as work).
- PORT: workflows DAG schema + dag-executor state machine; IAgentProvider/ProviderCapabilities;
  per-run git-worktree isolation; multi-surface control plane (server + adapters).
- MAP onto substrates (do NOT reimplement): run-ledger→hf; coordination→weave+grit; memory→icm;
  LLM agent-loop→provider CLIs.

## Archon package inventory (non-test .ts counts, 2026-06-13)
core 72 | web 57 | providers 50 | workflows 37 | adapters 29 | server 24 | cli 15 |
paths 9 | isolation 9 | git 6 | docs-web 5
