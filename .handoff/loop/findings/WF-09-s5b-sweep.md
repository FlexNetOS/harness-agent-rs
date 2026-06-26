# WF-09 Sub-cycle 5B — pre-DONE left-behind sweep (completeness gate)

**Date:** 2026-06-25 · **Scope:** WF-09 (the DAG executor) ONLY.
**Source:** `meta-yard/Archon/packages/workflows/src/dag-executor.ts` (Archon v0.4.1, 3710 lines).
**Rust target:** `harness-agent-rs/crates/har-dag-executor/src/dag_executor.rs` (8379 lines) + `executor_shared.rs`.

## VERDICT: **NOT-READY** (fail-closed)

WF-09 left real behavior behind. The original harvest under-counted the symbol surface (per the cycle-36
NOTE), and the missed `resolveNodeProviderAndModel` + `applyPresetOptions` functions hide a cluster of
**stubbed / dropped behaviors** that NO sub-cycle gate (4d/4e/4f/5A) targeted — because the probes used
provider/model configs that never trigger a capability mismatch or a preset cascade. These are in the
**live executing path** of every AI / Loop / Approval node (`resolve_node_provider_and_model` is called at
dag_executor.rs:3851 + :4129).

## Harvest method (deterministic, code-intelligence — not grep)

1. `git kb code symbols --json --limit -1 --file packages/workflows/src/dag-executor.ts` (git-kb 0.2.10).
2. **The git-kb index was STALE** — indexed 2026-06-05, `file_content_hash` mismatched the current file, and
   symbol line-ranges stopped at 3495 vs the file's actual 3710 lines (returned only 24 symbols). **Re-indexed**
   (`git kb code index <file>`) → 28 fn/interface/type symbols; fresh line ranges now match every ledger citation
   (executeNodeInternal 672-1490, executeDagWorkflow 2753-3710, etc.). A zero/stale harvest is NOT a vacuous pass.
3. git-kb's TS indexer emits only `function`/`interface`/`type_alias` — it does **not** emit `const`/`let`. The
   8 constants + 3 module caches were enumerated by column-0 grep (`^(export )?(const|let|...)`). 

**Full WF-09-owned source surface = 28 fn/iface/type + 8 const + 3 module cache.**

## Reconciliation table (source symbol → symbol-map → Rust target → status)

### Matched & already `- [x]` (no action)
18 prior rows: 8 constants (s1 byte-match), McpFailureEntry, parseMcpFailureServerNames,
loadConfiguredMcpServerNames, shouldContinueStreamingForStatus, substituteNodeOutputRefs, checkTriggerRule,
buildTopologicalLayers, executeNodeInternal (4c), executeBashNode (4a), executeScriptNode (4b),
executeLoopNode (4e), executeApprovalNode (4f), buildReaskPrompt (4c), emitReask (4c), executeDagWorkflow (rollup).

### MISSING rows — had NO symbol-map entry (the dangerous class). NOW ADDED to symbol-map.
| Source symbol | Rust target | Verdict |
|---|---|---|
| shellQuote | shell_quote (rs:195) | `[x]` covered s1/4a |
| shellQuoteOrFile | shell_quote_or_file (rs:203) | `[x]` covered s1 |
| getEffectiveNodeRetryConfig | get_effective_node_retry_config (rs:507) | `[x]` covered 4d |
| isTransientNodeError | is_transient_node_error (rs:549) | `[x]` covered 4d |
| runStreamPass (nested) | inlined `'reask: loop` (rs:5249-5975) | `[x]` covered 4c |
| skipIfStatusChanged (nested) | skip_if_status_changed (rs:4393) | `[x]` covered 5A |
| lastNodeCancelCheck (cache) | last_cancel_check() (rs:4699) | `[x]` covered 4c (H2) |
| lastNodeActivityUpdate (cache) | last_activity_update() (rs:4702) | `[x]` covered 4c |
| getLog | Rust tracing macros | `[≈]` idiom |
| cachedLog (cache) | N/A | `[≈]` idiom |
| logEventStoreError (nested) | inlined warn! (loop) | `[≈]` idiom, covered 4e |
| **resolveNodeProviderAndModel** | resolve_node_provider_and_model (rs:567) | **`[!]` PARTIAL — see gaps** |
| **applyPresetOptions** | apply_preset_options (rs:872, dead) | **`[!]` DEAD/STUB — see gaps** |
| **WorkflowLevelOptions** (iface) | threaded fields | **`[!]` behind resolve `[!]`** |

### Unverified rows still `- [~]` (block DONE) + corrections
| Source symbol | Issue | Resolution owed |
|---|---|---|
| scheduleReask | **PHANTOM TARGET** `schedule_reask()` does not exist; behavior INLINED at rs:5891-5904 (TS:1227) + rs:5930-5943 (TS:1241), covered 4c probe 8 | verifier flip → `[≈]` (closure inlined) |
| NodeExecutionResult | real target struct rs:4729; covered 4c/4d | verifier flip → `[x]` |
| NodeState | **WRONG CITATION** — 0× in dag-executor.ts; is WF-06's enum mirrored as Rust enum rs:4748 | verifier flip → `[x]`/`[≈]` after re-citing WF-06 |

### Phantom targets found: 1 (`schedule_reask` — behavior inlined, row target wrong).
### Rollup note: `executeDagWorkflow` `- [x]` was granted on dispatch-exhaustiveness, but it **calls**
`resolve_node_provider_and_model` (now `[!]`) → the rollup is technically violated until the resolve gaps close.

## GENUINE BEHAVIORAL GAPS (the gate failures) — all in resolveNodeProviderAndModel / applyPresetOptions

- **G1 — capability checking stubbed.** Rust ignores actual provider caps: `_caps` is bound-and-unused
  (rs:650); `cap_checks` has no capability flag and warns on EVERY set field (rs:696 "simplified — real impl
  checks caps fields"). TS warns only when `isSet && !caps[cap]` (TS:471-498). Result: FALSE capability
  warnings. The effort/thinking checks use the wrong stand-in `config_env_vars.is_some()` (rs:677-682).
- **G2 — capability-warning USER delivery dropped.** TS delivers via `safeSendMessage` + logs delivery
  failure (TS:500-511). Rust is `warn!`-log-only.
- **G3 — capability-warning delivery-failure error log dropped** (part of G2 path).
- **G4 — model/provider-conflict USER delivery dropped.** TS:421-440 delivers via `safeSendMessage`
  (+ `dag.model_provider_conflict_warning_delivery_failed` log). Rust `warn!`-only (rs:610 comment:
  "Warning delivery would require platform — skip in this utility-only scope").
- **G5 — agents+skills-collision USER message dropped.** TS:516-528 delivers a specific reserved-ID warning
  via `safeSendMessage`. Rust `warn!`-only (rs:712).
- **G6 — preset thinking/effort cascade not applied.** `apply_preset_options` is dead code; the inline
  path stubs it (`let _ = thinking`, rs:766-773). The TS preset cascade (TS:110-152) never runs for any node.
- **G7 — node hooks serialization deferred.** `node_config.hooks` never populated (rs:754 "deferred to
  sub-cycle 3"); node-level `hooks:` is silently dropped from the provider call.

## Sibling-unit deferrals (correctly owned elsewhere — NOT WF-09 gaps, cross-referenced so not forgotten)

- Web `send_structured_event` SSE override — owed by **WF-32 (deps.ts) / SV-03 (web.ts)**, both `- [ ]`.
  Confirmed faithful no-op default in 5A; the recording platform proved the seam is wired. Not a WF-09 gap.
- WF-15 event-emitter message-slot fidelity (the `[≈]` adjudications in 4e/4f) — owed by **WF-15**
  (`event-emitter.ts`, all `- [ ]`). Store events carry full fidelity. Not a WF-09 gap.
- NOTE: G2/G4/G5 (warning USER delivery) depend on `safe_send_message` (WF-11, `- [x]`) + the WorkflowPlatform
  seam (built 4a) — both ALREADY available. So these are WF-09's own port-incompleteness, NOT blocked on an
  unported sibling. They are honestly trackable and fixable now.

## Unit-grain (parity-ledger.md)

Sub-cycles 1, 2, 3, 4a, 4b, 4c, 4d, 4e, 4f, 5A are `- [x]`. **5B (this sweep) = NOT-READY.**
Additionally, the WF-09 unit body (parity-ledger.md:215-258) carries stale `- [ ]` checkboxes that duplicate
now-verified items AND the genuine `- [ ]` error-path items G1-G7 (lines 249-258: unknown-provider /
model-conflict / agents+skills collision / capability warnings). Those error-path `- [ ]` are REAL and match
this sweep's findings — they are not merely stale. Reconcile that block when G1-G7 land.

## What must happen before WF-09 → DONE

1. Implement G1-G7 in `resolve_node_provider_and_model` (+ wire `apply_preset_options`): check `caps[cap]`,
   deliver the 3 warning classes via `safe_send_message` (thread platform/conversationId or relocate to the
   dispatch), apply the preset thinking/effort cascade, serialize hooks into node_config.
2. Parity-verifier: differential-gate G1-G7 (provider missing a used capability; node with hooks; preset with
   thinking/effort; agents+skills collision; model/provider conflict) vs live bun.
3. Parity-verifier: flip the 3 `- [~]` rows (scheduleReask→`[≈]`, NodeExecutionResult→`[x]`, NodeState→`[x]`
   after re-citing WF-06) using the corrections recorded in symbol-map.
4. Re-run 5B: confirm zero missing / zero `- [ ]`/`- [~]`/`- [!]` / zero rollup violations → DONE-READY.
