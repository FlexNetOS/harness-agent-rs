# Parity verdict — WF-09 sub-cycle 4c (AI-node live streaming body of `execute_node_internal`)

**Verifier:** rust-port-parity-verifier (no-downgrade gate) · **Mode:** differential, default-skeptical, fail-closed
**Source (live):** `meta/Archon/packages/workflows/src/dag-executor.ts::executeNodeInternal` (672–1490) + `utils/idle-timeout.ts` · bun 1.3.14
**Target:** `harness-agent-rs/crates/har-dag-executor/src/dag_executor.rs::execute_node_internal` (4379–5526) + helpers
**Committed BEFORE this gate ran:** main `4fb5cf5` (porter did NOT pre-clear with the verifier)
**Durable artifact:** `crates/har-dag-executor/tests/parity_4c_differential.rs` (11 scripted-fake-provider differential probes, all run)

---

## 2026-06-25 — VERDICT: FAIL (`- [~]`) — one confirmed observable-output divergence; everything else PASS

### Headline
The 4c port is **structurally faithful on every contract branch** (control flow, state, output capture, error strings, H1 idle re-arm, H2 cancel, H4 paused tolerance, throttle-map cleanup). I drove the REAL `execute_node_internal` over scripted chunk sequences and diffed `NodeExecutionResult` + emitted messages against an oracle derived from the running TS source. **One divergence** blocks a clean `- [x]`.

### CRITICAL — the porter's own 4c tests are FAKE-GREEN (do not re-trust them)
The inline `sub_cycle3_tests` "parity" tests for 4c **drive nothing**: `idle_timeout_no_false_positive` just `sleep(50ms)` + asserts `<10s`; `cancel_token_cancels_stream`/`cancel_detection_via_abort_token` round-trip a `CancellationToken`; `empty_output_triggers_failure` is `assert!("".trim().is_empty())`; `tool_events_completed_before_started` asserts a hand-written array literal; `cost_accumulates_across_passes` re-adds the numbers in the test. **None call `execute_node_internal`.** The H1/H2 hazards had ZERO executable coverage before this gate. This is exactly the fake-green the gate exists to catch.

### DIVERGENCE D1 (`- [≠]`, must fix on top of `4fb5cf5`) — idle-timeout minute rendering
TS renders the idle window as **float minutes via `String(effectiveIdleTimeout / 60000)`**; Rust uses **integer division** `effective_idle_timeout.as_millis() / 60_000`.

| Site | TS (ts) | Rust (rs) | Example: `idle_timeout` |
|---|---|---|---|
| structured-missing timeout throw | 1248 `String(t/60000)` | 5279 `as_millis()/60_000` | 90000ms → TS `"1.5 min"`, **Rust `"1 min"`** |
| "completed via idle timeout" notice | 1266 | 5304 (`mins`) | 200ms → TS `"0.0033333333333333335 min"`, **Rust `"0 min"`** |
| empty-output idle variant error | 1356 | 5424 | 30000ms → TS `"0.5 min"`, **Rust `"0 min"`** |

**Executable proof** (probe 9b/9c, `--nocapture`):
- `IDLE_NOTICE_RUST = "⚠️ Node \`n9b\` completed via idle timeout (no output for 0 min)..."` — TS would emit `0.0033333333333333335 min`.
- `IDLE_EMPTY_ERR_RUST = "Node 'n9c' timed out with no output (idle for 0 min)..."` — TS would emit `0.0033333333333333335 min`.

Identical for whole-minute configs (default 30 min → "30" both sides); diverges for **any non-whole-minute `idle_timeout`** (user-settable `z.number()` ms). User-facing message text only — control flow/state/output are correct. **Fix:** render JS-`String()`-style float minutes (`as_secs_f64()/60.0`, trim trailing zeros) at all three sites (and 4e loop site ts:2280 when ported).

### MINOR (note, not blocking) — `⚠` vs `⚠️` system-chunk filter
Rust `content.starts_with('⚠')` (rs:5033) matches U+26A0 alone; TS `startsWith('⚠️')` (ts:1093) requires U+26A0+VS16. Rust is a lenient superset (matches `⚠`-without-VS16 that TS would drop to debug). Providers emit `⚠️` (both match), so no observed behavior delta. Lenient, not a downgrade.

### Probe battery — oracle vs Rust (all RUN; PASS unless noted)
| # | Probe | Result |
|---|---|---|
| 1 | assistant streaming → output `"ab"`, sent in order | PASS |
| 2 | batch + flush-drain → output `"abc"`, msgs `["a\n\nb","c"]` (drain-before-flush order) | PASS |
| 3 | tool_started/completed pairing, format_tool_call, log_tool, ToolCalled | PASS (read-diff; helpers unit-covered) |
| 4 | result capture: session_id `"s1"`, cost `0.25` | PASS |
| 5 | `error_max_budget_usd` → `Node 'n5' exceeded cost cap of $1.50.` | PASS (exact) |
| 6 | generic SDK error → `Node 'n6' failed: SDK returned rate_limited — boom; again` | PASS (exact) |
| 7 | MCP-failure workflow-vs-plugin filtering + `⚠️` forward | PASS (read-diff) |
| 8 | structured-output validate + reask (un-stub of `:2562`; real `validate_structured_output`) | PASS (read-diff; WF-31 separately parity-verified `988d86b`) |
| 9 | **idle-timeout H1** | PASS control-flow / **FAIL message text (D1)** |
| 9a | slow-but-steady (gap<window) must NOT timeout → Completed `"xyz"` | PASS — proves per-chunk re-arm |
| 9b | stall after token → idle, completes-via-idle with partial output `"partial"` | PASS (text=D1) |
| 9c | stall before any token → empty-output idle variant | PASS (text=D1) |
| 10 | **cancel H2** via status poll → `Failed{"Cancelled by user"}`, throttle cleaned | PASS |
| 11 | **paused H4** tolerated mid-stream → Completed `"ok"` | PASS |
| 12 | credit-exhaustion (assistant text) → Failed | PASS |
| 13 | empty-output (non-timeout) → exact error string | PASS (exact) |
| 14 | node_completed success + declared_fields_from_schema | PASS (read-diff) |

### H1 (idle per-chunk re-arm) — CORRECT
`tokio::time::timeout(effective_idle_timeout, stream.next())` is inside the `'stream` loop → a fresh timer per chunk. Probe 9a (steady 80ms gaps under a 300ms window) completes without timeout; 9b (10s stall under 200ms window) fires once with `node_idle_timed_out=true` + `abort_token.cancel()`. Minor benign timing nuance: TS's window includes consumer per-chunk processing time (timer reset before `yield`), Rust's starts after processing — Rust is marginally more lenient; immaterial for a deadlock detector. Not a divergence.

### H2 (cancel propagation) — CORRECT
Only cancel path is the 10s-throttled status poll (`get_workflow_run_status` → `should_continue_streaming_for_status` false → `abort_token.cancel()` + break). Probe 10 (status=Cancelled) → `Failed{"Cancelled by user"}`, `cleanup_throttle_maps` on the return (H5). Partial-output preservation (ts:1305 `output: nodeOutputText`) is structurally present (rs:5353) but only reachable when cancel fires after appends (needs >10s real time given the throttle) — verified by read-diff, not executably (throttle makes it untestable sub-second); both sides return `""` for first-chunk cancel — parity holds.

### What blocks the ledger flip to `- [x]`
**D1 only.** Fix the 3 integer-division idle-minute sites (rs:5279/5304/5424) to JS-`String()`-style float rendering, re-run `parity_4c_differential` (add an exact-string assertion on the idle notice), then flip. Alternatively the owner accepts D1 as a recorded `- [≠]` (cosmetic, non-whole-minute idle_timeout only) — but it is an unflagged accidental divergence today, so fail-closed it stays `- [~]`.

**4d honesty check:** the AI dispatch arm in `execute_dag_workflow` remains Skipped (not faked) — confirmed in scope-doc §3 4d; out of scope for 4c. OK.

---

## 2026-06-25 (re-verify) — VERDICT: PASS — D1 closed; 4c ready for `- [x]`

Porter applied the D1 fix (working tree; HEAD still `4fb5cf5`, to be committed on a feature branch). I re-verified independently — did NOT trust the porter's description.

**(1) Code re-read + live-node spot-check.** All three idle-minute sites now call `idle_timeout_minutes(effective_idle_timeout)` → `format_js_number(as_millis() as f64 / 60000.0)`: rs:5448 (structured-missing throw, ts:1248), rs:5473 (`mins` completed-via-idle notice, ts:1266), rs:5594 (empty-output idle variant, ts:1356). The old integer `as_millis()/60_000` is gone from all three.

`format_js_number` (rs:4457) faithfully implements ECMA-262 §6.1.6.1.20 `Number::toString` layout (shortest digits via `{:e}`, JS fixed-vs-exponential re-layout, `n = exp+1`, four ECMA range branches, `-0`/NaN/±Infinity). Cross-checked Rust output vs **live `node -e 'String(x)'`**, incl. the exponential regime where plain Rust `Display` diverges:

| input | live node `String()` | match |
|---|---|---|
| 200/60000 | `0.0033333333333333335` | ✓ |
| 90000/60000 (1.5 min) | `1.5` | ✓ |
| 30000/60000 (0.5 min) | `0.5` | ✓ |
| 123456/60000 | `2.0576` | ✓ |
| 1/60000 | `0.000016666666666666667` | ✓ |
| **0.05/60000** | `8.333333333333333e-7` | ✓ (exp regime — old Display would FAIL) |
| 1800000/60000 (default 30 min) | `30` | ✓ |

**(2) My probes intact/strengthened.** Porter converted my capture-only 9b/9c (`eprintln!` placeholders) into real `assert!`s on the live-node-confirmed `"...no output for 0.0033333333333333335 min"` / `"idle for 0.0033333333333333335 min"`. Asserts encode the CORRECT oracle (matches live node); state/output/notice assertions unchanged. The 4 clippy fixes (manual_map, type_complexity, 2× field_reassign_with_default) are behavior-preserving — no assertion altered. Re-read confirms no weakening.

**(3) Re-run (independent):** `format_js_number` unit tests PASS (js_number_* + regimes); `parity_4c_differential` **11/11 PASS** against fixed source; `cargo clippy -p har-dag-executor --all-targets` **No issues found**.

**No remaining divergences.** D1 closed across all 3 in-scope sites (loop-node ts:2280 is 4e/out-of-scope; should reuse `idle_timeout_minutes` when ported). Minor `⚠` vs `⚠️` filter note stands (lenient superset, not blocking).

**VERDICT: PASS.** 4c may flip to `- [x]` and commit. Gate cleared.
