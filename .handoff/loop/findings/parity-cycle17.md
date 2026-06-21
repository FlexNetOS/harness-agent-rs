# Parity Verdict — Cycle 17: Codex Provider (PR-07 + PR-08)

**Date:** 2026-06-21
**Gate:** rust-port-parity-verifier (differential, fail-closed)
**Oracle:** live `@openai/codex-sdk@0.125.0` (`node_modules/.bun/.../dist/index.js`) +
`packages/providers/src/codex/{provider,config,binary-resolver,capabilities}.ts` + `mcp/config.ts`,
captured via bun 1.3.14. Archon left pristine (`git status` clean); transient oracle scripts deleted.
**Durable harness:** `crates/har-provider/tests/parity_cycle17_codex.rs` — 22 pass, 2 `#[ignore]` KNOWN-FAIL.
**Gate run:** `cargo clippy -p har-provider --all-targets -- -D warnings` = clean;
`cargo test -p har-provider` = 498 passed, 4 ignored.

## OVERALL: **FAIL** — PR-08 PASS, PR-07 (CodexProvider) BLOCKED by 2 confirmed downgrades + 1 latent.

---

## Architectural note (oracle correctness)
The TS `provider.ts` does NOT build a CLI argv or parse NDJSON itself — it drives `@openai/codex-sdk`
as a black box (`startThread`/`resumeThread`/`runStreamed`) and consumes SDK events
(`thread.started`, `item.completed{agent_message|command_execution|reasoning|web_search|todo_list|
file_change|mcp_tool_call}`, `error`, `turn.failed`, `turn.completed`). The Rust port replicates the
SDK's INTERNAL `CodexExec.run()` argv builder + the SDK's raw JSON event shapes. So the correct oracle
for `argv.rs`/`parser.rs` is the SDK `dist/index.js`, which I used. (The task brief's "assistant/text,
tool_use, system/init, rate_limit" event-name list is inaccurate; verified against the real SDK/source.)

## PASS areas (PR-08 + PR-07 internals)
- **AREA 8 CODEX_CAPABILITIES** — PASS. 14/14 flags byte-exact vs capabilities.ts.
- **AREA 4 resolveCodexBinaryPath** — PASS. dev→None; env>config>vendor>autodetect>throw; both
  not-found error texts byte-exact; file_exists rejects dirs. Source tier-3 = VENDOR DIR
  (`~/.archon/vendor/codex/<bin>`), tier-4 = FIXED npm-prefix probes (NOT generic PATH); Rust mirrors.
- **AREA 3 parseCodexConfig** — PASS. defensive matrix; empty `additionalDirectories` → `Some([])` (no
  length guard, correctly different from Claude settingSources).
- **AREA 1 build_codex_argv** — PASS for argv ORDER + flatten + flag table. Verified token-for-token vs
  live SDK oracle CASE1/2/3/4/7 (minimal, full, resume-at-end, mcp-overrides-position, number/bool/array
  `[ , ]`-joined). resume after approval_policy; config overrides immediately after --experimental-json.
- **AREA 5 classify + model-access** — PASS. All 5 classes incl. model_access precedence; retry
  eligibility RateLimit+Crash only, max 3. `build_model_access_message` BYTE-EXACT all 4 cases
  (fallback / no-fallback / None / trim-before-fallback); the `\`-continuation indent gotcha is correct.
- **AREA 6 MCP convert** — PASS. headers→http_headers remap (and NOT when http_headers already present);
  all 22 CODEX_MCP_PASSTHROUGH_KEYS preserved; unknown keys dropped; non-object servers skipped.
- **AREA 2 parse_codex_stream** — PASS (representative + edges): happy thread.started→agent→completed;
  command exit-code suffix; turn.failed terminal; error event mcp-vs-non-mcp capture+surface; unknown
  item/event silently ignored; structured-output valid (Some) + invalid (warning-then-result).
- **AREA 7 send_query orchestration** — PASS (FakeSpawner): assistant+result, session from thread.started,
  turn.failed error-result, fail-stop synthesis on stream-close-without-terminal, crash→retry→success,
  cancel-before-start → empty stream.

## FAIL — concrete fixes the porter MUST make

### D1 (FAIL) — `argv::to_toml_value` drops control-char escaping
SDK `toTomlValue` (dist/index.js:330-331) = `JSON.stringify(value)`: `\n`→`\n`, `\t`→`\t`, `\r`→`\r`,
C0 controls→`\uXXXX`. Rust only `.replace('\\',…).replace('"',…)`, leaving RAW control bytes in the
`--config` token. Oracle (bun CASE6): `a"b\c<LF>d` → SDK `"a\"b\\c\nd"`, Rust `"a\"b\\c<LF>d"`.
Any MCP config value with a control char (multi-line header, args element, token) → divergent/malformed
`--config` flag. **Fix:** make `to_toml_value` string branch escape control chars like JSON.stringify
(serde_json::to_string of the String produces exactly this). Plain/quote/backslash strings already match.
Note: JSON.stringify does NOT escape ` `/` ` (verified) — don't over-escape those.

### D2 (FAIL) — structured-output schema NOT normalized (portable feature skipped, NOT a [≠])
provider.ts:310 applies `normalizeJsonSchemaForOpenAiStrict(rawSchema)` before sending the schema;
tested in provider.test.ts:763 ("adds additionalProperties:false … recursion through nested `meta`").
Rust `write_schema_temp_file` (provider.rs:851, comment admits "normalizer … not yet ported") writes
the schema VERBATIM. → the `--output-schema` file content differs; OpenAI strict-mode HTTP-400s the open
form (per source comment #1843) where TS succeeds. This is a portable, observable-output feature being
dropped → **FAIL** (route to porter to port `normalizeJsonSchemaForOpenAiStrict` recursive
`additionalProperties:false` injection + the `hasOpenAdditionalProperties` warn at provider.ts:303-308).
NOT a permitted `[≠]` (not inexpressible / not non-contractual / not a superset).

### D3 (QUALIFIED FAIL, latent) — UTF-8 byte-slice panic in warning preview
parser.rs:589 `&state.accumulated_text[..len.min(200)]` slices by BYTES; TS `.slice(0,200)` (UTF-16)
never panics. The slice is a `tracing::warn!` field arg, so it only evaluates (and panics) when WARN is
enabled for `provider.codex` — latent in `cargo test` (no subscriber), live in Archon (logger active).
Reproduced deterministically (199×'a'+😀 → byte 200 mid-char → panic). **Fix:** char-boundary-safe
truncation (e.g. `.chars().take(200).collect()`).

## Out-of-unit dependency (record, do not gate PR-07 alone)
PR-07's `send_query` MCP path calls an INLINE `load_mcp_config` (provider.rs:417) that is a stopgap
re-impl of `mcp/config.ts::loadMcpConfig` — the separate **PR-12** unit (still `- [ ]`). It diverges from
the source loader in ways reachable through Codex:
- env-var expansion accepts LOWERCASE ids (`$myvar` expands) — TS regex is UPPERCASE-only `[A-Z_][A-Z0-9_]*`.
- expands EVERY string recursively — TS expands ONLY `env`/`headers` subrecords.
- ignores the `{ "mcpServers": {…} }` wrapper (TS `normalizeMcpConfig` unwraps + throws on mixed keys).
- silently warns/continues on malformed config — TS THROWS (aborting the query).
These are already tracked as the `- [≈]` follow-up owed by PR-12 (ledger:582-588). They are NOT charged
against PR-07 in isolation, but PR-07's MCP behavior is not fully source-faithful until PR-12 lands and
`send_query` is rewired to the canonical loader. Flag for the orchestrator.

## Symbol roll-up
- PR-08: `CODEX_CAPABILITIES` `- [x]`, `resolveCodexBinaryPath` `- [x]`, `parseCodexConfig` `- [x]` → **PR-08 unit may flip `- [x]`.**
- PR-07: `CodexProvider` `- [~]` (FAIL) → **PR-07 unit stays open; do NOT commit as done.**

No porter fixture disagreed with the live oracle this cycle (the committed unit tests match my oracle
on the PASS areas). The two downgrades are omissions (un-ported normalizer; under-escaped TOML), not
mis-recorded fixtures.

---

# RE-VERIFICATION — Cycle 17 fixes (D1/D2/D3) — FINAL VERDICT

**Date:** 2026-06-21 (re-verify pass)
**Gate:** rust-port-parity-verifier (differential, fail-closed; independent oracle, porter report NOT trusted)
**Oracle (mine, fresh):** live `@openai/codex-sdk@0.125.0` dist/index.js + Archon `shared/structured-output.ts`
+ `codex/provider.ts`, run via bun 1.3.14. Archon left pristine (`git status` = clean; transient scripts
written to `/tmp/parity_c17/`, not in Archon). Rust side run via standalone cargo bins mirroring the
ported functions, plus the live crate gate.
**Gate run:** `cargo clippy -p har-provider --all-targets -- -D warnings` = **clean (No issues found)**;
`cargo test -p har-provider` = **515 passed, 0 failed, 2 ignored** (the 2 ignored are env-gated live
smoke tests in `mcp_sidecar.rs` + a cycle16 live smoke — NOT cycle17, NOT downgrade skips).
`--test parity_cycle17_codex` = **30 passed, 0 ignored** (was 22 pass + 2 `#[ignore]` KNOWN-FAIL in
cycle 17; the 2 previously-ignored tests are now LIVE and GREEN, plus 6 new tests). Zero `#[ignore]`
attributes remain in the cycle17 harness.

## OVERALL: **PASS** — both downgrade fixes confirmed; D3 QUALIFIED as `- [≠]` (cosmetic, log-only).
PR-07 (`CodexProvider`) and PR-08 may flip `- [x]` and commit.

### D1 — `argv::to_toml_value` String arm → `serde_json::to_string` — **CONFIRMED CLOSED**
Independent oracle: emitted `JSON.stringify(c)` for EVERY code point U+0000..U+00FF + astral/boundary
chars (`/`, U+00A0, U+FFFF, 😀 U+1F600, `\`, `"`, multi-char control mixes), and the Rust
`serde_json::to_string(&Value::String(..))` for the identical set. **Diff = 0 / 256 code points, 0 / 8
extra.** Confirms: `\n`/`\t`/`\r`/C0 → `\uXXXX` match; forward-slash `/` NOT escaped by either (no
over-escaping — the cycle-17 note was honored); DEL (0x7F) raw on both; unicode >0x7F raw on both;
`\`/`"`/plain unchanged. No char where serde_json diverges from JSON.stringify. argv.rs:165-172.

### D2 — `normalize_json_schema_for_openai_strict` + `has_open_additional_properties` +
`is_object_schema_node` ported (provider.rs:861-922; called provider.rs:932-949) — **CONFIRMED CLOSED**
Independent oracle: ran the live TS `normalizeJsonSchemaForOpenAiStrict`/`hasOpenAdditionalProperties`
(shared/structured-output.ts) over an 18-case matrix fed from a SHARED `cases.json` (identical insertion
order to both sides): primitives, simple/nested/deeply-nested(4-level) objects, `properties`-without-type,
`anyOf`, `$defs` + `definitions`, array `items` (object + tuple form), type-union with/without `object`,
already-closed, `additionalProperties:true`, open-subschema, nested-open-in-`$defs`, key-order, scalar+null
leaves in `properties`. Diffed normalized JSON (key order via `preserve_order`, confirmed enabled at
workspace `Cargo.toml:32` and feature-unified into the crate) AND the `hasOpen` warn trigger.
**Diff = 0 / 18 cases on both `normalized` and `hasOpen`.** No recursion branch missed; `isObjectSchemaNode`
rule (type==='object' | type-array-includes-object | has 'properties') matches; descent into arrays/scalars
matches; the un-ported-normalizer downgrade is GONE. `write_schema_temp_file` now writes the normalized
schema + warns on open-record exactly per provider.ts:303-311.

### D3 — `parser.rs:589` preview → `.chars().take(200).collect()` — **CONFIRMED no-panic; QUALIFIED `- [≠]`**
(a) **Panic safety CONFIRMED.** Ran 199×'a'+😀 (byte 200 mid-char), 200×'a'+😀, 250×😀 through
`.chars().take(200).collect()` → all return cleanly, exactly 200 scalars, **no panic** (the old
`&s[..200.min(len)]` byte slice would panic here).
(b) **Semantics divergence characterized & QUALIFIED.** TS `accumulatedText.slice(0,200)` is UTF-16 code
units; `.chars().take(200)` is Unicode scalar values. For 250×😀: TS preview = 200 UTF-16 units = **100
emoji**; Rust preview = 200 scalars = **200 emoji**. For 199×'a'+😀: TS slice even ends with a **lone
half-surrogate** (broken char); Rust never does. So they diverge in how many astral chars appear in the
preview. **This is LOG-COSMETIC, not data:** the preview is solely the `outputPreview`/`output_preview`
field of a `warn` (provider.ts:602-605 `getLog().warn`; parser.rs:594 `tracing::warn!`). The downstream
`system` event content is a FIXED string ("⚠️ Structured output requested but Codex returned non-JSON
text…") that does NOT include the preview, and `structuredOutput` is `undefined`/`None` regardless. No
consumer parses the preview. Per the `[≠]` bar this is **non-contractual / unobservable** (a log artifact);
the Rust form is in fact strictly more correct (no lone surrogate). → recorded as **`- [≠]` (cosmetic,
log-only; UTF-16-units vs scalar-values preview length in WARN log; non-contractual)**, NOT passed
silently, NOT a FAIL (it is not data-bearing).

## Carried-forward follow-up (unchanged, still owed)
- **PR-12 `loadMcpConfig` `- [≈]`** — PR-07's `send_query` MCP path still calls the inline stopgap
  `load_mcp_config` (provider.rs:417) that diverges from `mcp/config.ts::loadMcpConfig` (lowercase env-id
  expansion; expands every string vs only env/headers; ignores the `{mcpServers:{…}}` wrapper; warns-and-
  continues vs THROWS on malformed config). Tracked as the `- [≈]` owed by **PR-12** (ledger:582-588).
  Not charged against PR-07 in isolation; PR-07's MCP path is fully source-faithful only once PR-12 lands
  and `send_query` is rewired to the canonical loader. CARRY FORWARD.

## Symbol roll-up — FINAL
- **PR-08**: `CODEX_CAPABILITIES` `- [x]`, `resolveCodexBinaryPath` `- [x]`, `parseCodexConfig` `- [x]` → **PR-08 `- [x]`.**
- **PR-07**: `CodexProvider` → all contract behaviors re-verified incl. the 3 ex-downgrades; D1/D2 PASS,
  D3 `- [≠]` (survives the challenge: log-only/non-contractual). → **`CodexProvider` `- [x]`, PR-07 `- [x]` — COMMIT.**
  (Residual PR-12 `- [≈]` is an out-of-unit follow-up, does not block PR-07's own symbols.)
