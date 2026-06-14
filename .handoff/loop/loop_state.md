# Loop state — rust-port (Archon → harness-agent-rs)
session_started: 2026-06-14T00:00:00Z
loop: rust-port
branch: main
worktree: /home/drdave/Desktop/meta/harness-agent-rs
source_root: /home/drdave/Desktop/meta/Archon
source_toolchain: bun        # bun 1.3.14 — parity-verifier runs the TS source
rust_target: /home/drdave/Desktop/meta/harness-agent-rs
dest_repo: (none — port target IS this repo; no separate Y to merge into)
cycle_budget: 3
cycles_this_session: 1
cycles_total: 12
ledger: parity 33/79 units verified — har-isolation COMPLETE; provider registry + Claude sub-units.
        (PR-01/02/04/05/06; WF-01..08, WF-11..14; PA-01/06/07; GI-01..05; IS-01..08).
last_item: cycle 12 — Claude provider sub-units PR-04 binary-resolver + PR-05 config + PR-06 native-tools
           — PASS vs live bun (gate caught INSTALL_INSTRUCTIONS \-continuation indent-strip downgrade, fixed).
status: ITERATE — cycle 12 committed. NEXT = PR-03 ClaudeProvider (the big one) per the ARCHITECT DECISION
        in target-architecture.md §6: spawn `claude` CLI (--print --output-format stream-json) → parse NDJSON
        → MessageChunk. Deterministic parts (build_claude_argv, parse_claude_stream_json, structured-output,
        error/retry) ARE differential-testable (golden stream-json); live model call is env-gated SKIP. Shared
        cli_stream/ helper for all CLI providers. R8 NEEDS-HUMAN: native-tools in-process MCP → SIDECAR (do NOT
        set nativeTools=false without owner [≠]). Then PR-07 codex → PR-09/10/11 community → har-ledger
        (CO db MAP→hf) → WF-09 dag-executor.
last_update: 2026-06-14T07:30:00Z

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
