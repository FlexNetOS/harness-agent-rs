# Parity Findings — ITERATE Cycle 6 — UNIT WF-14 (model-validation)

**Date:** 2026-06-13
**Verifier:** rust-port-parity-verifier (adversarial, differential, fail-closed)
**Source X:** `/home/drdave/Desktop/meta/Archon` `packages/workflows/src/model-validation.ts` + `dag-executor.ts:136` (routePresetEffort call site) + `defaults/tier-defaults.json` — run under **bun 1.3.14**, Archon v0.4.1.
**Rust port:** `crates/har-dag-executor/src/model_validation.rs`.
**Method:** Live differential. Transient TS oracle (`__parity_wf14_oracle.ts`, now DELETED from Archon) emitted canonical JSON for 67 cases; Rust example `examples/parity_wf14_oracle.rs` emitted the same shape; diffed case-by-case with deep-equal (object-key-order-insensitive). The port's own 51 unit tests were NOT used as the oracle.

## Overall verdict: **PASS** (16/16 symbols `- [x]`, of which 1 is `- [≠]`)

Differential result: **66/67 cases byte-exact**, 1 intentional `- [≠]`. Port had **1 accidental text bug, fixed during verification** (no downgrade waved through).

---

## Per-behavior verdicts

| # | Behavior | Verdict | Evidence |
|---|----------|---------|----------|
| 1 | `resolveModelSpec` — tier branch (direct hit) | **PASS** | claude small/medium/large → haiku/sonnet/opus, exact |
| 1 | `resolveModelSpec` — fallback chain `large→[large,medium,small]` (only medium configured → medium) | **PASS** | `resolve.fallback.large_to_medium` → medium-model, both |
| 1 | `resolveModelSpec` — fallback `medium→[medium,large,small]` (only large → large; over-capable) | **PASS** | `resolve.fallback.medium_to_large` → large-model, both |
| 1 | `resolveModelSpec` — fallback `medium` walks to small (only small configured) | **PASS** | `resolve.fallback.medium_to_small` → small-only, both |
| 1 | `resolveModelSpec` — tier not configured → error | **PASS** | byte-exact: `Tier 'small' has no configured preset … .archon/config.yaml.` |
| 1 | `resolveModelSpec` — `@alias` found | **PASS** | `@fast` → claude/haiku, both |
| 1 | `resolveModelSpec` — `@alias` unknown, lists defined keys | **`- [≠]`** | prefix byte-exact; **key LIST ORDER differs**: Rust sorted, TS insertion. See adjudication. |
| 1 | `resolveModelSpec` — `@alias` unknown, no aliases → `(none)` | **PASS** (after fix) | was `(none).` (stray period) → fixed to `(none)`, now exact |
| 1 | `resolveModelSpec` — literal pass-through (model string, empty string, at-less word) | **PASS** | `{literal}` variant, all 3, exact |
| 2 | `buildAiProfile` — tier-defaults seeding (claude/codex/pi/copilot/opencode) | **PASS** | full alias maps deep-equal across all 5 providers |
| 2 | `buildAiProfile` — unknown provider → no tier defaults | **PASS** | empty aliases, both |
| 2 | `buildAiProfile` — layering: repoTiers beat globalTiers | **PASS** | small → repo-provider/repo-model, both |
| 2 | `buildAiProfile` — layering: repoAliases beat globalAliases | **PASS** | `@x` → sonnet (repo), both |
| 2 | `buildAiProfile` — layering: globalTiers override tier defaults | **PASS** | claude small → codex/gpt-5.5, both |
| 2 | `buildAiProfile` — reject reserved alias name (small/medium/large) | **PASS** | all 3 byte-exact, incl. repoAliases path |
| 2 | `buildAiProfile` — reject alias missing `@` | **PASS** | byte-exact incl. the `(e.g. '@myalias')` hint |
| 2 | `buildAiProfile` — reject empty provider / empty model (alias path) | **PASS** | byte-exact |
| 2 | `buildAiProfile` — reject invalid tier name | **PASS** | `Tier name 'xlarge' is invalid. Supported tiers: small, medium, large.` exact |
| 2 | `buildAiProfile` — reject empty provider/model on **tier** path | **PASS** | TS reports `Alias 'small' has invalid provider…` (validate called with tier key); Rust identical |
| 3 | `TIER_FALLBACK` exact chains | **PASS** | large/medium/small chains byte-exact (unit tests + fallback cases above) |
| 4 | `routePresetEffort` full matrix (4 providers × 7 efforts) | **PASS** | 28/28 exact: claude→`effort` for low/med/high/max; codex→`modelReasoningEffort` for minimal/low/med/high/xhigh; all cross-provider (claude+minimal/xhigh, codex+max, pi/*, unknown/*, empty) → None |
| 5 | `assertNotReserved` | **PASS** | rejects small/medium/large; accepts `@fast`/`myalias` (covered via build_ai_profile reject cases + pub-fn unit tests) |
| 6 | `isLiteralSpec` | **PASS** | true for literal, false for preset, both |
| 6 | `ModelAliasPreset`/`RawAliasEntry` shapes — effort/thinking optionality | **PASS** | effort+thinking object form (`{type:enabled,budgetTokens:1024}`) preserved through resolve, both; absent optionals omitted (serde + TS spread) |
| 7 | `tier-defaults.json` embedded data | **PASS** | embedded const `serde_json`-parsed and `==`-compared to source JSON in Python: **deep-equal**, all 5 providers × 3 tiers × {model,effort?} |

---

## Adjudication of the divergences

### (a) Trailing period on UnknownAlias — ACCIDENTAL PORTER BUG → FIXED
- TS source (model-validation.ts:198): `` `Unknown alias '${ref}'. Defined aliases: ${list}` `` — **no trailing period**.
- Rust port had `#[error("Unknown alias '{alias}'. Defined aliases: {defined}.")]` — a stray `.` after `{defined}`, affecting EVERY UnknownAlias message (visible in both `(none).` and `…small.`).
- This was **not** a deliberate choice — just a template typo. Fixed to byte-exact: `#[error("…Defined aliases: {defined}")]`. Re-verified: now exact. **Not waved through.**

### (b) Sorted vs insertion-order alias-key list — INTENTIONAL `- [≠]`
- Rust lists defined keys SORTED (`@alpha, @zebra, large, medium, small`); TS lists object-insertion order (`small, medium, large, @zebra, @alpha`).
- **Contractuality test (does it matter?):** NO consumer parses the alias-list portion.
  - Callers: `validator.ts:348`, `executor.ts:391`, `dag-executor.ts:414`, `orchestrator-agent.ts:124` — all let the thrown `Error` propagate; `dag-executor` writes `err.message` verbatim into logs/failure-state. No substring/structured match on the list.
  - Source's only test (`model-validation.test.ts:361`) asserts `/Unknown alias '@unknown'/` — a **prefix-only regex** that the Rust message satisfies.
- **Rationale:** Rust `HashMap` iteration is unordered; emitting it unsorted would make the message **non-reproducible across runs** — strictly worse than TS's stable order. Sorting is a behavior-preserving determinism improvement on a display-only, unparsed string.
- **Ruling:** legitimate `- [≠]` (no downgrade). The error prefix and structure are byte-identical; only the order of listed keys differs. Recorded in symbol-map + ledger. Allow-listed (and fail-closed against staleness) in the golden test.

---

## Durable artifacts committed (run in CI without bun)
- `crates/har-dag-executor/examples/parity_wf14_oracle.rs` — Rust differential oracle (67 cases).
- `crates/har-dag-executor/tests/fixtures/wf14_ts_golden.json` — captured bun 1.3.14 golden output.
- `crates/har-dag-executor/tests/wf14_parity_golden.rs` — runs the Rust oracle, diffs vs golden, allows exactly the 1 `- [≠]` (and fails if the allow-list entry is not exercised).

## Cleanup
- Transient TS oracle `Archon/packages/workflows/src/__parity_wf14_oracle.ts` **DELETED**; `git status` on Archon `packages/workflows/` clean — source pristine.

## Ledger items flipping to `- [x]`
- UNIT **WF-14** → `- [x]` (verified).
- symbol-map WF-14: all 16 symbols `- [x]`, except `resolveModelSpec` = `- [≠]`:
  TIER_NAMES, ModelAliasPreset, RawAliasEntry, RawAliasesConfig, RawTiersConfig, ResolvedAiProfile, ResolvedModelSpec, TIER_FALLBACK (tier_fallback_chain), isLiteralSpec, **resolveModelSpec `- [≠]`**, buildAiProfile, routePresetEffort, assertNotReserved (assert_not_reserved_pub), tier-defaults.json embedded, CLAUDE_EFFORTS, CODEX_REASONING_EFFORTS.

## Baseline
- `cargo test -p har-dag-executor`: **255 passed, 0 failed** (incl. new golden test).
- `cargo clippy -p har-dag-executor --all-targets -- -D warnings`: **clean**.
