# Loop state — rust-port (Archon → harness-agent-rs)
session_started: 2026-06-13T16:00:00Z
loop: rust-port
branch: main
worktree: /home/drdave/Desktop/meta/harness-agent-rs
source_root: /home/drdave/Desktop/meta/Archon
source_toolchain: bun        # bun 1.3.14 — parity-verifier runs the TS source
rust_target: /home/drdave/Desktop/meta/harness-agent-rs
dest_repo: (none — port target IS this repo; no separate Y to merge into)
cycle_budget: 3
cycles_this_session: 3
cycles_total: 9
ledger: parity 27/79 units verified (PR-01; WF-01..08, WF-11..14; PA-01/06/07; GI-01..05;
        IS-01,04,05,06,07,08). IS-02 WorktreeProvider + IS-03 Resolver remain (next).
last_item: cycle 9 — har-isolation foundation IS-01/04/05/06/07/08 — PASS vs live bun (gate caught a
           Node-join vs Rust-Path::join absolute-path divergence in copyFiles, fixed via node_join()).
status: AT CYCLE BUDGET (3/3 this session) — HAND OFF. Next: IS-02 WorktreeProvider (closes the IS-04
        [≠] panic) + IS-03 Resolver → then PR-02.. provider registry/adapters → WF-09 dag-executor (core).
last_update: 2026-06-14T04:30:00Z

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
