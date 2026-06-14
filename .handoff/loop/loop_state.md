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
cycles_total: 6
ledger: parity 13/79 units verified (PR-01; WF-01..08, WF-11 executor-shared, WF-12, WF-13, WF-14 model-validation)
last_item: cycle 6 — WF-14 model-validation (har-dag-executor/model_validation.rs) — PARITY-VERIFIED PASS vs bun 1.3.14 (66/67 byte-exact; 1 intentional - [≠] sorted alias list; 1 porter bug fixed: stray trailing period)
status: ITERATE — cycle 6 committed (WF-14 verified [x]); next = WF-09 dag-executor (core state machine)
last_update: 2026-06-13T23:20:00Z

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
