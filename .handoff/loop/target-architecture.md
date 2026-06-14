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

---

## 6. Provider-adapter strategy (PR-03+) — SDK→CLI, decided once for all built-in + community providers

This is the per-unit structural decision for **PR-03 ClaudeProvider**, and by construction the same
shape for **PR-07 CodexProvider** and **PR-09/10/11** (copilot / opencode / pi). It refines R5 from a
"keep as CLI-delegated" hand-wave into the concrete idiom map the porter executes.

### 6.0 Decision (the one-line answer)

`ClaudeProvider::send_query` is implemented as **DELEGATE→provider-CLI** per ADR-0001: spawn the
`claude` (claude-code) binary as a `tokio::process::Child` with
`--print --output-format stream-json --verbose --input-format text`, write the prompt to stdin, parse
its NDJSON stdout (one JSON object per line) and map each line → `MessageChunk`. **No Rust port of
`@anthropic-ai/claude-agent-sdk` is built, embedded, or vendored.** The TS SDK is itself only a typed
wrapper that spawns this same binary in this same mode (the provider already configures it through the
SDK: `pathToClaudeCodeExecutable`, `executableArgs`, `stderr` callback, `abortController`,
`permissionMode: 'bypassPermissions'` — `provider.ts:509-560`), so the CLI **is** the real boundary;
the SDK was an in-process convenience over it. This is exactly the model Archon **already ships** for
Codex: `CodexProvider` consumes `@openai/codex-sdk`, which drives the `codex` CLI via
`codexPathOverride`/`runStreamed` (`codex/provider.ts:64,855`) and normalizes a CLI event stream
(`thread.started` / `item.completed` / `turn.completed` …) → `MessageChunk` in `streamCodexEvents`
(`codex/provider.ts:330-642`). PR-03 is therefore the **same proven pattern**, applied to claude.

> Why this is no-downgrade: every `sendQuery` behavior in `provider.ts` is either (a) an argv/option
> the SDK forwards to the CLI (portable 1:1), (b) an event→chunk mapping the CLI's `stream-json`
> already emits (portable 1:1), or (c) an in-process Node concern (hooks closures, in-process MCP) that
> has a CLI-flag or config-file equivalent — except `native-tools` (§6.5), the single genuine
> `NEEDS-HUMAN`. No feature is dropped to "make Rust easier."

### 6.1 What the SDK wraps, confirmed

The claude-code CLI exposes a non-interactive streaming mode that emits the *same* message objects the
SDK surfaces — the normalizer in `streamClaudeMessages` (`provider.ts:633-767`) reads SDK events
`{type:'assistant'|'system'|'rate_limit_event'|'result'}`, which are the claude-code `stream-json`
line types verbatim. Evidence the provider already speaks CLI: it logs and special-cases the CLI's own
stderr banners `--output-format` / `--permission-mode` / `Spawning Claude Code` (`provider.ts:552-554`),
and resolves a spawnable `claude` binary (`binary-resolver.ts:126-169`, `CLAUDE_BIN_PATH` → config →
`~/.local/bin/claude`). The Rust impl spawns that binary directly with `--print --output-format
stream-json --verbose`; the SDK layer disappears.

- **Build-time NEEDS-HUMAN gate (R7, below):** the porter MUST capture a real
  `claude --print --output-format stream-json --verbose` session and confirm each emitted line type is
  one of `{system(subtype:init), assistant, user, result, ...}` matching the SDK fields the normalizer
  reads (`session_id`, `usage`, `structured_output`, `is_error`/`subtype`, `stop_reason`, `num_turns`,
  `model_usage`, `total_cost_usd`, `mcp_servers[].status`, `rate_limit_info`). If the CLI's line schema
  is **not** 1:1 with the SDK message shape, that delta is an `- [≠]`/`- [!]` owner item — never a
  silently dropped field.

### 6.2 Option → CLI-flag mapping (the deterministic argv builder)

`buildBaseClaudeOptions` + `applyNodeConfig` (`provider.ts:496-561`, `275-442`) build an SDK `Options`
object. In Rust this becomes a pure function `build_claude_argv(&SendQueryOptions, &NodeConfig,
&AssistantDefaults, cwd) -> (Vec<OsString> /*argv*/, ClaudeRunConfig /*settings file + env*/,
Vec<ProviderWarning>)`. Mapping (SDK option → claude-code CLI surface):

| sendQuery / nodeConfig field (source) | CLI surface |
|---|---|
| `model` (`provider.ts:516`) | `--model <id>` |
| `fallbackModel` (`524`) | `--fallback-model <id>` |
| `resume`=resumeSessionId (`936`) | `--resume <session_id>` |
| `forkSession` (`530`) | `--fork-session` |
| `permissionMode:'bypassPermissions'` + `allowDangerouslySkipPermissions` (`533-534`) | `--permission-mode bypassPermissions --dangerously-skip-permissions` |
| `systemPrompt` preset/append (`535`) | `--system-prompt` / `--append-system-prompt` (preset `claude_code` is the default) |
| `settingSources` (`536`, default `['project','user']`) | `--setting-sources project,user` |
| `allowed_tools`→`tools` / `denied_tools`→`disallowedTools` (`282-289`) | `--allowed-tools …` / `--disallowed-tools …` |
| `mcp` config + wildcard allow (`319-333`) | `--mcp-config <file>` (+ `mcp__<server>__*` added to allowed-tools) |
| `agents` / `skills`→AgentDefinition (`345-396`) | `--agents <json>` (skills wrapped as an inline agent, as today) |
| `effort` / `thinking` / `sandbox` / `betas` (`398-416`) | `--effort` / `--thinking` (or settings file) / `--sandbox` / `--betas …` |
| `output_format`→json_schema (`418-424`, `518`) | `--output-format-schema <json_schema>` (distinct from the **stream**-json transport flag) |
| `maxBudgetUsd` (`521`) | `--max-budget-usd <n>` |
| `hooks` (declarative YAML matchers, `291-316`, `233-255`) | written to a `--settings <file>` hooks block — **declarative-only** (see §6.5) |
| `env` (per-request codebase env) (`867`) | child-process `env` (not argv) |
| `executableArgs:['--no-env-file']` when JS cli (`514`, `shouldPassNoEnvFile` `487-490`) | only when the resolved binary is a Bun-runnable `.js`/`.mjs`/`.cjs` |
| `pathToClaudeCodeExecutable` (`513`) | the spawned program path (from `resolve_claude_binary_path`) |

Auth modes (`buildSubprocessEnv` `88-99`: `CLAUDE_USE_GLOBAL_AUTH`, `CLAUDE_CODE_OAUTH_TOKEN`/
`CLAUDE_API_KEY` detection), the root/UID-0 guard (`constructor` `830-835`), and the
`stripCwdEnv`-clean env assumption (`provider.ts:82-99` comment, owned by `har-paths`) are carried as
child-process env construction — **deterministic, fully testable** without a live model.

### 6.3 Event line → `MessageChunk` mapping (the deterministic parser)

`streamClaudeMessages` (`provider.ts:633-767`) is the second pure unit:
`parse_claude_stream_json(line: &str, &mut ToolResultQueue) -> Vec<MessageChunk>`. Mapping is 1:1 with
the existing normalizer — preserve every branch:

- `assistant` → for each content block: `text`→`MessageChunk::Assistant{content}`;
  `tool_use`→`MessageChunk::Tool{tool_name,tool_input,tool_call_id:id}` (`657-668`).
- `system` subtype `init` with `mcp_servers` → emit `MessageChunk::System` only for servers whose
  `status != "connected"` (`669-682`).
- `rate_limit_event` → `MessageChunk::RateLimit{rate_limit_info}` (`683-686`).
- `result` → `MessageChunk::Result{ session_id, tokens (normalizeClaudeUsage `64-79`), structured_output,
  cost, stop_reason, num_turns, model_usage, is_error/error_subtype/errors }` — **including** the
  load-bearing `is_error===true && subtype==='success'` ⇒ *clean success* reclassification
  (`716`, the `stop_sequence` termination case). This exact predicate is a golden-test target.
- Tool-result draining: PostToolUse/PostToolUseFailure hook output is captured into a queue and drained
  *between* events as `MessageChunk::ToolResult` (`buildToolCaptureHooks` `569-625`, drain `639-649`,
  `756-766`). **In CLI mode the equivalent is the `user`-role tool-result lines the CLI emits in
  stream-json** — the Rust parser reads those directly instead of running an in-process hook closure
  (this is the clean replacement for the in-process capture hook; the `10_000`-char truncation and the
  `❌ Error`/`⚠️ Interrupted` prefixes are preserved in the parser).

Provider-warning emission (`provider.ts:879-882`, the `mcp_env_vars_missing` / `mcp_haiku_tool_search`
system chunks) is produced by the argv builder and yielded before streaming — deterministic.

### 6.4 Deterministic-vs-live split + differential-parity plan for PR-03

The unit splits cleanly into a **deterministic core** (differentially testable, the bulk of the LOC)
and a **live-only tail** (cannot be diffed; proven by contract + a smoke gate):

**Deterministic (the parity-verifier proves these, no model call):**
1. **argv/config differential.** For a representative matrix of `(SendQueryOptions, NodeConfig)` —
   plain prompt; model+fallback; resume(sessionId)+fork; mcp config (with present and missing env
   vars); skills; inline agents; effort+thinking+sandbox+betas; output_format schema; haiku+mcp
   warning; denied/allowed tools — run **both** the TS path and `build_claude_argv`, and diff the
   resulting CLI invocation. *How to get the TS "expected":* intercept what the SDK hands the CLI by
   stubbing the spawn (the SDK's `pathToClaudeCodeExecutable` + `executableArgs` + the flags it derives
   from `Options`), or assert against a recorded argv fixture. Diff argv + settings-file JSON + env
   keys. Any divergence fails PR-03.
2. **event→chunk differential (golden stream-json).** Capture/synthesize NDJSON sequences covering:
   assistant text; assistant tool_use; system-init with a failed MCP server; rate_limit_event;
   `result` success; `result` with `is_error:true,subtype:'success'` (the stop_sequence case);
   `result` with a real error subtype + `errors[]`; interleaved user tool-result lines (success +
   failure + interrupt + >10k truncation). Feed each sequence through **both** the TS
   `streamClaudeMessages` and the Rust `parse_claude_stream_json`, and diff the `MessageChunk` streams
   field-by-field (serde-serialize both to JSON and compare). This is the core no-downgrade proof.
3. **golden unit tests** for: `normalize_claude_usage` (the `input`/`output` present, `total` optional
   logic `64-79`); `structured_output` extraction from the result chunk; error classification
   (`classifySubprocessError` `116-125`: rate_limit/auth/crash/unknown patterns) + retry eligibility
   (`classifyAndEnrichError` `775-812`, incl. the aborted/timeout precedence at `783-792`); the
   `withFirstMessageTimeout` first-event-timeout + abort + `#1067` diagnostic message (`160-197`);
   `shouldPassNoEnvFile` (`487-490`).

**Live-only (cannot be diffed — proven by contract, not output equality):**
- The actual model tokens/text/tool choices a live `claude` produces. Parity here is **behavioral
  contract**, not output equality: a single **smoke gate** (run one trivial real `--print
  --output-format stream-json` query if `CLAUDE_BIN_PATH`/auth is available in the verify env; else mark
  the live leg `SKIPPED — env-gated` in the parity ledger, never `PASS`) confirms the spawned argv is
  accepted by the CLI and that ≥1 `assistant` + a terminal `result` line round-trip through the parser.
  The retry loop (`MAX_SUBPROCESS_RETRIES=3`, exponential `RETRY_BASE_DELAY_MS=2000`, `894-988`) is
  unit-tested with an **injected fake spawner** (constructor already takes `retryBaseDelayMs`,
  `829-837` — port that seam) that yields scripted crash/rate_limit/success sequences; no live model
  needed to prove the retry/backoff/abort behavior.

The differential harness reuses the `rust-port-parity` skill's golden-fixture method: fixtures live
under `crates/har-provider/tests/fixtures/claude/{argv,stream}/…`; the TS reference outputs are
captured once from the source tree and committed as `.expected.json`.

### 6.5 NEEDS-HUMAN risks specific to PR-03+

- **R7 — claude-code stream-json schema fidelity.** PR-03 assumes the CLI's `--output-format
  stream-json` line schema is 1:1 with the SDK message objects the normalizer reads (§6.1). If a field
  the `result` mapping needs (`structured_output`, `model_usage`, `total_cost_usd`,
  `mcp_servers[].status`, `rate_limit_info`) is absent or renamed on the CLI surface, it is an
  `- [≠]` owner item. **Gate: capture a real stream-json session before writing the parser.**
- **R8 — in-process `NativeTool` MCP server has no CLI equivalent (the one true blocker).**
  `native-tools.ts` builds an **in-process** SDK MCP server via `createSdkMcpServer`/`tool()` whose
  handlers are **live Rust/TS closures** (`buildArchonMcpServer` `70-87`), registered as
  `mcpServers[ARCHON_TOOL_SERVER]` (`provider.ts:924-932`). A spawned `claude` subprocess runs in its
  own OS process and **cannot call an in-process closure** in the parent. This is how `manage_run`
  (the orchestrator's native tool, R6) reaches the model. Options, in preference order, all
  **no-downgrade** — pick one with the owner, do **not** drop `nativeTools`:
  - **(a) sidecar stdio-MCP server (recommended).** Spin up a tiny in-process MCP server bound to a
    unix socket / stdio, write a generated `--mcp-config` pointing the CLI at it (the CLI connects out
    to it as an MCP client), and dispatch each tool call back to the same Rust `NativeTool.handler`
    closure. Preserves the exact `NativeTool` contract (`har-contract`, `types.ts:276-281`) and the
    `mcp__archon__*` allowed-tools wildcard. This is the faithful port of "in-process tool" onto the
    CLI boundary.
  - **(b)** map onto an existing FlexNetOS MCP substrate (`mcp_hub`) if one already hosts archon tools
    — `map-onto-substrate` rather than reimplement.
  - **(c)** gate `nativeTools` off for claude with an explicit `ProviderCapabilities.nativeTools=false`
    **only with owner `- [≠]` sign-off** — this *is* a capability downgrade and is the last resort.
  - **Recommendation:** (a). Flag for owner before PR-03 ships, since it changes the `har-provider`
    internal surface (adds a `cli_stream::mcp_sidecar` helper). The argv/parser work (§6.2/§6.3) is
    independent and can land first; `nativeTools` wiring is its own follow-up sub-unit.
- **R9 — declarative hooks only.** Archon's hooks are already declarative YAML matchers turned into
  trivial closures that just return a canned `response` (`buildSDKHooksFromYAML` `233-255`:
  `hooks:[async()=>m.response]`). These port to a `--settings` hooks block (static response, no live
  callback) with **no behavior loss**. The *only* live-closure hooks are the PostToolUse capture hooks
  (§6.3), which are replaced by reading the CLI's `user` tool-result lines. Confirm the claude-code
  `--settings` hooks schema accepts the matcher+response+timeout shape; if not, `- [!]` owner item.

### 6.6 Crate / module layout for the provider family

Refines the `har-provider` row of §1. The CLI-subprocess machinery is factored into **one shared
helper module reused by every CLI-delegated provider** — claude, codex, and the three community
providers all spawn a CLI and parse a streaming event protocol, so the spawn/stream/retry/cancel/
stderr-capture scaffold is written once:

```
crates/har-provider/src/
  lib.rs                      // ProviderRegistry, get_agent_provider(), builtins registration (registry.ts, index.ts)
  errors.rs                   // ProviderError (UnknownProviderError → exact Display, errors.ts:5-15)
  capabilities.rs             // ProviderCapabilities consts per provider (claude/capabilities.ts, codex/capabilities.ts)
  cli_stream/                 // ★ SHARED CLI-subprocess helper (the reusable substrate)
    mod.rs                    //   spawn(program, argv, env, cwd) -> Child via tokio::process
    stream.rs                 //   line-framed NDJSON read of stdout → impl Stream<Item = Result<Value>>
    retry.rs                  //   MAX_SUBPROCESS_RETRIES + exp-backoff + first-event-timeout + abort (provider.ts:894-988,160-197)
    cancel.rs                 //   CancellationToken → child kill + AbortController parity (per-attempt controller, provider.ts:886-902)
    stderr.rs                 //   stderr line classification (error vs info banner, provider.ts:538-559)
    spawner.rs                //   trait Spawner (real + fake) — the injected seam for retry tests
    mcp_sidecar.rs            //   ★ R8(a): in-process NativeTool → out-of-process MCP server bridge
  claude/
    mod.rs                    //   ClaudeProvider: IAgentProvider impl (send_query orchestration, provider.ts:851-989)
    argv.rs                   //   build_claude_argv (§6.2) — DETERMINISTIC, unit+differential tested
    parser.rs                 //   parse_claude_stream_json (§6.3) — DETERMINISTIC, golden+differential tested
    config.rs                 //   parse_claude_config (config.ts) — model/settingSources/claudeBinaryPath
    binary_resolver.rs        //   resolve_claude_binary_path (binary-resolver.ts:126-169) + path_kind/validate_and_expand
    native_tools.rs           //   NativeTool → mcp_sidecar registration (native-tools.ts; the JSON-Schema→tool-shape mapping)
  codex/
    mod.rs, argv.rs, parser.rs, config.rs, binary_resolver.rs   // PR-07, same shape; parser maps codex item.* events
  community/
    copilot/ , opencode/ , pi/                                  // PR-09/10/11, same shape; per-provider argv+parser
  mcp/
    config.rs                 //   load_mcp_config (mcp/config.ts) — shared by claude+codex argv builders
  shared/
    structured_output.rs      //   normalize_json_schema_for_openai_strict etc. (shared/structured-output.ts) — codex path
```

`ClaudeProvider`/`CodexProvider`/community impls are thin: they assemble argv (their own `argv.rs`),
hand it to `cli_stream`, and feed each line to their own `parser.rs`. The trait stays
`async_trait IAgentProvider` returning `Pin<Box<dyn Stream<Item=MessageChunk> + Send>>` (PR-01
contract, already ported). The registry's `UnimplementedProvider` factory seam (PR-02) is replaced
provider-by-provider as each `argv.rs`+`parser.rs` pair passes its differential gate.

### 6.7 Porter checklist for PR-03 (cycle gate)

1. Land `cli_stream/` (mod+stream+retry+cancel+stderr+spawner) — generic, no claude specifics; unit-test
   retry/backoff/first-event-timeout with the **fake `Spawner`**.
2. Land `claude/binary_resolver.rs` + `claude/config.rs` (pure; golden-test against binary-resolver.ts
   path-kind cases).
3. Land `claude/argv.rs` → pass the **argv differential** (§6.4 #1).
4. Land `claude/parser.rs` → pass the **stream-json differential** + golden tests (§6.4 #2/#3).
5. Wire `ClaudeProvider::send_query` over `cli_stream`; register in `ProviderRegistry` (replace the
   `UnimplementedProvider` claude seam).
6. **Live smoke gate** (env-gated; `SKIPPED` if no `CLAUDE_BIN_PATH`/auth) — never blocks the cycle on
   absence, but must PASS when present.
7. `native_tools.rs` + `cli_stream/mcp_sidecar.rs` (R8) as a **follow-up sub-unit** once owner picks
   option (a)/(b)/(c). Until then `ProviderCapabilities.nativeTools` for claude is declared per the
   owner decision — **do not silently set it false.**

PR-07 (codex) and PR-09/10/11 (community) reuse steps 1–6 verbatim with their own `argv.rs`/`parser.rs`;
codex additionally reuses `shared/structured_output.rs`.

---

## 6.8 R8 resolved — native-tools loopback MCP sidecar (the faithful, no-downgrade band-aid)

**Owner decision R8 (2026-06-14, binding):** the 3 interim options are all BAND-AIDS; the REAL fix is a
pure-Rust-native provider (`docs/POST-PORT-UPGRADES.md` UP-1), built AFTER 100% port. For the port NOW:
implement a band-aid that **KEEPS THE FULL native-tools feature — no downgrade**.
`ProviderCapabilities.native_tools` for claude **stays `true`** (`har-provider/src/lib.rs:84`). The
argv seam `native_tools_mcp_config_path` in `build_claude_argv` (`argv.rs:107,413-429`) is already wired
(adds `--mcp-config <path>` + `mcp__archon__*`). This subsection supersedes §6.5-R8's "options
(a)/(b)/(c) — pick one" and the "DEFERRED to UP-1 / tools will NOT be available this turn" stance in
`claude/provider.rs:463-475`. That warning is a quiet downgrade and **must be deleted** when this lands.

### The crux, verified against source (not assumed)

`buildArchonMcpServer()` (`native-tools.ts:70-87`) builds an **in-process** SDK MCP server via
`createSdkMcpServer({name:'archon',version:'1.0.0',tools,alwaysLoad:true})`, wired at
`provider.ts:924-932` as `options.mcpServers['archon']=server` + `allowedTools.push('mcp__archon__*')`.
Each tool handler is `async(args)=>string` wrapped to `{content:[{type:'text',text}]}` (`native-tools.ts:76-78`).
In Rust, `NativeTool.handler` is `Arc<dyn Fn(HashMap<String,Value>)->Future<String>+Send+Sync>`
(`har-contract/src/lib.rs:469-473`) — an **in-process closure** (the one real tool, `manage_run`, closes
over live orchestrator run-state: `codebaseId` + `startWorkflow`, `manage-run-tool.ts:147-188`).

The claude CLI's delegation model passes MCP servers as a `--mcp-config <file>` whose servers the CLI
then **connects to as an MCP client**. The CLI's server-config schema is a discriminated union on `type`
(extracted from the installed `claude` 2.1.177 bundle): `type:"sdk"` (in-process — unavailable from a
separate program), `type:"stdio"{command,args?,env?}`, `type:"sse"{url,headers?}`, `type:"http"{url,headers?}`,
`type:"ws"`. **A spawned subprocess cannot reach the parent's in-process `Arc` closure** (the closure holds
live orchestrator state by reference; serializing it across a process boundary would sever `manage_run`
from the run it operates on). Therefore the faithful band-aid is an **in-process loopback MCP server**: the
Rust `har-provider` process itself serves MCP over `127.0.0.1`, the handler closure stays in-process, and
the CLI connects in over a local transport. **Confirmed correct.**

### Decision 1 — Transport: in-process loopback **HTTP** server (streamable-HTTP MCP)

**Locked: `type:"http"`, `url:"http://127.0.0.1:<ephemeral-port>/mcp"`.** Rejected alternatives:
- `type:"stdio"` — would force the CLI to *spawn* our server as a child `{command,args}`; that child is a
  separate process and **cannot** reach the parent's `Arc` closure. Fatal — same wall as a subprocess sidecar.
- `type:"sdk"` — in-process to the **CLI's own** process, not ours; only the TS SDK (which embeds the CLI)
  can use it. Unavailable across the spawn boundary.
- unix-socket — the CLI's `http`/`sse` configs take a `url` (TCP); no documented unix-socket transport in
  the config schema. Loopback TCP is the portable, schema-supported equivalent.
- `sse` vs `http` — both carry `{url,headers?}`. **`http`** (streamable-HTTP, the current MCP transport)
  chosen over legacy `sse`; if the live-CLI smoke gate (§6.8 Decision 8) shows 2.1.x rejects `http` for a
  config-file server, fall back to `sse` (identical config shape, one-line change) — recorded as the only
  transport contingency.

**Exact `--mcp-config` JSON the CLI consumes** (written to a temp file by Decision 5; satisfies BOTH the
ported `loadMcpConfig` normalizer, `mcp/config.ts`, which accepts a bare server-map or a `{mcpServers:{…}}`
wrapper, AND the live CLI's union schema):

```json
{ "mcpServers": { "archon": { "type": "http", "url": "http://127.0.0.1:<PORT>/mcp" } } }
```

`headers` is omitted (loopback, no auth). The CLI-side `alwaysLoad` is NOT set on the config object — it
surfaces per-tool in `tools/list` `_meta` (Decision 3), exactly as the SDK does. `loadMcpConfig`'s env-var
expansion (`$VAR`) is a no-op here (no `$` in the literal url) — round-trips unchanged.

### Decision 2 — MCP protocol surface the in-process server implements (minimal + COMPLETE)

The server (one tool, `manage_run`) must speak JSON-RPC 2.0 over streamable-HTTP and implement exactly:
- `initialize` → result `{ protocolVersion:"<negotiated>", serverInfo:{name:"archon",version:"1.0.0"},
  capabilities:{tools:{listChanged:true}} }`. *Captured live from the SDK server* (cycle-15 verifier-
  confirmed, against `@anthropic-ai/claude-agent-sdk` 0.2.141): `serverInfo={name:"archon",version:"1.0.0"}`,
  `capabilities={tools:{listChanged:true}}` (the SDK's `McpServer` auto-advertises `listChanged:true`; a
  cycle-15 porter "correction" to `{tools:{}}` was REFUTED by the differential gate). Echo the
  client's `protocolVersion` if supported, else the server's pinned default (mirror the MCP SDK's
  negotiation; pin the exact version string in the smoke gate).
- `notifications/initialized` → no response (notification).
- `tools/list` → `{tools:[ <see Decision 3> ]}`.
- `tools/call` → `{content:[{type:"text",text}], isError?:true}` (Decision 4).
- `ping` → `{}` (implement; the CLI health-checks MCP servers per `claude mcp get` "health-checked").

`listChanged:true` is advertised but the tool set is static (no `tools/list_changed` notification ever sent —
matches the SDK, whose set is also fixed at build).

### Decision 3 — `tools/list` `inputSchema`: emit the **`zod-to-json-schema` rendering**, NOT the original

**This is the parity trap.** The wire `inputSchema` the CLI sees is the SDK's
`zodToJsonSchema(zodShape,{strictUnions:true,pipeStrategy:"input"})` output (`sdk.mjs`: the in-process
server lists `inputSchema:(()=>{let Y=X9(Q.inputSchema);return Y?BB(Y,{strictUnions:!0,pipeStrategy:"input"})…})`),
where the zod shape was itself reconstructed from `NativeTool.inputSchema` by `jsonSchemaToZodShape`
(`native-tools.ts:24-59`). So the Rust server must emit a **reconstruction that matches `zod-to-json-schema`
output**, NOT `NativeTool.input_schema` verbatim. **Captured live** (SDK in-memory transport, `manage_run`):

```json
{
  "type": "object",
  "properties": {
    "action":  { "description": "<desc>", "type": "string", "enum": ["help","list","get","start"] },
    "subtool": { "type": "string" },
    "runId":   { "type": "string" },
    "confirm": { "type": "boolean" }
  },
  "required": ["action"],
  "$schema": "http://json-schema.org/draft-07/schema#"
}
```

Non-obvious rules the Rust serializer MUST replicate (each verified live, byte-for-byte):
1. **`$schema":"http://json-schema.org/draft-07/schema#"` is appended** to the inputSchema object.
2. **`required` lists only non-optional fields**, in declaration order.
3. **`description` is dropped on OPTIONAL fields, kept only on REQUIRED fields.** Verified: a field built
   `z.string().describe("run id").optional()` (exactly `native-tools.ts:55-56`'s order:
   `field=field.describe(d)` THEN `required?field:field.optional()`) serializes to `{"type":"string"}` —
   **no description**. Only `action` (required) keeps its description. The ported `ToolField` already
   carries `{kind,description,required}` (`native_tools.rs:59-70`) — the serializer emits `description` iff
   `required==true`. (For `manage_run`, only `action` is required, so only `action` shows a description.)
4. **Enum field key order: `description` FIRST, then `type`, then `enum`** (`{"description":…,"type":"string","enum":[…]}`).
   `serde_json` with `preserve_order` (already enabled, root Cargo.toml:31) lets the serializer emit keys in
   this exact order. Plain string/boolean fields emit `{"type":"string"}` / `{"type":"boolean"}` only.
5. **NO `additionalProperties`** key is emitted.
6. Each tool object ALSO carries (verified live): `"execution":{"taskSupport":"forbidden"}` and
   `"_meta":{"anthropic/alwaysLoad":true}` (this is where `alwaysLoad:true` lands on the wire — NOT in the
   config file). The Rust `tools/list` must emit both.

**Decision: the Rust server reconstructs the wire `inputSchema` from the ported `ToolField` Vec**
(`native_tools.rs` `validate_and_convert_schema` → `Vec<ToolField>`), NOT by passing `NativeTool.input_schema`
through. Rationale: the original JSON Schema has no `$schema`, keeps descriptions on optional fields, and may
order keys differently — it would **not** match the SDK's `zod-to-json-schema` wire shape, failing the
differential. The reconstruction-from-`ToolField` path reproduces rules 1-6 deterministically.
**Differential capture from bun (pin this fixture):** the SDK server's `tools/list` `inputSchema` (+
`execution`/`_meta`) for `manage_run` built via the real `INPUT_SCHEMA` (`manage-run-tool.ts:54-89`) → commit
as `tests/fixtures/claude/native_tools/tools_list.expected.json`.

### Decision 4 — `tools/call`: args → handler → `{content:[{type:"text",text}]}`; faithful error behavior

Verified live, two distinct paths — both MUST be reproduced:
- **Valid args, handler returns text** → `{content:[{type:"text",text:"<handler output>"}]}` (no `isError`).
- **Handler throws** → the SDK's `tool()` wrapper **catches** and returns
  `{content:[{type:"text",text:"<Error.message>"}],isError:true}`. (Verified: a throwing handler with valid
  args yields `{"content":[{"type":"text","text":"handler exploded"}],"isError":true}`.) So the Rust dispatch
  wraps the `NativeToolHandler` call: `Ok(text)=>{content:[text]}`, and since the Rust handler returns
  `String` (infallible — `manage-run-tool.ts:153-187` catches everything internally and returns an error
  *string*, never throwing), `isError` is effectively never set by `manage_run` itself. **But the wrapper-catch
  path must still exist** (faithful to the SDK) for any future fallible tool — implement it as: if the future
  panics/aborts, surface `{content:[{type:"text",text:<msg>}],isError:true}`.
- **Arg-validation failure (bad enum / wrong type)** → the SDK validates args against the zod shape *before*
  the handler and returns `{content:[{type:"text",text:"MCP error -32602: Input validation error: …"}],isError:true}`
  (verified: invalid `action` enum value). **Faithful Rust behavior:** validate `tools/call` args against the
  `Vec<ToolField>` (required present? enum value allowed? type matches?) and on failure return the same
  `isError:true` text-result shape. The exact `-32602` message body is zod-specific; pin it as a `- [≈]`
  *qualified*-parity item (message text may differ in detail — the *shape* (`isError:true`, text content) is
  the hard contract; capture the SDK's exact string as the fixture and match structure, not necessarily the
  zod error prose). This is the one place full byte-parity is impractical without porting zod's error
  formatter; flag as `- [≈]` in the parity ledger, not `- [≠]` (no capability lost — bad args still rejected).

### Decision 5 — Lifecycle & `send_query` wiring

Mirror `provider.ts:924-932` ("merge so a nodeConfig mcp config and native tools can coexist"). When
`requestOptions.native_tools` is non-empty (`provider.rs:466`):
1. Build the `McpServerDescriptor` (`build_archon_mcp_server`, `native_tools.rs:210`) from the tools.
2. **Start the loopback HTTP MCP server**: bind `TcpListener` on `127.0.0.1:0` (ephemeral port), spawn the
   axum/tower MCP router as a `tokio::task`, capture the bound port.
3. **Write the temp mcp-config file** (`tempfile::NamedTempFile`, already a har-provider dep) with the
   Decision-1 JSON pointing at `http://127.0.0.1:<port>/mcp`. Keep the handle alive for the query's lifetime.
4. **Set the argv seam**: pass the temp file path as `native_tools_mcp_config_path` to `build_claude_argv`
   (the seam at `argv.rs:413-429` already appends `--mcp-config <path>` and `mcp__archon__*` to allowed-tools)
   — and **MERGE** with any `nodeConfig.mcp` servers: if nodeConfig already supplies a `--mcp-config`, the
   merged config file must contain BOTH that node's servers AND `archon` (faithful to the SDK's
   `options.mcpServers={...existing, archon}` spread). Implementation: when both are present, read the
   nodeConfig mcp config, inject the `archon` server into its `mcpServers` map, write the merged file, and
   pass the single merged path (the CLI accepts space-separated `--mcp-config` files too, but a single merged
   file is cleanest and matches the SDK's single merged `mcpServers` object).
5. **Shut down** the server + drop the temp file when the query ends, errors, or is cancelled — tie its
   lifetime to the `send_query` stream/`CancelGuard` (`cli_stream/cancel.rs`): on stream completion or
   `CancellationToken` trip, abort the server task and drop the `NamedTempFile`. The server outlives all
   retry attempts within one `send_query` (the `Arc` closures and port are stable across retries — bind ONCE
   before the retry loop, mirror how subprocess env is built once at `provider.rs:383-385`).

**Placement:** the bind/write/merge/teardown lives in `ClaudeProvider::send_query` (`provider.rs`, replacing
the inert warning block at `463-475`), delegating the server impl to the new `cli_stream/mcp_sidecar.rs`
module. The argv mutation already lives in `claude/argv.rs` via the seam — no change there.

### Decision 6 — Crate/module placement & deps

New module **`crates/har-provider/src/cli_stream/mcp_sidecar.rs`** (the §6.6 layout already reserves this
slot) — placed in the **shared `cli_stream/`** substrate, NOT under `claude/`, because codex/community
providers with native tools reuse the identical loopback-MCP bridge (no claude specifics in the MCP protocol
itself). `claude/native_tools.rs` stays the claude-specific JSON-Schema→`ToolField`→wire-`inputSchema`
serializer (it owns the `zod-to-json-schema`-faithful rendering of Decision 3); `mcp_sidecar.rs` owns the
generic JSON-RPC/HTTP server + dispatch-to-`NativeToolHandler`. **Deps — reuse workspace, add nothing new:**
- HTTP server: **axum 0.8 + tower** (already workspace deps, root Cargo.toml:59-60; `tokio` is `full`). The
  MCP streamable-HTTP endpoint is a single `POST /mcp` axum handler doing JSON-RPC dispatch — no extra crate.
- temp file: **`tempfile`** (already har-provider dep, Cargo.toml:22).
- No `rmcp`/MCP-SDK crate is pulled — the surface (Decision 2) is tiny (5 methods, one tool) and hand-rolling
  it over axum is lighter and keeps the wire shapes under our exact control for the differential. (If a future
  unit needs the full MCP spec, revisit `rmcp` then — out of scope now.)

### Decision 7 — Cycle split (cycle-15 vs cycle-16)

**Recommended split — LOCKED:**
- **Cycle 15 (this cycle): the in-process MCP JSON-RPC server CORE + tool dispatch + wire serializer.**
  `cli_stream/mcp_sidecar.rs` (JSON-RPC `initialize`/`initialized`/`tools/list`/`tools/call`/`ping`, dispatch
  to `NativeToolHandler`) + the Decision-3 `tools/list` `inputSchema` serializer in `native_tools.rs`
  (extending the existing `ToolField`→wire path). **Fully differentially testable WITHOUT a live model**: speak
  JSON-RPC to the in-process server (in-process axum test client or direct handler calls) and diff
  `tools/list` + `tools/call` wire JSON vs the SDK fixtures captured from bun (Decisions 3+4). No CLI, no model.
- **Cycle 16: transport bind + mcp-config write/merge + `send_query` lifecycle wiring** (Decision 5) + the
  argv seam activation (delete the inert warning) + the **live-CLI end-to-end smoke** (env-gated SKIP).

**Justification:** the protocol core is the bulk of the risk and is 100% provable offline against captured SDK
fixtures (it's pure request→response JSON). The transport/lifecycle/CLI-handshake is small but its end-to-end
proof needs a live `claude` binary + auth, which is env-gated. Splitting keeps cycle-15 a fully-green,
differentially-proven unit; cycle-16's live leg degrades to `SKIPPED — env-gated`, never blocking. This mirrors
§6.4's deterministic-core / live-tail split for the argv+parser units.

### Decision 8 — Differential-testability (what cycle-15 proves vs env-gated-skip)

**Cycle-15 proves against live bun (deterministic, the parity gate):**
- `tools/list` wire JSON (incl. `inputSchema` with `$schema`, required-only descriptions, enum key order,
  `execution`, `_meta:{"anthropic/alwaysLoad":true}`) **byte-equal** to the SDK fixture for `manage_run`
  built from the real `INPUT_SCHEMA` (`manage-run-tool.ts:54-89`). Capture: drive the SDK server over the MCP
  in-memory transport (the exact method used to pin these findings) → commit `tools_list.expected.json`.
- `tools/call` happy-path: `{action:'list'}` → `{content:[{type:'text',text:<handler output>}]}` shape
  (the handler output itself depends on DB state — diff the **envelope shape**, with a stubbed handler
  returning a fixed string for byte-parity).
- `tools/call` error envelopes: handler-throw → `{content:[{text:msg}],isError:true}`; bad-enum →
  `isError:true` text result (shape diff; the zod message body is `- [≈]` qualified, Decision 4).
- `initialize` result (`serverInfo`, `capabilities={tools:{listChanged:true}}`) and `ping` → `{}`.

**Genuinely env-gated-skip (cycle-16, cannot be diffed offline):**
- The live `claude` 2.1.x CLI actually *connecting* to the loopback `http` server, reading `tools/list`, and
  the model invoking `mcp__archon__manage_run` end-to-end. Proven by a single smoke gate (run iff
  `CLAUDE_BIN_PATH`/auth present): assert the CLI health-checks the server (`mcp_servers[].status==connected`
  in the stream-json `system:init` line — which the §6.3 parser already reads) and a trivial round-trip
  succeeds; else mark `SKIPPED — env-gated` in the parity ledger, never `PASS`. This is also where the
  `http`-vs-`sse` transport contingency (Decision 1) is confirmed.

**Net:** the protocol core (the no-downgrade proof that `manage_run` is faithfully exposed) is provable
offline against the captured running source; only the CLI-accepts-our-loopback-server handshake is env-gated.
`native_tools=true` is preserved end-to-end — no field, branch, or behavior of the in-process tool is dropped.
