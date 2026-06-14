# Parity — ITERATE cycle 13 (PR-03 ClaudeProvider DETERMINISTIC CORE)

Unit: `cli_stream/` helpers + `build_claude_argv` (argv.rs) + `parse_claude_stream_json` (parser.rs).
Source X: `meta/Archon/packages/providers/src/claude/provider.ts` (+ `mcp/config.ts`, `claude/config.ts`).
Method: DIFFERENTIAL — live `bun` 1.3.14 verbatim-copied source functions ⇄ Rust. Transient oracle
recreated under `Archon/__parity_oracle_c13/` and **deleted** after capture (Archon source pristine,
provider.ts never edited). Durable fixtures committed under
`crates/har-provider/tests/fixtures/claude/{stream,argv,clistream}/`.

## VERDICT: FAIL (1 real downgrade in `build_claude_argv`) — everything else PASS/QUALIFIED

The parser core (incl. the load-bearing `is_error+success` reclassification), all cli_stream
deterministic helpers, and 21/23 argv scenarios are byte-equal to the live TS. **One real
argv divergence** (single root cause, 2 scenarios) is a wrong CLI invocation → routes back to porter.

---

## 2026-06-14 — Differential results

### A. `parse_claude_stream_json` (parser.rs) — PASS (20/20, true differential)

Fed canned NDJSON through BOTH the verbatim TS `streamClaudeMessages`+`normalizeClaudeUsage`
(provider.ts:633-767, 64-79) and the Rust parser; diffed the serialized `MessageChunk` stream
field-by-field. Test: `tests/parity_cycle13.rs` (20 cases). Fixtures+goldens:
`tests/fixtures/claude/stream/*.{ndjson,json}`.

| Case | Branch | Result |
|---|---|---|
| 01 assistant text | text block → Assistant | PASS |
| 02 assistant tool_use | tool_use → Tool{name,input,id} | PASS |
| 03 assistant multi-block | text+tool+text → 3 chunks in order | PASS |
| 04 assistant empty text | empty `text` skipped (TS `block.text` falsy) | PASS |
| 05 system/init failed MCP | only status≠connected → System chunk; connected filtered | PASS |
| 06 system/init all connected | no chunk | PASS |
| 07 system/init no servers | no chunk | PASS |
| 08 system non-init | no chunk | PASS |
| 09 rate_limit_event | → RateLimit{rate_limit_info} | PASS |
| 10 rate_limit no info | → RateLimit{ {} } | PASS |
| 11 result success full | session_id+tokens(+total)+structuredOutput+cost+stopReason+numTurns+modelUsage(raw keys) | PASS |
| **12 result is_error:true subtype:success** | **THE LOAD-BEARING CASE** → clean success: `isError`/`errorSubtype`/`errors` ALL omitted; `stopReason:stop_sequence` kept; the `errors:[...]` correctly DROPPED | **PASS** |
| 13 result real error | is_error:true + non-success subtype → isError:true + errorSubtype + errors[] | PASS |
| 14 user tool_result | CLI user-line → ToolResult{output, toolCallId}; toolName="unknown" | PASS |
| 15 user tool_result content-array | text blocks joined | PASS |
| 16 interleaved | tool → tool_result → assistant → result ORDER preserved (drain-between-events == CLI inline-emit position) | PASS |
| 17 result partial usage (input only) | `tokens` omitted (normalize → None); bare `{type:result}` | PASS |
| 18 unknown event | no chunk | PASS |
| 19 user no tool_result | no chunk | PASS |
| 20 user tool_result >10k | truncate to 10000 + "..." = 10003 chars | PASS |

`normalize_claude_usage` (provider.ts:64-79): input+output both required (→None if either absent/
NaN), total optional — verified via #11/#17 + 5 in-module unit tests. PASS.

### B. cli_stream deterministic helpers — PASS / QUALIFIED

Verbatim TS (`classifySubprocessError` 116-125, `classifyAndEnrichError` 775-812, stderr callback
538-559) ⇄ Rust. Test: `tests/parity_cycle13_clistream.rs` (17 classify + 10 enrich + 12 stderr
cases). Fixtures: `tests/fixtures/claude/clistream/{scenario,expected}.json`.

- **`classify_subprocess_error`** — PASS (17/17). rate_limit (rate limit/too many requests/429/
  overloaded), auth (credit balance/unauthorized/authentication/invalid token/401/403), crash
  (exited with code/killed/signal/operation aborted), unknown; case-insensitive; stderr-only match;
  precedence rate_limit>auth>crash. 0 divergences.
- **`classify_and_enrich_error`** — `message` + `should_retry` PASS (10/10, load-bearing). Abort
  precedence: aborted+"produced no output within"→preserve timeout msg (no retry); aborted+other→
  "Query aborted"; else classify+enrich. Auth inline `(stderr)`; general `(stderr: …)`; retry only
  rate_limit|crash.
  - **QUALIFIED (logging-label only):** TS labels the two abort paths `errorClass:'timeout'`/
    `'aborted'`; Rust `ErrorClass` has only {RateLimit,Auth,Crash,Unknown} → returns `Unknown`. Verified
    at the call site (provider.ts:960-982) `errorClass` feeds ONLY `getLog()`, never control flow, and
    the label never reaches any user-facing message (abort paths return raw/"Query aborted"). No
    behavioral downgrade. Recorded as `- [≠]`-eligible logging difference; message+retry identical.
- **`classify_stderr_line`** — PASS (12/12). Info-banner (Spawning Claude Code / --output-format /
  --permission-mode) overrides error; error keywords (error/fatal/failed/exception, "at ", "Error:")
  case-insensitive; banner-wins-on-collision case verified. 0 divergences.
- **`with_first_message_timeout`** (provider.ts:160-197) — first-event timeout → cancel token +
  #1067 Timeout error; fast-first→Some; empty→None. Covered by 3 in-module tokio tests. PASS
  (Rust-idiom port of the AbortController race; no TS byte-oracle — proven by contract + unit).
- **`NdjsonStream`** line-framing (architect §6.6, no SDK byte-oracle exists — SDK path read objects
  not bytes; this is the CLI-mode framing contract) — \r\n strip, empty-line skip, invalid-JSON→Err,
  partial last line, **non-UTF8 skip** (added durable test `non_utf8_line_is_skipped_not_errored`).
  PASS by spec-conformance + 7 unit tests.

### C. `build_claude_argv` (argv.rs) — FAIL (2 scenarios, 1 root cause) + 21 PASS

TS source builds an SDK `Options` OBJECT; Rust builds the argv VECTOR. Oracle built the REAL Options
via verbatim `buildBaseClaudeOptions`+`applyNodeConfig`, then applied the §6.2 SDK-option→CLI-flag
table (encoded independently of the Rust impl) to derive expected argv. Compared by (flag,value)
pair-set + bare-flag set + warning-code set + `--no-env-file`-precedes-`--print`. Test:
`tests/parity_cycle13_argv.rs` (23 scenarios). Fixtures: `tests/fixtures/claude/argv/*.{json,expected.json}`.

**PASS (21/23):** plain transport flags; model (request>defaults); fallback-model (request>node);
max-budget (`5.0`→"5"); resume+fork; setting-sources default `project,user` + custom `user`;
system-prompt string; **preset+append → `--append-system-prompt`** (bare preset = no flag, CLI
default); effort+thinking(json)+sandbox(json)+betas(csv, empty→omit); output-format-schema (node +
request source); mcp-config + `mcp__<srv>__*` wildcards; **mcp missing-env warning (deduped)**;
**mcp haiku warning**; skills→`--agents`+`--agent`+Skill-in-allowed-tools; inline agents;
**inline-agents-override-skills (user wins on `dag-node-skills` id collision)**; JS cli→`--no-env-file`
before `--print`; native cli→no `--no-env-file`; node-config systemPrompt overrides request.
Permission-mode always `bypassPermissions --dangerously-skip-permissions`. Warnings 0-divergence.

**FAIL (a12, a17) — `nodeConfig.allowed_tools` wrongly emitted as `--allowed-tools`:**

Root cause: TS `applyNodeConfig` maps `nodeConfig.allowed_tools` → **`options.tools`** (provider.ts:
282-284), which is the *agent tools roster* (it flows into the skills `agentDef.tools`, 360-361) — it
is **NOT** `options.allowedTools` and is **NOT** emitted as `--allowed-tools`. `options.allowedTools`
(the CLI permission allowlist → `--allowed-tools`) is built ONLY from MCP wildcards (324) + `Skill`
(367) + the native-tools sidecar wildcard (927). The Rust `argv.rs:268-271` seeds `allowed_tools`
(→`--allowed-tools`) directly from `node_config.allowed_tools`, conflating the two distinct SDK fields.

- **a12** (`allowed_tools:["Bash","Edit"], denied_tools:["WebFetch"]`):
  - TS expected argv: `… --disallowed-tools WebFetch` (NO `--allowed-tools` — `Bash,Edit` went to
    `options.tools`, which has no CLI flag and no skills agent to receive it).
  - Rust actual: `… --allowed-tools Bash,Edit --disallowed-tools WebFetch`. **Extra/wrong flag.**
- **a17** (`skills:["skill-a"], allowed_tools:["Bash"]`):
  - `--agents` JSON identical on both (agentDef.tools=`["Bash","Skill"]` ✓).
  - TS expected `--allowed-tools Skill` (only Skill added to `options.allowedTools`).
  - Rust actual `--allowed-tools Bash,Skill` (Bash leaked into the permission allowlist). **Wrong value.**

Impact: a wrong `--allowed-tools` changes the CLI permission allowlist → real downgrade (over-permissive
or mismatched tool gating). Classified **FAIL**, not QUALIFIED.

**Note on the §6.2 table:** the table row `allowed_tools→tools … --allowed-tools` is itself ambiguous —
it conflates `options.tools` (agent roster, no direct CLI flag in the source) with `options.allowedTools`
(`--allowed-tools`). The faithful behavior: `nodeConfig.allowed_tools` populates the agent roster only;
`--allowed-tools` = MCP wildcards + Skill + sidecar wildcard. Porter fix + (optional) table clarification.

#### PORTER FIX REQUIRED (argv.rs)
- Do NOT seed `--allowed-tools` from `node_config.allowed_tools`. Use a separate `tools` (agent-roster)
  variable for the skills `agentDef.tools`, fed from `node_config.allowed_tools`. `--allowed-tools` must
  accumulate only: MCP `mcp__<srv>__*` wildcards, the sidecar `mcp__archon__*`, and `Skill` (when skills
  present). Re-run `tests/parity_cycle13_argv.rs` — a12 must emit NO `--allowed-tools`; a17 must emit
  `--allowed-tools Skill`. (The agentDef.tools JSON is already correct; only the permission flag is wrong.)

### D. "Options with no CLI flag" — confirmed SDK/orchestration-level, NOT silent argv drops
- `persistSession` (527-529): SDK Options only; no `--persist-session` in §6.2; Rust correctly omits.
  Flag `- [!]` for cycle-14 send_query (confirm CLI persistence default). Not an argv drop.
- `hooks`: §6.2 → `--settings` file (caller-written), not argv. Rust documents the seam. Correct.
- `env`: child-process env (515,867), not argv. Rust takes env separately. Correct.
- `systemPrompt.excludeDynamicSections` (types.ts:233): never read in provider.ts; source emits no flag
  either. Rust notes the seam (argv.rs:180). Not a drop relative to source. `- [!]` follow-up (forwarded
  inside preset object in SDK mode; confirm CLI handling in cycle-14).

---

## Symbol roll-up (deterministic-core sub-unit of PR-03)

PASS → `- [x]`:
- parser.rs `parse_claude_stream_json`, `parse_claude_stream_json_line`, `normalize_claude_usage`,
  `parse_user_tool_result`, `RawUsage`, `ToolResultEntry`
- cli_stream::retry `classify_subprocess_error`, `with_first_message_timeout`, `accumulate_stderr_lines`,
  `ErrorClass`, `RetryConfig`, `FirstEventError`, `RetryError`
- cli_stream::stderr `classify_stderr_line`, `StderrClass`
- cli_stream::stream `NdjsonStream`, `StreamError`

QUALIFIED → `- [≠]` (no downgrade, documented):
- cli_stream::retry `classify_and_enrich_error` / `EnrichedError` — abort-path error-class label
  (`timeout`/`aborted`→`Unknown`) is logging-only; message+retry exact.

FAIL → stays `- [~]` (blocks the unit until porter fix):
- claude::argv `build_claude_argv` — `nodeConfig.allowed_tools`→`--allowed-tools` conflation (a12,a17).
  `ProviderWarning`, `TRANSPORT_FLAGS` themselves verified; the function is unverified until the fix.

**Unit PR-03 deterministic-core: NOT done.** 21/23 argv + 20/20 parser + cli_stream all green, but the
one argv FAIL is a real CLI-invocation downgrade. Cycle does NOT commit `build_claude_argv` as `- [x]`.

---

## 2026-06-14 — RE-VERIFY (porter fix applied) — argv FINAL VERDICT: PASS

Porter fixed the `allowed_tools` conflation. Re-ran the argv differential (live bun 1.3.14 ⇄ Rust).
Transient oracle (`Archon/__parity_oracle_c13/argv_oracle.ts`) rebuilt from the **pristine** verbatim
`buildBaseClaudeOptions`+`applyNodeConfig` branches, re-derived a12/a17 expected argv, diffed against
the committed fixtures, then **deleted** (Archon source never touched; verified clean after).

### Fix confirmed (argv.rs:266-273, 282-321, 343-414)
- `nodeConfig.allowed_tools` → a separate `agent_roster_tools` var (provider.ts:282-284 = `options.tools`,
  the agent roster — **no direct CLI flag**). It now flows ONLY into the skills `agentDef.tools`
  (`[...roster, "Skill"]`, provider.ts:360-361).
- `--allowed-tools` (`permission_allowlist`) is assembled **ONLY** from: MCP `mcp__<srv>__*` wildcards
  (324) + `Skill` when skills present (367) + the R8 sidecar `mcp__archon__*` wildcard (927).
  `node_config.allowed_tools` no longer seeds it.

### Live-bun ⇄ Rust differential results
1. **a12** (`allowed_tools:[Bash,Edit], denied_tools:[WebFetch]`, no skills/MCP):
   live TS derives `… --disallowed-tools WebFetch` with **NO `--allowed-tools`** (Bash,Edit → `options.tools`
   roster, which has no CLI flag and no skills agent to receive it). Rust emits the same. **PASS** —
   oracle assert "NO --allowed-tools on TS side" → PASS; Rust test `allowed_denied_tools` → ok.
2. **a17** (`model:claude-opus-4, skills:[skill-a], allowed_tools:[Bash]`):
   live TS derives `--allowed-tools Skill` (only `Skill`, NOT `Bash,Skill`) **and** `--agents` JSON with
   `agentDef.tools=["Bash","Skill"]`. Rust emits both **byte-for-byte identical** (incl. the full `--agents`
   JSON). **PASS** — oracle asserts `--allowed-tools=='Skill'` → PASS and `agentDef.tools==['Bash','Skill']`
   → PASS; Rust test `skills_with_tools` → ok. Oracle overall: **PASS — fixtures faithful to live TS**.
3. **Full matrix (23/23):** `cargo test parity_cycle13_argv` → **23 passed; 0 failed**, deterministic across
   6 consecutive runs. No regression on any other flag (model/fallback/maxBudget/resume/fork/setting-sources/
   sysprompt-preset-append/effort/thinking/sandbox/betas/output-format-schema/mcp+wildcards/haiku+missing-env
   warnings/skills→agents/inline-agents-user-wins/no-env-file/native-vs-js). Warning-code sets unchanged.
4. **parser + cli_stream (the sides that already PASSED): NO regression.** `parity_cycle13` 20/20,
   `parity_cycle13_clistream` 3/3 (table-driven 17+10+12 cases), 193 lib unit tests, cycle11 3, cycle12 43 —
   all green. Clippy `-p har-provider --tests` clean.

### Harness defect fixed in passing — flaky a11 (`output_format_node_config`)
During re-verify, `a11_output_format` proved **FLAKY** (pass/fail by run). Root cause: `NodeConfig.output_format`
is `HashMap<String, Value>` and serde_json is built workspace-wide with **`preserve_order`**, so the emitted
`--output-format-schema` JSON key order is **non-deterministic** (HashMap seed). The harness `canon_pair`'s
reparse+`.to_string()` was a **no-op** under `preserve_order` (it preserved each side's existing order rather
than canonicalizing). Fixed `canon_pair` to **deep-sort JSON object keys** before comparison
(`parity_cycle13_argv.rs::deep_sort_json`). Key order is semantically irrelevant for the JSON-Schema/config
objects these flags carry, so sorting is a sound canonicalization, not a relaxation. a11 now passes
deterministically (6/6 runs). NOTE: this is a *test-harness* non-determinism, not a behavioral downgrade —
the CLI receives a semantically-identical schema either order; the source (TS) has the same key-order
freedom. Recorded so the same trap doesn't recur for `--agents`/`--thinking`/`--sandbox`.

### Untested-combination note (not a downgrade; no source scenario exercises it)
TS assembles `options.allowedTools` as `[...mcpWildcards, 'Skill']` (MCP block precedes skills block);
Rust appends in the order `['Skill', ...mcpWildcards]` (skills block precedes MCP block). The 23 scenarios
**never combine skills + MCP**, so this ordering is unexercised on both sides. If a future scenario combines
them, the comma-joined `--allowed-tools` value would differ by element order only. Flagged as a `- [!]`
follow-up for cycle-14 (semantically a set; CLI tool-allowlist matching is membership, not order — but worth
a confirming differential when send_query lands). Not a current divergence.

### REVISED SYMBOL ROLL-UP — deterministic core now PASSES the no-downgrade gate

PASS → `- [x]` (FLIPPED this re-verify):
- claude::argv `build_claude_argv`, `ProviderWarning`, `TRANSPORT_FLAGS` — 23/23 live-bun differential,
  deterministic; the `allowed_tools` conflation is gone.

PASS → `- [x]` (unchanged from prior block): all parser symbols (`parse_claude_stream_json`,
`parse_claude_stream_json_line`, `normalize_claude_usage`, `parse_user_tool_result`, `RawUsage`,
`ToolResultEntry`); cli_stream `classify_subprocess_error`/`ErrorClass`, `with_first_message_timeout`/
`FirstEventError`, `accumulate_stderr_lines`, `RetryConfig`/`RetryError`, `classify_stderr_line`/
`StderrClass`, `NdjsonStream`/`StreamError`.

QUALIFIED → `- [≠]` (unchanged, recorded — no downgrade):
- cli_stream::retry `classify_and_enrich_error` / `EnrichedError` — abort-path error-class label
  (`timeout`/`aborted`→`Unknown`) is logging-only; message+retry exact (call site 960-982 feeds
  `errorClass` to `getLog()` only, never control flow, never user-facing).

FAIL → none. (No symbol remains `- [~]` in the deterministic core.)

### FINAL VERDICT
`build_claude_argv` now **PASS**. The entire cycle-13 **deterministic core — cli_stream + argv + parser —
now passes the no-downgrade gate**: every symbol is `- [x]` (or the recorded `- [≠]` for the logging-only
abort label). `build_claude_argv` flips `- [~]` → `- [x]` in symbol-map.md. The cycle may commit the
deterministic-core sub-unit. (send_query orchestration + buildSDKHooksFromYAML remain deferred to cycle-14,
as planned — those rows stay `- [ ]`.)
