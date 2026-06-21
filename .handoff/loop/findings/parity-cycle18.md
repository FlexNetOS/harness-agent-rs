# Parity Cycle 18 — GitHub Copilot community provider (PR-11 / symbol-map PR-10)

**Date:** 2026-06-21
**Gate:** rust-port-parity-verifier (differential, fail-closed)
**Oracle:** the LIVE TypeScript source run under bun 1.3.14 (NOT the porter's report/fixtures).
Imported directly from `Archon/packages/providers/src/community/copilot/*.ts` +
`shared/{structured-output,skills}.ts`. Archon left pristine (oracle scripts in /tmp, symlink removed).

**Substrate note:** Copilot wraps the `@github/copilot-sdk` Node SDK (createSession/sendAndWait/
abort/session.on), NOT a CLI subprocess. The porter ported all surrounding logic and left the live
SDK session-binding (provider.ts:520-618 + event-bridge.ts:271-434 `bridgeSession`) as a NEEDS-HUMAN
seam returning a clean `MessageChunk::Result{ is_error:true, error_subtype:"copilot_sdk_not_bound" }`.

---

## OVERALL VERDICT: **FAIL** (ported non-SDK surface) — two structured-output downgrades + one minor wire `[≈]`

The event-bridge mapper, binary-resolver, config, skills, token/env/error-classification, and
capabilities are parity-PASS and byte-exact vs the live source. **But the shared structured-output
helpers carry two genuine, fixable downgrades** that route back to the porter before the unit's
provider symbol can flip. The SDK seam itself is a legitimate NEEDS-HUMAN owner-wall (framed below).

---

## Per-area verdicts

### 1. event-bridge `map_copilot_event` + `normalize_copilot_usage` + `AsyncQueue` — **PASS**
24-event + 5-usage adversarial matrix diffed vs bun (`/tmp/copilot-oracle/eb_oracle.ts`). 0 diffs on:
all 8 event types; `tool.execution_complete` preferring `detailedContent` over `content`; failure
path emitting `⚠️ Tool <name> failed` System chunk **and** `❌ <output>` ToolResult; emoji byte-exact
(`\u{26A0}\u{FE0F}` ⚠️, `\u{274C}` ❌, `\u{2699}\u{FE0F}…` ⚙️ compaction); `assistant.usage` token
capture + None-when-both-absent; `session.error` defers (no chunk) and records msg / `Copilot session
error` fallback (empty msg → fallback); reasoning→Thinking; unknown→ignored (debug log).
- **MINOR `[≈]` (D-tool-input):** `tool.execution_start` with **absent** `arguments` → source emits
  `toolInput: {}` (present empty object; from `args ?? {}`). Rust maps `None` → `tool_input: None`,
  and `MessageChunk::Tool.tool_input` has `skip_serializing_if = Option::is_none` → the field is
  **omitted** from the wire chunk. Observable to any consumer that reads `toolInput`. The porter's own
  test (`tool_execution_start_without_arguments_uses_empty_object`) does NOT distinguish None from
  empty (`map(|m| m.is_empty()) != Some(false)` is true for both) → it masks the gap. **Fix:** emit
  `Some(HashMap::new())` when arguments absent.

### 2. binary-resolver — **PASS**
All tiers + error text byte-exact vs source: dev-mode `None`; env > config > vendor > autodetect >
PATH; env/config not-executable error strings; not-found install-instructions string; empty-string
env/config falls through (matches JS `if (envPath)` / `if (configCliPath)` truthiness). `is_executable_file`
mode&0o111 unix / is-file win32; `resolve_from_path` first-line trim. Covered by the unit suite.

### 3. config `parse_copilot_config` — **PASS**
22-case defensive-parse matrix diffed vs bun (`/tmp/cfg_oracle.ts`): 0 diffs. model/cliPath/configDir
string-typed; enable/useLoggedIn bool-typed; logLevel enum; modelReasoningEffort enum + `max`→`xhigh`
alias; every wrong-typed / missing / extra / null value silently dropped (never throws).

### 4. shared/structured-output — **FAIL (two downgrades)**
Oracle: `/tmp/copilot-oracle/so_oracle.ts` (40-case matrix) + `/tmp/jr_probe.ts` (jsonrepair probe).
Rust matrix from a temp harness over the identical inputs.

  **(a) `try_parse_structured_output` — bidirectional divergence (REFUTES the porter `[≠]`):**

  | input | source (bun) | Rust | dir |
  |---|---|---|---|
  | `{"x": 1} and some trailing prose` | **None** | `{"x":1}` | OVER-accept |
  | `{"a":1} {"b":2}` | **None** | `{"a":1}` | OVER-accept |
  | `note {"a":1} end` | **None** | `{"a":1}` | OVER-accept |
  | `{"x":1}\nFor example: {"y":2}` | **None** | `{"x":1}` | OVER-accept |
  | `{"a":1,}` (trailing comma) | `{"a":1}` | **None** | UNDER-accept |
  | `{'a':1}` (single quotes) | `{"a":1}` | **None** | UNDER-accept |
  | `{a:1}` (unquoted key) | `{"a":1}` | **None** | UNDER-accept |
  | `{"a":1` (truncated tail) | `{"a":1}` | **None** | UNDER-accept |
  | `{"a": "unterminated` | `{"a":"unterminated"}` | **None** | UNDER-accept |

  Mechanism (proven): source tier-3 calls `jsonrepair(region)` then `tryJsonParseObject` (object-only).
  jsonrepair **throws** on `{…} trailing prose` / two-object inputs (`Unexpected character … at
  position N`) → swallowed → `undefined`. So the source's contract for "leading object + trailing
  prose / second object" is **None** (deliberate conservative-failure, documented at
  structured-output.ts:83-87,109-111). The Rust port replaced tier-3 with naive balanced-brace
  slicing, which **accepts** those → returns a value where source returns "unavailable". It also
  drops the legit jsonrepair recoveries (trailing comma etc.).
  - The porter's `- [≠]` ("conservative subset of jsonrepair; only heavily-malformed diverge") is
    **REFUTED**: the divergence is BOTH directions, and the UNDER-accept set includes a plain trailing
    comma `{"a":1,}` — an extremely common model output, not "heavily-malformed". The OVER-accept set
    is the dangerous one: the executor would feed a workflow node structured data the source would have
    failed/re-asked on. **This is a behavior-shifting downgrade, not a defensible divergence → FAIL.**
  - **Fix:** port a jsonrepair-equivalent tier-3 (a Rust json-repair crate or a faithful subset),
    keep the object-only gate, and DROP the naive balanced-brace acceptance so trailing-prose →
    None like the source.

  **(b) `augment_prompt_for_json_schema` — non-deterministic schema key order:**
  Instruction prose is byte-exact vs bun. BUT the embedded `JSON.stringify(schema, null, 2)` block has
  **non-deterministic key order** in Rust because the schema is `HashMap<String,Value>` (observed
  `type, required, properties` vs source insertion `type, properties, required`). The augmented string
  is sent to the LLM as the user prompt. Non-determinism (vs the deterministically-*sorted* WF-14
  `[≠]` precedent) breaks reproducibility/caching and is not a stable divergence. **Fix:** carry the
  schema as order-preserving `serde_json::Value` (workspace already enables `serde_json/preserve_order`)
  end-to-end, or move `OutputFormat.schema` off `HashMap` — the same `HashMap` root the codex port
  already routed around via Value+preserve_order. → **FAIL.**

### 5. skills `resolve_skill_directories` — **PASS**
17-case matrix diffed vs bun (`/tmp/copilot-oracle/skills_oracle.ts`): 0 diffs. Precedence
.agents>.claude>~/.agents>~/.claude; dedup; trim; empty-after-trim skipped silently (not missing);
traversal/nested/absolute/`.`/`..` → missing.

### 6. token + env resolution — **PASS**
`resolve_token_source` byte-matches provider.ts:502-519: copilot-token wins; else
`useLoggedInUser===false` → (generic-token if GH/GITHUB present, else logged-in-user); else
logged-in-user. `build_copilot_env` request-env over process-env; `resolve_copilot_token`
(COPILOT_GITHUB_TOKEN, empty→None); `resolve_generic_github_token` GH_TOKEN > GITHUB_TOKEN. Unit-tested.

### 7. error classification — **PASS**
`is_model_access_error` (model && (not available|not found|unsupported), lowercased) and
`build_friendly_copilot_error` model-access + auth message text are **byte-exact** vs provider.ts:368-414
(indent/newlines included). NOTE: `safe_error_string` is simplified (source JSON-stringifies non-string
throwables; Rust only ever sees a string at the unfilled seam) — `#[allow(dead_code)]`, not on a live
path; **re-verify when the SDK seam lands**.

### 8. COPILOT_CAPABILITIES — **PASS (flags) / HONESTY FLAG (seam)**
All 14 flags byte-exact vs capabilities.ts (re-confirms PR-02). **Honesty:** while the SDK seam is
unbound, `send_query` returns `copilot_sdk_not_bound` for every query, so EVERY advertised `true`
capability (mcp/skills/agents/toolRestrictions/structuredOutput:best-effort/envInjection/effort/
thinking/sessionResume) is non-functional. This is a property of the seam, not a flag mismatch — the
flags correctly mirror the source (which CAN honor them). **Do NOT edit the flags** (that would diverge
from source). The honesty gap belongs to the seam decision below.

---

## The seam — NEEDS-HUMAN (decision-grade owner-wall)

The `copilot_sdk_not_bound` seam is **exactly isolated** to the SDK session lifecycle: provider.rs
`send_query` steps 1-9 (config parse, env merge, token resolve, binary resolve, all translation
warnings, reasoning, structured-output augment, session-config logging, abort check) run faithfully
BEFORE the seam; only step 10 (createSession/resumeSession/sendAndWait/abort + `bridgeSession`) is the
gap. `bridgeSession` (event-bridge.ts:271-434) is correctly NOT ported (the pure mapper IS). **No other
behavior was dropped under cover of the seam** — confirmed by reading the full `send_query` + the source
provider.ts:436-619. (The two structured-output FAILs above are SEPARATE porter bugs in `shared/`, not
hidden behind the seam.)

**Options (mirroring how Claude R8 native-tools was handled — band-aid now, real fix post-port):**
- **(a) Node sidecar** running `@github/copilot-sdk` that the Rust shells out to (analogous to the R8
  loopback band-aid that PRESERVED native-tools). Cost: a process boundary + RPC protocol; preserves
  the full feature incl. streaming/abort/resume. **Most consistent with the R8 precedent.**
- **(b) Ship with the documented SDK-binding seam**, everything-else ported, capability honesty
  enforced (e.g. gate the advertised caps behind "seam bound" at runtime, or document the limitation).
  Cost: provider compiles + registers but cannot serve a live query until (a) or a Rust SDK exists.
- **(c) Explicit capability downgrade `[≠]`** — flip the SDK-dependent caps to false. Cost: diverges
  from source capabilities.ts; only honest if the owner accepts a permanently reduced Copilot.

**Recommendation:** (a) the Node sidecar, consistent with the Claude R8 precedent (band-aid that
preserves the feature now; native Rust SDK if/when one exists). Until then the unit stays `- [~]`.
**This is a genuine NEEDS-HUMAN owner decision — not the gate's to make.**

---

## Carried-forward / other ledger items
- **PR-12 `load_mcp_config` `[≈]`:** the porter reused codex's inline stopgap loader (`resolve_mcp_config`
  in provider.rs). Out-of-unit, does not block PR-10's other symbols; tracked at PR-12.
- **`[≈]` D-tool-input** (event-bridge absent-arguments → omitted vs `{}`): minor wire shape; fix listed.

## Required fixes before the provider symbol → `- [x]` (route back to porter)
1. **structured-output tier-3:** port a jsonrepair-equivalent + object-only gate; drop naive
   balanced-brace acceptance (kills both the OVER- and UNDER-accept divergences).
2. **augment schema order:** carry schema as order-preserving `serde_json::Value`.
3. **(minor) tool.execution_start:** absent arguments → `Some(empty map)` not `None`.
4. **Owner:** decide the SDK seam (recommend option (a)).

## Gate health
- `cargo clippy -p har-provider --all-targets -- -D warnings` — clean.
- `cargo test -p har-provider` — 480 passed / 0 failed / 1 ignored (lib) + suites all green;
  new harness `tests/parity_cycle18_copilot.rs` = 5 passed / 0 failed / 3 ignored (the 3 ignored =
  the documented FAILs as golden assertions that flip green when fixed).
- Durable harness: `crates/har-provider/tests/parity_cycle18_copilot.rs`.

---

# Re-verification — Cycle 18 fixes (a)(b)(c) + blast radius

**Date:** 2026-06-21 (re-verify pass)
**Gate:** rust-port-parity-verifier (differential, fail-closed)
**Oracle:** LIVE TS source under bun 1.3.14 — `tryParseStructuredOutput` / `augmentPromptForJsonSchema`
imported directly from `Archon/packages/providers/src/shared/structured-output.ts`; raw `jsonrepair@3.14.0`
(npm, from Archon node_modules) probed in parallel. Rust run through the actual `har_provider` pub fns.
Archon pristine (0 dirty lines; oracle scripts in /tmp/copilot-reverify, never inside Archon).
**NOT trusting the porter's report/tests** — re-ran my own matrices end-to-end.

## OVERALL RE-VERIFY VERDICT: **PASS** (ported non-SDK surface) — with one precisely-bounded `[≠]` sliver

All three fixes are CLOSED against the live oracle; fix (b)'s contract change introduced **no consumer
regression**. The SDK seam remains NEEDS-HUMAN (unchanged). One genuine, inherent, bounded jsonrepair
library-edge `[≠]` was found and is recorded with exact inputs (below) — it is NOT a porter feature-skip
(the tier was faithfully ported with the only/latest Rust equivalent crate; no config/upgrade closes it).

### (a) try_parse_structured_output tier-3 (jsonrepair-rs v0.2.1) — **CLOSED + bounded `[≠]`**
20-case matrix, Rust full-pipeline vs bun full-pipeline: **0 diffs on all 20.** The previously-FAILing
cases now match exactly:
- RECOVER→object (matches source): `{"a":1,}`, `{'a':1}`, `{a:1}`, `{"a":1` (truncated),
  `{"a": "unterminated` → repaired object.
- THROW→None (matches source): `{"x":1} and trailing prose`, `{"a":1} {"b":2}`, `note {"a":1} end`
  (jsonrepair-rs throws at the SAME positions npm jsonrepair throws — pos 9/8/8).
- array→gate→None (matches source): `{"x":1}\nFor example: {"y":2}` (jsonrepair-rs → array, object-only
  gate rejects → None, exactly as npm jsonrepair → array → gate → None).
- non-object/prose/empty/whitespace → None; valid object / fences / preamble / nested → object — all match.

**Divergence hunt (jsonrepair-rs 0.2.1 vs npm jsonrepair 3.14.0) — TWO genuine, bounded, recorded `[≠]`:**
| exact input | source (bun) FULL | Rust FULL | class |
|---|---|---|---|
| `{"a": NaN}` | `{"a":"NaN"}` (string) | `{"a":null}` | non-finite literal → string vs null |
| `{"a": Infinity}` | `{"a":"Infinity"}` | `{"a":null}` | non-finite literal |
| `{"a": -Infinity}` | `{"a":"-Infinity"}` | `{"a":null}` | non-finite literal |
| `{"a": +1}` / `+1.5` / `+1e3` / `[+1,+2]` | **None** (jsonrepair THROWS) | `{"a":1}` etc. (strips `+`) | leading-`+` over-accept |

Everything ELSE in the divergence-hunt set AGREES (comment_line, block_comment, trailing_garbage_brace,
`{True,None}`→`{true,null}`, `"x"+"y"`→`"xy"`, ObjectId(..)→string, NDJSON→array→None, nested trailing
comma, `undefined`→null, `-1`→`-1`). **Bound:** the only observable disagreements are (1) the three
non-finite numeric literals NaN/Infinity/-Infinity (jsonrepair-rs coerces to `null`, npm to the string
form), and (2) leading-`+` numbers (jsonrepair-rs strips the `+` and accepts; npm throws → None).

**`[≠]` justification (survives the challenge):** This is NOT a portable feature the porter skipped —
the porter PORTED the jsonrepair tier-3 faithfully using `jsonrepair-rs` (the de-facto Rust equivalent;
**0.2.1 is the latest published version**, and its `jsonrepair(&str)` API exposes **no options** to tune
NaN/`+` handling). The residual is an inherent edge-case disagreement between two independent repair
implementations on **pathological, invalid-JSON inputs** (`NaN`/`Infinity`/leading-`+` are values an
instruction-following model would essentially never emit as structured output). No config and no upgrade
closes it. This is exactly the "different repair libs can disagree on edge cases" the verification
anticipated. → **bounded `[≠]`** with the exact inputs above. (If a future Rust crate matches npm's
NaN→string / `+`→throw behavior, re-port and flip to `[x]`.)

### (b) augment_prompt_for_json_schema key order + OutputFormat contract change — **CLOSED, no regression**
1. **Key order:** augmented prompt is now **byte-identical** to bun for schema
   `{type, properties, required}` — emits `type → properties → required` (insertion order), matching
   `JSON.stringify(schema,null,2)`. Instruction prose byte-exact. (`OutputFormat.schema` is now
   order-preserving `serde_json::Map<String,Value>` via the workspace `serde_json/preserve_order` feature.)
2. **BLAST RADIUS — no consumer downgraded.** The two other `OutputFormat.schema` consumers both still
   produce the same wire bytes:
   - **claude/argv.rs:276** — `serde_json::to_value(&of.schema)` → `Value::Object` → `--output-format-schema`
     string. `Map` and `HashMap` both serialize to a JSON object; only key order changes (now
     deterministic/insertion-order — strictly better, never weaker). Wire shape unchanged. Claude argv
     tests green.
   - **codex/provider.rs:680-686** — iterates `fmt.schema` into `Map<_,_>` → `Value::Object` → `--output-schema`
     temp file. `Map.iter()` behaves identically to `HashMap.iter()`. Wire shape unchanged. Codex parity
     suite green (119/119 single-threaded). (Cosmetic: the comment at codex/provider.rs:676 still says
     "HashMap<String, Value>" — stale, harmless; suggest the porter refresh it.)
   - `OutputFormat` overall serde wire shape is unchanged (a JSON object either way); only ordering became
     deterministic. **No previously-PASSing claude/codex parity is broken by the contract change.**

### (c) tool.execution_start absent arguments → `{}` — **CLOSED**
Direct wire diff (absent `arguments`):
- Rust:   `{"type":"tool","toolName":"read","toolInput":{},"toolCallId":"c1"}`
- Source: `{"type":"tool","toolName":"read","toolInput":{},"toolCallId":"c1"}` (`toolInput: args ?? {}`)
**Byte-identical.** `tool_input` is now always `Some(map)` (empty when absent), so `skip_serializing_if`
no longer omits it — present as `{}`, matching source. The MINOR `[≈]` D-tool-input from the first pass
is now closed (no longer an `[≈]`).

## Gate health (re-verify)
- `cargo clippy -p har-provider --all-targets -- -D warnings` — **clean (exit 0).**
- `cargo test -p har-provider` — **487 passed / 0 failed / 1 ignored** (clean on 2 consecutive full runs).
  The 1 ignored = `live_cli_smoke_native_tools_end_to_end` (env-gated; requires CLAUDE_BIN_PATH +
  ANTHROPIC_API_KEY — legitimately gated, unrelated to this cycle).
- `tests/parity_cycle18_copilot.rs` — **8 passed / 0 failed / 0 IGNORED — FULLY LIVE.** The 3 formerly-
  `#[ignore]` golden rows now assert the FIXED behavior and pass (header comment is stale: still narrates
  the old FAIL/ignore state — suggest the porter refresh the doc-comment; the assertions are all live).
- **Test-infra FLAKE (non-blocking, NOT a regression):** `codex::provider::tests::send_query_yields_assistant_and_result`
  failed once under parallel-load (`chunks.len()` async-stream draining via FakeSpawner), then passed
  3/3 in isolation and 119/119 single-threaded, and the full suite passed clean on re-run. It is in codex
  and untouched by the contract change (Map vs HashMap does not affect stream timing). Flagged to the
  porter as a test-hardening item (drain the stream deterministically), NOT a parity failure.

## FINAL VERDICT on the Copilot PORTED SURFACE: **PASS (commit as `- [~]` pending the SDK seam)**
The non-SDK ported surface is parity-proven byte-exact vs the live source across structured-output,
event-bridge, config, binary-resolver, skills, token/env, error-classification, and capabilities. The
provider symbol stays `- [~]` ONLY because of the NEEDS-HUMAN SDK-binding seam (the owner decision), NOT
because of any porter bug — all three porter bugs from the first pass are now CLOSED.

### Restated open items (unchanged by this pass)
- **SDK-binding seam → NEEDS-HUMAN** (owner decision). 3 options: (a) Node sidecar running
  `@github/copilot-sdk` (R8-precedent band-aid — **recommended**, consistent with the Claude R8
  native-tools loopback), (b) ship with the documented seam + capability honesty gate, (c) explicit
  capability `[≠]` downgrade. Until decided, `send_query` returns `copilot_sdk_not_bound` and the
  provider symbol stays `- [~]`.
- **PR-12 `load_mcp_config` `[≈]`** — carried forward (codex inline stopgap loader reused); out-of-unit,
  tracked at PR-12.
- **Bounded `[≠]` (jsonrepair sliver)** — NaN/Infinity/-Infinity → null-vs-string and leading-`+` →
  accept-vs-throw; exact inputs recorded above; inherent to jsonrepair-rs 0.2.1 (latest, no options).
- **New dependency noted:** `jsonrepair-rs = "0.2.1"` added to `har-provider` (workspace dep). Pure-Rust,
  no build script, replaces the naive balanced-brace tier-3. Archon untouched.
