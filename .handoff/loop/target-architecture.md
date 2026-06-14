# Target Architecture — Archon (TS/Bun) → harness-agent-rs (Rust)

Status: DISCOVER. Authoritative for the porter. Extend, do not rewrite, prior decisions.
Source: `/home/drdave/Desktop/meta/Archon` (v0.4.1, current architecture only — legacy excluded).
Target: `/home/drdave/Desktop/meta/harness-agent-rs` (cargo workspace, resolver=2, edition 2021).
ADR-0001 substrate mapping is authoritative and is baked into the dependency table below.

Citations are `package/path:line` relative to the Archon source root.

---

## 1. Crate / module layout

Small focused crates, layered bottom-up. Dep arrows point to what a crate depends on.
The tiering mirrors Archon's `@archon/*` import DAG, which is already acyclic and contract-first
(`packages/providers/src/types.ts:1-3` is a deliberate zero-dep contract subpath — preserve that).

```
har-contract ──────────────┐  (zero-dep types: provider contract, message chunks, capabilities)
   ▲                        │
har-provider ───────────────┤  (IAgentProvider trait + registry + builtins over provider CLIs)
   ▲                        │
har-workflow-schema ────────┤  (zod→serde schemas: workflow, dag-node union, loop, hooks, retry)
   ▲          ▲             │
har-isolation │             │  (per-run git-worktree isolation; IIsolationProvider)
   ▲          │             │
har-git ──────┘             │  (git plumbing — execFile→tokio::process or git2)
   ▲                        │
har-ledger (MAP→hf) ────────┤  (durable run/workflow state — IWorkflowStore over hf, NOT Postgres)
   ▲                        │
har-coord  (MAP→weave/grit)─┤  (run locks / resumable-run coordination over weave+grit)
   ▲                        │
har-memory (MAP→icm) ───────┤  (cross-run memory over icm)
   ▲                        │
har-dag-executor ───────────┤  (the state machine: topo layers, parallel, loop, approval gates)
   ▲                        │
har-orchestrator ───────────┤  (manage_run native tool + orchestrator agent loop)
   ▲                        │
har-adapters ───────────────┤  (multi-surface: chat=slack/telegram/discord, forge=github/gitea/gitlab)
   ▲                        │
har-server ─────────────────┤  (axum control plane: REST + SSE/WS real-time push)
   ▲                        │
har-cli ────────────────────┘  (binary entrypoint)
har-paths (leaf util)          (path resolution, env loading, logging, telemetry — used widely)
```

### Crate table

| Crate | Responsibility | Ports (Archon package/units) | Public surface | Depends on |
|---|---|---|---|---|
| **har-contract** | Zero-dependency provider/message contract. The hard "no SDK, no sibling import" rule (`providers/src/types.ts:1-3`) is preserved as: this crate depends on `serde` only. | `packages/providers/src/types.ts` (whole file) | `MessageChunk` enum, `TokenUsage`, `SystemPromptInput`, `AgentRequestOptions`, `SendQueryOptions`, `NodeConfig`, `NativeTool`, `ProviderCapabilities`, `ProviderRegistration`/`ProviderInfo`, `IAgentProvider` trait | serde |
| **har-paths** | Path resolution, env loading + cwd-strip, structured logging, telemetry, update-check. | `packages/paths/src/*` (archon-paths, env-loader, strip-cwd-env, logger, telemetry, update-check, bundled-build) | `paths::{resolve_*}`, `EnvLoader`, `create_logger`, telemetry capture fns | serde, tracing, directories |
| **har-git** | Git plumbing: branch ops, repo ops, worktree add/remove/list, `execFileAsync`. | `packages/git/src/{exec,branch,repo,worktree,types}.ts` | `GitRepo`, `Worktree`, `BranchName` newtype, exec helper | tokio (process), thiserror, har-paths |
| **har-isolation** | Per-run isolation: worktree provider, resolver (adopt/reuse/create), PR-state, worktree-copy. | `packages/isolation/src/*` (types, factory, resolver, pr-state, worktree-copy, providers/) | `IIsolationProvider` trait, `IsolationRequest` enum (issue/pr/review/thread/task), `WorktreeEnvironment`, `IsolationResolution`, `IsolationHints` | har-git, har-ledger, thiserror, async-trait |
| **har-provider** | `IAgentProvider` impls over **provider CLIs** (MAP — agent-loop delegated), registry with capability declarations, builtins, MCP wiring. | `packages/providers/src/{registry,index,errors}.ts` + `src/{claude,codex,community/{copilot,opencode,pi},mcp,shared}` | `ProviderRegistry`, `get_agent_provider()`, builtin provider structs, `UnknownProviderError` | har-contract, tokio (process), serde_json, thiserror, async-trait |
| **har-workflow-schema** | All workflow zod schemas → serde structs + validation. Discriminated unions → enums. | `packages/workflows/src/schemas/*` (workflow, dag-node, loop, hooks, retry, node-artifact, workflow-run, workflow-node-session) + `model-validation`, `validator`, `condition-evaluator`, `output-ref`, `command-validation`, `validation-parser` | `WorkflowDefinition`, `DagNode` enum, `LoopNodeConfig`, `ApprovalNode`, `ThinkingConfig`, `SandboxSettings`, `ProviderCapabilities` warnings, validators | har-contract, serde, garde (or hand-rolled), thiserror |
| **har-ledger** (MAP→hf) | Durable run/workflow/event state. **Replaces Archon's Postgres `IWorkflowStore`** with an `hf`-backed impl. | `packages/workflows/src/store.ts` (`IWorkflowStore`) + `core/src/db/{workflows,workflow-events,workflow-node-sessions,sessions,conversations}.ts` (behavioral contract only — NOT the SQL) | `WorkflowStore` trait + `HfWorkflowStore` impl; `WorkflowRun`, `WorkflowEvent`, `WorkflowNodeSession`, `ApprovalContext` | har-workflow-schema, hf integration, async-trait, thiserror |
| **har-coord** (MAP→weave/grit) | Run-level locks, resumable-run claim/release, orphan reclamation. | `store.ts::{findResumableRun, resumeWorkflowRun, failOrphanedRuns, cancelWorkflowRun}` semantics | `RunCoordinator` over weave+grit | weave, grit integration, har-ledger |
| **har-memory** (MAP→icm) | Cross-run memory surface (if/where Archon persists durable agent memory). | core memory/session-context paths (scope TBD by cartographer) | `MemoryStore` over icm | icm integration |
| **har-dag-executor** | The state machine: topological parallel layers, per-node execution (command/prompt/bash/script), loop-until (fresh/shared ctx), human-approval gates, cancel nodes, resume/skip, session threading, cost accounting, capability warnings, retry/idle-timeout. | `packages/workflows/src/{dag-executor,executor,executor-shared,router,event-emitter,artifacts-index,script-discovery,deps,hooks}.ts` | `execute_dag_workflow(...)`, `build_topological_layers()`, `WorkflowDeps`, `IWorkflowPlatform` trait, `WorkflowEventEmitter` | har-workflow-schema, har-provider, har-ledger, har-coord, har-isolation, har-git, tokio, async-trait |
| **har-orchestrator** | The single-agent orchestrator path + `manage_run` native tool + prompt builder. | `core/src/orchestrator/{orchestrator,orchestrator-agent,manage-run-tool,prompt-builder,orchestrator-isolation}.ts`, `core/src/handlers/*`, `core/src/operations/*` | `Orchestrator`, `build_manage_run_tool()` (a `NativeTool`) | har-provider, har-dag-executor, har-ledger, har-isolation, tokio |
| **har-adapters** | Multi-surface platform adapters (the "send/stream to a surface" side of the control plane). | `packages/adapters/src/{chat/{slack,telegram},community/chat/discord,forge/github,community/forge/{gitea,gitlab},utils}` | `IPlatformAdapter` impls per surface; `message-splitting` | har-contract, reqwest, serde_json, tokio, async-trait |
| **har-server** | Multi-surface control plane: REST API (OpenAPI), real-time push (SSE `/api/stream/:conversationId`, dashboard stream), auth, web bridge, pg-notify→push fan-out (re-targeted onto hf/weave event stream). | `packages/server/src/*` (index, routes/api, routes/schemas/*, auth/*, adapters/web/*, github-auth-bootstrap) | `build_app()` axum Router, route handlers, auth middleware, SSE handlers | har-orchestrator, har-dag-executor, har-adapters, har-ledger, axum, tower, tokio |
| **har-cli** | Binary entrypoint, command dispatch. | `packages/cli/src/*` | `main()`, subcommands | har-server, har-orchestrator, clap, tokio |
| **har-core** | EXISTING placeholder. Fold into `har-contract` or keep as a thin re-export facade for downstream convenience. Owner decision (Risk R1). | — | — | — |

> `packages/docs-web` (5 files) and `packages/web` (57 files, React) are **frontend** — see Open Risks R2.
> They are NOT Rust crates. `packages/core` is decomposed across har-ledger / har-orchestrator /
> har-paths rather than ported as one crate.

---

## 2. Idiom map (decide once, apply everywhere)

### 2.1 Error model — `thiserror` per crate, `anyhow` only at bins
- Each TS `throw`/typed error → a **typed error enum** in the owning crate via `thiserror::Error`.
  Preserve every variant. E.g. `UnknownProviderError` (`providers/src/errors.ts:5-15`, carries
  `requestedProvider` + `registeredProviders`) → `enum ProviderError { Unknown { requested: String, registered: Vec<String> } }` with the **exact** Display string
  `"Unknown provider: '{requested}'. Available: {registered…}"`.
- Isolation's `errors.ts` and the `IsolationBlockReason='creation_failed'` (`isolation/types.ts:231`)
  → variants on `IsolationError`. DAG runtime cycle-panic (`dag-executor.ts:659`) → a
  `DagError::CycleAtRuntime` (return `Err`, do NOT `panic!` in library code).
- Crate-public APIs return `Result<T, ThisCrateError>`. `anyhow::Result` is allowed only in
  `har-cli`/`har-server` `main`/handlers for top-level context.
- Functions that "must not throw — return undefined on failure" (`deps.ts:118-147`
  `resolveBotGitHubToken`, `getUserGithubToken`) → `-> Option<T>` (never `Result`), preserving the
  fail-soft contract exactly.

### 2.2 Async — one tokio multi-thread runtime
- Bun/`Promise` → `tokio` (multi-thread, `#[tokio::main(flavor = "multi_thread")]` at the bins).
- `Promise.allSettled` over a layer (`dag-executor.ts:2848`) → `futures::future::join_all` over
  `tokio::spawn`ed tasks, collecting `Result` per node so one node's failure never aborts the layer
  (allSettled semantics — preserve: a failed node yields a `failed` NodeOutput, layer continues).
- `withIdleTimeout`/`STEP_IDLE_TIMEOUT_MS` (`dag-executor.ts:71`) → `tokio::time::timeout`.
- `AbortSignal` (`AgentRequestOptions.abortSignal`, `types.ts:244`) → `tokio_util::sync::CancellationToken` threaded through `sendQuery`; cancel nodes (`schemas/dag-node.ts:334`) trip it.

### 2.3 Streaming / event emitter
- `IAgentProvider.sendQuery(...) → AsyncGenerator<MessageChunk>` (`types.ts:423-428`) →
  `fn send_query(...) -> impl Stream<Item = MessageChunk>` via the **`async-stream`** crate
  (`stream! { ... yield chunk; }`), or a boxed `Pin<Box<dyn Stream>>` when stored in a trait object.
- `getWorkflowEventEmitter()` (`event-emitter.ts`) → a `tokio::sync::broadcast` channel
  (multi-consumer fan-out; matches the SSE dashboard + per-conversation listeners in
  `server/routes/api.ts:1981,2017`). Per-conversation single-consumer push → `mpsc`.
- Server SSE (`/api/stream/:conversationId`) → axum `Sse<impl Stream>` fed from a broadcast subscriber.

### 2.4 Ownership — fresh vs shared DAG context
- DAG node `context: 'fresh' | 'shared'` (`schemas/dag-node.ts:147`) governs session threading.
  - `shared` / sequential single-node layers thread `lastSequentialSessionId` forward
    (`dag-executor.ts:2825-2827`) → carry `Option<SessionId>` by value through the sequential path.
  - `fresh` and any parallel layer (`>1` node) reset to `None` (`dag-executor.ts:2843-2844`).
- `WorkflowDeps`/config shared read-only across spawned node tasks → `Arc<WorkflowDeps>`,
  `Arc<WorkflowConfig>` cloned into each `tokio::spawn`. `nodeOutputs: Map` mutated after each layer
  by the **single** driver task (not inside spawns) → owned `HashMap`, no lock needed; spawns return
  their output and the driver writes it post-`join_all` (matches Archon: it `await`s the layer then
  writes outputs). `WorkflowStore`/provider handles behind `Arc<dyn Trait + Send + Sync>`.

### 2.5 Traits — interfaces → object-safe `async_trait`
- `IAgentProvider` (`types.ts:415`) → `#[async_trait] trait AgentProvider: Send + Sync`. The streaming
  method returns a boxed stream (object-safe): `fn send_query(...) -> BoxStream<'_, MessageChunk>`;
  `get_type()` and `get_capabilities()` are sync. Registry holds `factory: Box<dyn Fn() -> Arc<dyn AgentProvider>>` (`ProviderRegistration.factory`, `types.ts:391`).
- `IWorkflowStore` (`store.ts:51`), `IWorkflowPlatform` (`deps.ts:57`), `IIsolationProvider`
  (`isolation/types.ts:177`) → `#[async_trait]` object-safe traits; stored as `Arc<dyn …>`.
  Optional methods (`sendStructuredEvent?`, `adopt?`) → trait methods with a `default` impl
  returning the no-op/`None` (preserves "optional" semantics without an `Option<fn>`).

### 2.6 Serialization & validation — `serde` + schema-faithful validators
- Every zod schema → a `#[derive(Serialize, Deserialize)]` struct/enum. **Preserve every constraint**:
  - `z.enum([...])` → fieldless Rust enum with `#[serde(rename_all=...)]` matching the string literals
    (e.g. `effortLevelSchema = ['low','medium','high','max']` `dag-node.ts:40`; `triggerRule`,
    `modelReasoningEffort`, `webSearchMode`).
  - `z.discriminatedUnion('type', …)` (`thinkingConfigSchema` `dag-node.ts:65`; `MessageChunk`
    `types.ts:178`) → `#[serde(tag = "type", rename_all="snake_case")]` enum, one variant per arm,
    each carrying exactly that arm's fields.
  - `z.preprocess` shorthands (`'adaptive'`→`{type:'adaptive'}`, `dag-node.ts:56-61`) → a custom
    `Deserialize` (or `#[serde(untagged)]` helper) accepting **both** the bare string and the object.
  - `.min(1)`, `.positive()`, `.nonempty()`, cross-field `.superRefine` (loop's `interactive ⇒ gate_message`, `schemas/loop.ts`) → validation via **`garde`** derives where expressible, hand-rolled
    `validate()` for cross-field rules. Preserve the exact error messages (they are user-facing).
  - `[key: string]: unknown` open bags (`ProviderDefaults`, `NodeConfig` `types.ts:330`) →
    `#[serde(flatten)] extra: serde_json::Map<String, Value>` so unknown fields round-trip (Archon
    "unknown fields are ignored", `types.ts:288`).
- The DAG node taxonomy (`command | prompt | bash | script | loop | approval | cancel`,
  `schemas/dag-node.ts:349`) → a single `enum DagNode` discriminated on node kind; the per-kind
  refinement in `dagNodeSchema.superRefine` becomes the enum's `validate()`.

### 2.7 Generics / unions
- TS union types → Rust enums (above). TS `Record<string, T>` → `HashMap<String, T>`
  (or `IndexMap` where insertion order is observed). `readonly T[]` → `&[T]` / `Vec<T>`.
- `AsyncGenerator<MessageChunk>` is the one place generics → `impl Stream` (2.3).

---

## 3. Dependency-equivalent table

| Archon lib / subsystem | Rust equivalent | Decision |
|---|---|---|
| Hono (`@hono/zod-openapi`, server) | **axum** + `utoipa` (OpenAPI) | port |
| zod (schemas) | **serde** + **garde** (validation) + custom `Deserialize` for `preprocess` | port |
| Bun subprocess / `execFileAsync` (`git/src/exec.ts`) | **tokio::process::Command** | port |
| AsyncGenerator streams | **async-stream** + `futures::Stream` | port |
| ws / SSE real-time (`/api/stream/*`) | **axum SSE** (`axum::response::Sse`); WS via **tokio-tungstenite** if a WS surface is needed | port |
| event-emitter fan-out | **tokio::sync::broadcast** / **mpsc** | port |
| git ops (`git/src/{branch,repo,worktree}.ts`) | shell out via **tokio::process** (mirrors Archon's execFile) — prefer over `git2` to keep worktree/branch semantics identical; `git2` only if a hot path needs it | port (shell-first) |
| Postgres `pg`/`postgres` driver + `IWorkflowStore` SQL (`core/src/db/*`) | **MAP — do not add a Rust DB dep.** Implement `WorkflowStore` over **hf** (the run-ledger substrate). | MAP→hf |
| run locks / resumable-run / orphan reclaim | **MAP — integrate weave + grit**, not a Rust lock crate | MAP→weave+grit |
| durable cross-run memory | **MAP — integrate icm** | MAP→icm |
| the LLM agent-loop itself | **MAP — delegate to provider CLIs** (claude/codex/copilot/opencode/pi via `tokio::process`). har-provider drives the CLI, does NOT embed an SDK. | MAP→provider CLIs |
| Slack SDK (`adapters/chat/slack`) | **slack-morphism** or `reqwest` to Web API | port (reqwest fallback) |
| Telegram SDK (`adapters/chat/telegram`) | **teloxide** or `reqwest` | port (reqwest fallback) |
| Discord SDK (`community/chat/discord`) | **serenity**/**twilight** or `reqwest` | port (reqwest fallback) |
| GitHub / Gitea / GitLab SDKs (`forge/*`) | **octocrab** (GitHub) + `reqwest` (Gitea/GitLab) | port |
| pino logger (`paths/src/logger.ts`) | **tracing** + `tracing-subscriber` | port |
| telemetry (`paths/src/telemetry.ts`) | `reqwest` + serde (keep the same event shapes) | port |
| `directories`/path resolution | **directories** / **dirs** | port |
| JSON Schema `output_format` (`types.ts:246`) | **schemars** for emit; **jsonschema** crate for the post-parse validate net (capabilities `'enforced'|'best-effort'|false`, `types.ts:367`) | port |
| clap | **clap** (derive) for har-cli | port |

> MAP rows are the ADR-0001 substrates. The porter MUST integrate the substrate (drive `hf`/`weave`/
> `grit`/`icm`, delegate to provider CLIs) and MUST NOT add a Rust dependency that reimplements them.

---

## 4. Async runtime & error strategy (workspace-wide)

- **Runtime:** one `tokio` multi-thread runtime, started in `har-cli`/`har-server` `main` via
  `#[tokio::main(flavor = "multi_thread")]`. Libraries take no runtime; they are runtime-agnostic
  `async fn`s. No nested runtimes.
- **Cancellation:** `tokio_util::sync::CancellationToken` is the workspace cancellation primitive
  (replaces `AbortSignal`); threaded through `send_query`, node execution, loop/cancel nodes.
- **Error convention:** every library crate defines its own `thiserror` enum, re-exported from its
  root as `pub enum <Crate>Error` + `pub type Result<T> = std::result::Result<T, <Crate>Error>`.
  Cross-crate errors compose via `#[from]`. `anyhow` is confined to bin crates (`har-cli`, and the
  top of `har-server` handlers for request-context). No `unwrap()`/`expect()` in library code paths
  reachable at runtime; the one Archon runtime-panic (cycle, `dag-executor.ts:659`) becomes a typed
  `Err` because cycle detection already runs at load (`har-workflow-schema` validator).
- **Edition/resolver:** keep edition 2021, resolver 2 (workspace already set).

---

## 5. Open risks (need owner decision — flag, don't guess)

- **R1 — `har-core` placeholder.** The existing green `crates/har-core` is a placeholder. Decision
  needed: fold its identity into `har-contract`, or keep `har-core` as a thin re-export facade
  (`pub use har_contract::*; pub use har_workflow_schema::*;`) for ergonomic downstream imports.
  Recommendation: keep as a facade so the baseline stays green and downstream `use` paths are stable.
- **R2 — React `web` + `docs-web` frontends (57 + 5 files).** Not Rust. Options: (a) port the
  REST/SSE **contract** only and keep the existing TS/React frontend as-is, served as static assets
  by `har-server` (recommended — no capability loss, frontend is out-of-language); (b) replace with a
  Rust/WASM UI (large scope, out of port charter); (c) drop the UI (capability downgrade — needs an
  explicit `- [≠]` owner sign-off). **Default to (a).** Owner must confirm the frontend is retained
  verbatim and only its backend contract is ported.
- **R3 — server: axum vs map onto an existing FlexNetOS control plane.** Charter says PORT the
  multi-surface control plane. Decision: build `har-server` on **axum**, but its durable state and
  event stream are MAP'd onto hf/weave (not Postgres/pg-notify). Confirm there is no existing
  FlexNetOS control-plane substrate that `har-server` should instead extend (vs a fresh axum app).
  If one exists, this becomes `map-onto-substrate` rather than `port-fresh`.
- **R4 — `hf` ledger shape vs `IWorkflowStore` contract.** `IWorkflowStore` (`store.ts:51-148`) has
  ~25 methods incl. CAS-style `resumeWorkflowRun`, `pauseWorkflowRun(approvalContext)`,
  `getCompletedDagNodeOutputs`, per-node session upsert. Need to confirm `hf` can express: (a) durable
  keyed records with optimistic concurrency (the resume-CAS test exists,
  `core/db/workflows.resume-cas.integration.test.ts`), (b) an append-only event log
  (`createWorkflowEvent`), (c) pause/resume with stored approval context. If `hf` can't express a
  method, that method is a `- [!]` owner-decision, never a silent drop or a downgrade to a Rust DB.
- **R5 — provider CLIs availability.** Built-ins claude/codex are MAP→CLI; community copilot/opencode/
  pi assume their CLIs exist on the host (paths in `types.ts:34,56,93`). The Rust port keeps these as
  CLI-delegated providers; capability flags (`ProviderCapabilities`, `types.ts:349`) are preserved
  per-provider. No SDK is embedded. Confirm the loop targets claude/codex first; community providers
  port as the same trait impl shape (no special-casing needed).
- **R6 — `manage_run` NativeTool boundary.** Archon crosses the providers↔core boundary as "data + a
  function" to avoid a cyclic import (`types.ts:264-281`). In Rust this is a `NativeTool` whose
  `handler` is a `Box<dyn Fn(Value) -> BoxFuture<Result<String>> + Send + Sync>` closing over `Arc`'d
  context. Confirm object-safety + `Send + Sync` bounds hold across the spawn boundary (they do for
  `Arc<dyn …>` captures); flagged because it's the one place the closure-over-context pattern is
  load-bearing.

---

## Summary for the porter

- **Cycle order (bottom-up):** har-contract → har-paths → har-git → har-workflow-schema →
  har-provider → har-isolation → har-ledger(hf) → har-coord(weave/grit) → har-dag-executor →
  har-orchestrator → har-adapters → har-server → har-cli. Port a leaf before its dependents.
- **The crown jewel** is `har-dag-executor` (`dag-executor.ts`, 2750+ LOC): topo layers + parallel
  `join_all` (allSettled) + loop/approval/cancel nodes + fresh/shared session threading + resume-skip
  + cost accounting + capability warnings. Do NOT downgrade any branch.
- **Do not reimplement** the ledger (hf), coordination (weave/grit), memory (icm), or the agent-loop
  (provider CLIs) — integrate the substrates per the MAP rows.
