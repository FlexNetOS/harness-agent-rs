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
cycles_this_session: 1
cycles_total: 7
ledger: parity 16/79 units verified (PR-01; WF-01..08, WF-11..14; PA-01 paths, PA-06 env, PA-07 strip-cwd)
last_item: cycle 7 — har-paths PA-01/PA-06/PA-07 — PASS vs live bun (gate caught CLAUDECODE warning-indent divergence, fixed); WF-11 duplicate reconciled into har-paths
status: ITERATE — cycle 7 committed. Next: GI git (har-git) → IS isolation (har-isolation) + PR-02 provider
        registry, to unblock WF-09 dag-executor (the core). PA-02..05 (logger MAP→tracing, telemetry MAP→icm,
        update-check, bundled-build) still TODO in har-paths.
last_update: 2026-06-14T00:45:00Z

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
