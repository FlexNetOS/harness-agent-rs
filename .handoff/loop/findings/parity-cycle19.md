# Parity Verdict — Cycle 19 — OpenCode community provider (PR-11)

**Date:** 2026-06-21
**Gate:** rust-port-parity-verifier (differential, fail-closed)
**Oracle:** live TypeScript source under `meta/Archon/packages/providers/src/community/opencode/`, run through `bun 1.3.14`. All golden values captured from that live run; the porter's `cargo test` report was NOT trusted (and indeed missed all 4 divergences below — its `.contains()` assertions never checked byte-order or empty-field omission).
**Harness (durable):** `crates/har-provider/tests/parity_cycle19_opencode.rs` — 27 assertions, **23 PASS / 4 FAIL**.
**Archon:** kept PRISTINE (4 transient oracle scripts written under the opencode dir, run, then deleted; `git status` clean).
**New deps confirmed present:** `rand`, `url`, `hex` (workspace), `futures-util` — all already wired in `crates/har-provider/Cargo.toml`.

---

## OVERALL VERDICT: **FAIL**

The ported surface is **mostly** byte-exact (config / errors / tokens / agent_config / multi_agent aggregation / runtime helpers / session demux / provider control-flow / seam isolation all PASS), but **4 observable divergences** in two symbols are genuine no-downgrade violations and must be fixed before PR-11 can commit. The SDK seam itself is correctly isolated and is an ACCEPTED honest seam (UP-2 option b).

Routed back to the porter with the exact fixes in §"Required fixes".

---

## Per-area verdicts

| # | Area | Verdict | Evidence |
|---|------|---------|----------|
| 1 | config (`parse_model_ref`, `parse_opencode_config`) | **PASS** | 15-case model-ref oracle + 11-case defensive config matrix; 0 diffs |
| 2 | errors (classify / enrich / errorMessage / RetryableErrorClass) | **PASS** | 25-case classify corpus (aborted-first, rate_limit-before-auth precedence) + byte-exact enrich; 0 diffs |
| 3 | tokens (`normalize_tokens`) | **PASS** | 14-case oracle (total-from-3-fields, omit-when-0, cost, non-number→0/omit); 0 diffs |
| 4 | agent_config (kebab / list / select / adapt / tools / resolvePrompt / warn-once) | **PASS** | 20-case kebab corpus + adapt model/tools/invalid-error byte-exact + buildTools collision; warn-once AtomicBool confirmed single-fire; 0 diffs |
| 5 | agent_fs (`build_agent_file_content`, `materialize_agents`) | **FAIL** | D1 (empty description emitted) + D2 (tools key order) — byte-exact file divergence. materialize orchestration PASSES but inherits content FAIL |
| 6 | runtime (helpers + seam isolation) | **PASS** | 64-hex pw, server-config, port-extract, 4 bind-conflict patterns + negatives, range [20000,60000); Windows kill `[≠]` SKIP (faithful) |
| 7 | session (`createSessionPromptBody`, demux, result chunk, resolve logic) | **FAIL** | key order PASSES (preserve_order); D3 (empty systemPrompt emitted). Demux/result-chunk/resolve-logic PASS |
| 8 | multi_agent (with_agent_node_config / format / collect / aggregate_tokens) | **PASS** | aggregate reduce semantics diffed vs bun incl. single-element-no-cost (cost absent) + zero-total-first; 0 diffs |
| 9 | seam isolation + materialize-before-seam side effect | **PASS** | `acquire_embedded_runtime` is the only live-SDK entry; send_query fires `materialize_agents` (real FS writes) BEFORE the `opencode_sdk_not_bound` Result; nothing portable hidden behind the seam |

---

## The 4 divergences (FAIL evidence)

### D1 — `build_agent_file_content`: empty `description` emitted (agent_fs.rs)
- **Input:** `{ description: "", prompt: "" }`
- **TS (oracle):** `---\nmode: subagent\n---`
- **Rust:** `---\nmode: subagent\ndescription: ""\n---`
- **Root cause:** `if let Some(desc) = &agent_config.description.as_str().get(0..)` — `.get(0..)` returns `Some("")` for an empty string, so the line is always emitted. TS uses `if (agentConfig.description)` (falsy for `""`).
- **D1b** (empty desc + real prompt): TS `---\nmode: subagent\n---\n\nhas prompt`; Rust adds `description: ""`.
- Reachable: `description` is a required `string` in the source schema but may be `""`.

### D2 — `build_agent_file_content`: tools key order (agent_fs.rs)
- **Input:** reviewer with `tools:['read','grep'], disallowedTools:['bash']`
- **TS (oracle):** `tools:\n  read: true\n  grep: true\n  bash: false` (insertion order: allowed then denied)
- **Rust:** `tools:\n  bash: false\n  grep: true\n  read: true` (alphabetical — `sort_by_key`)
- **Root cause:** Rust builds a `HashMap` then `sorted.sort_by_key(|(k,_)| k)`. The `.opencode/agents/*.md` file is parsed by the OpenCode SDK; byte-order is the contracted grain (cartographer specified byte-exact via preserve_order). The source deterministically emits insertion order.

### D3 — `create_session_prompt_body`: empty `systemPrompt` emitted (session.rs)
- **Input:** `{ systemPrompt: "" }`
- **TS (oracle):** body keys `["parts","model"]` — `system` OMITTED (TS `requestOptions?.systemPrompt ?` falsy for `""`)
- **Rust:** emits `"system": ""` (any `Some(SystemPromptInput::Single(""))` is inserted)
- **Root cause:** `if let Some(ref system) = opts.system_prompt { body.insert("system", ...) }` — no JS-truthiness guard. Note `Multi(vec![])` (empty array) IS truthy in JS and both emit `system:[]` → only the empty-string `Single("")` diverges.

---

## `[≠]` challenges — all survive

- **abortableStream → tokio CancellationToken:** OBSERVABLE abort behavior matches — provider abort-check at retry-loop top yields an `aborted` Result and returns (no further chunks); internal mechanism differs but output identical. PASS.
- **embeddedRuntimePromise init-once → `OnceLock<Mutex>`:** single-init semantics preserved (acquire returns the same handle; seam path returns Err before any double-init). PASS (the seam makes a live double-init unreachable; the cell logic is faithful).
- **warnedMultipleAgents → `AtomicBool`:** warn-once confirmed — `swap(true)` fires the tracing::warn exactly once per process. PASS.
- **Windows kill path:** SKIP — untestable on Linux. Confirmed PRESENT + faithful (`taskkill /F /PID`, PowerShell `Get-NetTCPConnection`). The Unix path (`lsof -ti:<port> || fuser <port>/tcp`, `kill(pid, SIGKILL)`) matches the source command construction.
- **json_schema-only `OutputFormatType`:** the TS non-`json_schema` "omit format" path is genuinely INEXPRESSIBLE in the Rust contract (enum has only `JsonSchema`). Not a feature-skip — a type-level constraint. PASS.

## Carried items
- **`[≈]` (provider-wide, established):** TS throws / Rust surfaces error-as-`Result{is_error:true}` chunk. Carried, not re-litigated.
- **SDK seam `opencode_sdk_not_bound`:** ACCEPTED (UP-2 option b). Confirmed isolated to `acquire_embedded_runtime` (createOpencode) + the post-create `client.session.*`/`event.subscribe` calls. Materialize-agents FS side-effect fires before the seam (verified).
- **No NEW `[≠]` introduced.**

---

## Required fixes (porter)
1. **agent_fs.rs `build_agent_file_content`:** (a) omit the `description:` line when the string is empty (falsy guard, drop the `.get(0..)`); (b) preserve tools INSERTION order — use an insertion-ordered structure (e.g. `Vec<(String,bool)>` or `IndexMap`) for the allowed-then-denied build, remove `sort_by_key`.
2. **session.rs `create_session_prompt_body`:** guard `system_prompt` against the empty-string `Single("")` (replicate JS truthiness) before inserting `system`.
3. Re-run `crates/har-provider/tests/parity_cycle19_opencode.rs` — the `divergences::*` tests must flip to PASS (all 27 green) for PR-11 to commit.

## Reproduction
- Oracle: 4 transient scripts (`__oracle_cycle19.ts`, `__oracle_fs19.ts`, `__oracle_ma19.ts`, `__oracle_sess19.ts`) under the opencode dir, run with `bun run <f>.ts`, then deleted (Archon pristine). Golden values are inlined in the Rust harness.
- Rust: `cargo test -p har-provider --test parity_cycle19_opencode` → 23 pass / 4 fail; `cargo clippy -p har-provider --all-targets -- -D warnings` → clean.

---

## RE-VERIFY #2 — 2026-06-21 (same gate, fresh live-bun oracle, porter applied 3 fixes)

**Verdict on the OpenCode PORTED SURFACE: PASS** (commit unit `- [x]`; the SDK seam stays the ACCEPTED pending-SDK seam, UP-2 opt b).

I did NOT trust the porter's report or tests. I rebuilt the live-bun oracle (`bun 1.3.14`) from the actual TS source, ran 2 fresh transient oracle scripts (`__oracle_d3.ts`, `__oracle_d1d2.ts`) under the opencode dir, captured golden values, then DELETED them — Archon verified pristine (`git status` clean).

### D1 — empty `description` line omission → **CLOSED**
Rust now `if !description.is_empty()`. Re-diffed vs bun (`agent-fs.ts:23 if (agentConfig.description)`):
- `description:""` → OMITTED (`---\nmode: subagent\n---`) ✓
- `description:""` + prompt → `---\nmode: subagent\n---\n\nhas prompt` ✓
- **whitespace `" "`** → JS-truthy → EMITTED `description: " "` ✓ (`" ".is_empty()` is false → Rust matches; the explicit risk in the task is confirmed safe)
- `description:"hi"` → `description: "hi"` ✓

### D2 — tools key order → **CLOSED**
Rust now `Vec<(String,bool)>` insertion order (allowed=true first, then disallowed=false), in-place value overwrite on collision, no sort. Re-diffed vs bun byte-for-byte:
- `tools=[read,grep]`, `disallowed=[bash]` → `read:true, grep:true, bash:false` ✓
- `tools=[read,grep,bash]` → `read:true, grep:true, bash:true` ✓
- **collision** `tools=[read,grep]`, `disallowed=[read,bash]` → `read:false, grep:true, bash:false` ✓ (bun re-assigns the VALUE in place; the key keeps its ORIGINAL position — `read` stays at index 0, value flips to false. The Rust Vec does `tools_vec[idx] = (k,false)` — exact match. This is the subtle JS-object semantics; confirmed identical.)
- empty tools → no `tools:` block ✓
- tools-only `[write,edit]` → `write:true, edit:true` ✓
- disallowed-only `[bash,net]` → `bash:false, net:false` ✓

### D3 (CHALLENGED) — `create_session_prompt_body` system field → porter's fix was **WRONG for `Multi([])`**; gate CORRECTED it → now PASS

**(1) Source type evidence (the crux):**
`packages/providers/src/types.ts:236`: `export type SystemPromptInput = string | string[] | SystemPromptPreset;`
`AgentRequestOptions.systemPrompt?: SystemPromptInput` (types.ts:245). So at `session.ts:69`,
`requestOptions.systemPrompt` is **NOT string-only** — it can be a bare string, a **`string[]`** (→ Rust `Multi`), or a preset object (→ `Preset`). The `string[]`/Multi variant is genuinely **reachable** at this call (the same union the pi provider narrows with `typeof === 'string'` — opencode does NOT narrow, it passes the raw value into the truthiness check). So Multi is NOT dead at this path → it must be exercised, not QUALIFIED-as-dead.

**(2) Live-bun oracle of `...(requestOptions?.systemPrompt ? { system: ... } : {})`:**
| input | `system` present? | value |
|---|---|---|
| `Single("")` | **NO** (omitted) | — |
| `Single(" ")` | YES | `" "` |
| `Single("x")` | YES | `"x"` |
| **`Multi([])`** | **YES** | **`[]`** |
| `Multi(["a"])` | YES | `["a"]` |
| `Preset{type,preset}` | YES | `{"type":"preset","preset":"claude_code"}` |
| `undefined` | NO | — |

In JS **only `""`/`0`/`null`/`undefined`/`false`/`NaN` are falsy** — an empty array `[]` is **TRUTHY**. The porter's fix added `Multi(v) => v.is_empty()`, treating `[]` as falsy (a Rust/Python empty-collection assumption). That **OMITS** `system` for `Multi([])` while bun **INCLUDES** it as `[]`.

**(3) Rust diff (differential test `d3c`, before the gate's correction):**
```
left  (Rust):    None            ← system omitted (WRONG)
right (oracle):  Some(Array [])  ← system should be []
```
This is a **NEW divergence introduced by the D3 fix** — exactly the no-downgrade risk the challenge targets.

**Gate correction (oracle leaves zero design choice):** `session.rs` `is_falsy` match arm `Multi(v) => v.is_empty()` → `Multi(_) => false` (arrays are always JS-truthy). Also corrected the porter's inline unit test `empty_multi_system_prompt_is_omitted` (it encoded the bug → renamed `..._is_included_as_empty_array`, asserts `system == []`). `Single("")`→omit and `Preset`→include were already correct and stay.

### Verification (this re-verify)
- `crates/har-provider/tests/parity_cycle19_opencode.rs` → **34/34 live** (added D3b whitespace-Single, D3c `Multi([])`, D3d `Multi(["a"])`, D3e Preset, D2c collision, D2f disallowed-only, D1c whitespace-desc; all golden values from the live bun run).
- Full crate: `cargo test -p har-provider` → **797 pass / 0 fail / 2 ignored**.
- `cargo clippy -p har-provider --all-targets -- -D warnings` → **clean**.
- Archon: **pristine** (oracle scripts deleted; `git status` clean).

### Seam isolation / carried items (unchanged)
- **Seam isolation:** the only live-SDK entry is `acquire_embedded_runtime` (`createOpencode`) + the post-create `client.session.*` / `event.subscribe` calls. `send_query` fires `materialize_agents` (real FS writes to `.opencode/agents/`) **BEFORE** the seam returns `Result{is_error:true, error_subtype:"opencode_sdk_not_bound"}`. Nothing portable is hidden behind the seam.
- **ACCEPTED SDK seam:** `opencode_sdk_not_bound` (UP-2 option b) — honest, isolated, non-panic.
- **Carried `[≈]`:** provider-wide TS-throw → Rust error-as-`Result{is_error:true}` chunk. Not re-litigated.
- **`[≠]` (all survive the challenge):** Windows kill path (untestable on Linux, present + faithful); json_schema-only `OutputFormatType` (the TS non-json_schema omit-path is INEXPRESSIBLE in the Rust enum, type-level constraint, not a feature-skip); abortableStream→CancellationToken / init-once OnceLock / warn-once AtomicBool (all observable-behavior-equal). **No NEW `[≠]` introduced.**
