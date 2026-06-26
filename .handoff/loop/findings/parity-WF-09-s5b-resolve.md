# Parity verdict — WF-09 sub-cycle 5B-resolve (G1–G7 + #2 + nuances + #5 coverage)

**Date:** 2026-06-26 · **Verifier:** rust-port parity-verifier (differential, vs live bun 1.3.14)
**HEAD at start:** `b75f1b5` (porter did not commit) · **Verifier did not commit.**
**Source (oracle):** `meta-yard/Archon/packages/workflows/src/dag-executor.ts`
  (`resolveNodeProviderAndModel` TS:390-581, `applyPresetOptions` TS:110-152)
**Rust under test:** `crates/har-dag-executor/src/dag_executor.rs`
  (`resolve_node_provider_and_model` rs:618, `apply_preset_options` rs:1089)

## VERDICT: **PASS** — WF-09 is **DONE-READY**

All seven dropped behaviors (G1–G7) are restored and differentially confirmed; code-review #2 is folded;
the two flagged nuances are adjudicated (nuance 2 was a REAL divergence and was FIXED by the verifier);
the code-review #5 coverage gap is closed with a discriminating, mutation-proven test. Every WF-09 symbol
is now `[x]`/`[≈]`, with zero `[ ]`/`[~]`/`[!]` rows and the `executeDagWorkflow` rollup restored.

Gate after all fixes: `cargo test -p har-dag-executor` → **457 passed / 0 failed**;
`cargo clippy -p har-dag-executor --all-targets -- -D warnings` → **clean**.

## Differential method (oracle built independently, not the porter's reported values)

The internal functions are NOT exported from `dag-executor.ts`, so the oracle was assembled from the
exported runtime data the Rust path actually consumes plus the TS source templates, and cross-checked
against Archon's own bun test suite:
- **Warning strings** — read directly from the TS template literals (TS:434, 505, 525) and confirmed
  byte-identical to the Rust `format!` outputs (incl. the em-dash U+2014 and single/plural agreement).
  Cross-validated against `dag-executor.test.ts` (the live bun suite asserts the same substrings, e.g.
  `:1341` model/provider conflict).
- **Capability registry** — the Rust `har_provider` claude/codex `ProviderCapabilities` were compared
  field-by-field against the REAL `packages/providers/src/{claude,codex}/capabilities.ts` (NOT the test
  mocks): byte-equal. This is load-bearing — the Rust cap-check warnings depend on the real flags.
- **Effort routing** — `CLAUDE_EFFORTS` `['low','medium','high','max']` and `CODEX_REASONING_EFFORTS`
  `['minimal','low','medium','high','xhigh']` + `route_preset_effort` were compared to
  `model-validation.ts:211-241`: byte-equal.
- **Whole-DAG end-to-end** — the warning *delivery* and the cost-omit event are observable only through
  the full executor, so they are gated by the live-bun golden in `tests/cycle9_5_wholedag.rs`
  (golden regenerated from `wholedag-oracle.test.ts` under bun 1.3.14).

## Per-gap result

| Gap | Behavior | Result | Evidence |
|-----|----------|--------|----------|
| G1 | real capability checking (`isSet && !cap`, real `ProviderCapabilities`; effort/thinking use `effort_control`/`thinking_control`; bogus `config_env_vars.is_some()` removed) | **PASS** | cap_checks rows carry the actual flag (rs:760-780); claude/codex caps byte-equal to real `capabilities.ts`; probes `g1_g2`, `g1_supported`, `g2_multiple` |
| G2 | capability warning delivered via `safe_send_message`, byte-matched text | **PASS** | TS:505 template == Rust (single "it/this will be", plural "them/these will be"); probes confirm 1 message, exact bytes |
| G3 | `dag.capability_warning_delivery_failed` on delivery fail | **PASS** | transient FailPlatform → `Ok(false)` → error log + resolve still `Ok` (probe `g3_delivery_failure_does_not_break_resolve`) |
| G4 | model/provider conflict INSIDE the Preset arm (`node.provider && node.provider !== resolved`), delivered + `…conflict_warning_delivery_failed` | **PASS** | rs:658-696; byte-matched TS:434; probe `g4_model_provider_conflict_delivered` |
| G5 | agents+skills collision reserved-ID message via `safe_send_message` | **PASS** | byte-matched TS:525; claude supports agents+skills so ONLY the collision message fires (probe `g5_…`, asserts `msgs.len()==1`) |
| G6 | preset thinking/effort cascade (`apply_preset_options` wired; `route_preset_effort`; route to `nodeConfig.effort` vs `assistantConfig.modelReasoningEffort`) | **PASS** | claude high→`nodeConfig.effort`; codex medium→`assistantConfig.modelReasoningEffort` (nc.effort None); codex max→None→`dag.preset_effort_unsupported` warn+skip; probes `g6_*` |
| G7 | `node_config.hooks` populated (event-keyed shape) | **PASS** | `serde_json::to_value(WorkflowNodeHooks)` → `{"PreToolUse":[…]}`; probe `g1_supported_capability_no_warning` asserts the serialized shape reaches the provider |
| #2 | rs:4316 lossy local match → `workflow_run_status_str(&status)` (Pending→"pending") | **PASS** | shared exhaustive helper reused; confirmed in 5A status-casing area |

### Structural embedding (load-bearing for G6/G7 being observable)
Confirmed the TS `options = { ...baseOptions, nodeConfig, assistantConfig }` (TS:574-578) is faithfully
reproduced: the resolved `nodeConfig`/`assistantConfig` are embedded INTO `base_options`
(`base_options.node_config` / `.assistant_config`, serde `nodeConfig`/`assistantConfig`), and BOTH dispatch
sites consume `resolved.base_options.clone()` (AI rs:4137, loop rs:4427). The top-level
`ResolvedProviderAndModel.{node_config,assistant_config}` are `None` and **read nowhere** → no double-wrap,
no regression. The full nodeConfig build (TS:545-561) is field-faithful; **agents are copied field-by-field**
via `inline_agent_from_def` (rs:591) — a `serde_json` round-trip would silently drop
`disallowedTools`/`maxTurns` due to the wire-name mismatch, so this is correct and necessary.

## Nuance adjudications

1. **wlo.effort/thinking/betas/sandbox arrive `None`** — **FAITHFUL `[≈]` (honest deferral, tracked).**
   In TS the values come from the `workflow` param's `& WorkflowLevelOptions` spread (TS:2763, 2781-2787 —
   workflow-definition level). The Rust `execute_dag_workflow` signature plumbs only `fallback_model`
   (rs:3756-3762), so the other four arrive `None`. The unit's `node.x ?? wlo.x` precedence IS implemented
   and differentially exercised (cap checks + nodeConfig baseline + preset cascade). The gap is in the
   DATA SOURCE — the `execute_dag_workflow` workflow-options signature plumbing / SV-01 outer caller (same
   class as code-review #10) — NOT a defect of the resolve unit. **Cross-ref:** track WLO population on the
   `execute_dag_workflow` signature extension (SV-01).

2. **FATAL platform error mapped to "not delivered → failure log" (`unwrap_or(false)`)** — **REAL
   DIVERGENCE → FIXED by the verifier.** TS `safeSendMessage` rethrows on FATAL (executor-shared.ts:632-634),
   and resolve's `await` has no try/catch (TS:431/502/522), so a FATAL delivery error REJECTS the resolve
   promise → the dispatch catch reports "Node '…' failed before execution: …". The Rust `.unwrap_or(false)`
   swallowed `Err(SafeSendError::Fatal)` → resolve returned `Ok` and the node proceeded — a dropped error
   branch. Fixed at all three delivery sites (G2 rs:811, G5 rs:845, G4 rs:675): a `Err(Fatal(e))` now returns
   `Err("Platform authentication/permission error: {e}")`, which the existing dispatch `Err`-handler renders
   as the matching "failed before execution" node-failure. Transient/below-threshold errors still log the
   delivery-failure and continue (resolve passes no `UnknownErrorTracker`, so only FATAL rethrows — matching
   TS). New probe `g3_fatal_delivery_propagates_as_resolve_err` (FatalPlatform → `expect_err` containing the
   prefix). Discriminating by construction: before the fix the `.unwrap_or(false)` returns `Ok`, failing the
   `expect_err`.

## Code-review #5 — cost-omit coverage (the headline gap) — ADDED

The 5A whole-DAG differential scripted every completing AI node with `cost: Some(0.01)`, so fix #7 (cost_usd
OMIT-when-absent) had zero discriminating coverage. **Closed:** a 4th isolated workflow `costomit` was added
to BOTH the bun oracle (`wholedag-oracle.test.ts`) and the Rust differential (`tests/cycle9_5_wholedag.rs`)
— a single completing AI node whose Result OMITS `cost`/`stop_reason`/`num_turns`. The golden was
regenerated from live bun (now omits `cost_usd`: the node_completed data is `{duration_ms, node_output}`).
The Rust test adds an explicit guard (`node_completed` for the cost-omit node must have no `cost_usd` key)
plus the golden field-diff. **Proven discriminating:** temporarily reverting rs:6443 to
`completed_data.insert("cost_usd", json!(node_cost_usd_pass.unwrap_or(0.0)))` made the gate FAIL
(`…got: {…,"cost_usd":0.0}`); reverted. (model_usage limb of #6 and mcp__/absent-field tool briefs remain
the must-have-adjacent items noted in #5; the cost-omit must-have is delivered.)

## Rows flipped (symbol-map.md)

- `resolveNodeProviderAndModel` → **`[x]`** (PARITY-VERIFIED; fatal-path fixed)
- `applyPresetOptions` → **`[x]`** (PARITY-VERIFIED; G6 cascade)
- `WorkflowLevelOptions` → **`[≈]`** (struct + precedence faithful; population deferred to SV-01, tracked)
- `scheduleReask` → **`[≈]`** (phantom target corrected; behavior inlined, 4c-covered)
- `NodeExecutionResult` → **`[x]`** (real struct rs:4729, 4c/4d-covered)
- `NodeState` → **`[x]`** (re-cited to WF-06; Rust enum mirror rs:4748)

parity-ledger.md: WF-09 5B-resolve row added (PASS, DONE-READY); stale error-path `- [ ]` boxes (lines
246, 249-258) reconciled to `[x]`/`[≈]` with the verifying-cycle citation.

## DONE-READY determination

**WF-09 = DONE-READY.** All WF-09-owned symbols are `[x]` or `[≈]` (each `[≈]` is an idiom/honest-deferral
with rationale + cross-ref, no `[≠]`), zero `[ ]`/`[~]`/`[!]`, and the `executeDagWorkflow` rollup is
restored (it now calls a verified `[x]` `resolve_node_provider_and_model`). One tracked follow-up remains
OUTSIDE WF-09's unit boundary: the `execute_dag_workflow` workflow-level option population
(effort/thinking/betas/sandbox), owed by the SV-01 outer-caller port.
