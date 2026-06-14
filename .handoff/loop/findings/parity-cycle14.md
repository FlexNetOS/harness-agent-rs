# Parity verdict — ITERATE cycle 14 (PR-03 orchestration: `ClaudeProvider::send_query` + `build_hooks_settings_json` + registry wiring)

## 2026-06-14 — VERIFIER VERDICT: **FAIL** (unit held open)

**Source X:** `meta/Archon/packages/providers/src/claude/provider.ts` (sendQuery 851-989, buildSDKHooksFromYAML 233-255, applyNodeConfig 275-442).
**Rust:** `crates/har-provider/src/claude/provider.rs` (`ClaudeProvider`, `send_query`, `build_hooks_settings_json`, `run_single_attempt`), `crates/har-provider/src/claude/argv.rs`, `crates/har-provider/src/cli_stream/retry.rs`, `crates/har-provider/src/lib.rs` (registry wiring).
**Method:** live bun 1.3.14 differential for the bun-differentiable seams (hooks, allowedTools order); FakeSpawner end-to-end for the orchestration control flow; source-read for the SDK-only / live-model behaviors; live `claude --help` (2.1.177) for CLI-flag-surface verification.
**Baseline:** `cargo test -p har-provider` → 305 passed, 1 ignored. `cargo clippy -p har-provider --all-targets --all-features` → clean.

---

### Per-behavior results

| # | Behavior | Verdict | Evidence |
|---|----------|---------|----------|
| 1 | `send_query` orchestration (retry loop, abort-before-attempt, argv-per-attempt, MAX=3, backoff `base*2^attempt`, classify gate, first-event timeout, cancel, exit-code) | **PASS** | `tests/parity_cycle14_orchestration.rs` (6 scenarios, all green) + source-read of provider.ts:894-988 |
| 2 | `build_subprocess_env` (process env + per-request overlay; request wins) | **PASS** | lib test `build_subprocess_env_overlays_request_env`; matches provider.ts:88-99 + 866-867 |
| 3 | `build_hooks_settings_json` (`buildSDKHooksFromYAML`) | **PASS** (1 benign `[≠]`) | `tests/parity_cycle14_hooks.rs` differential vs live-bun oracle (7 cases) |
| 4 | argv `--allowed-tools` order `[...mcpWildcards, 'Skill']` (cycle-13 `[!]`) | **PASS — `[!]` RESOLVED** | live-bun oracle → `["mcp__my-server__*","Skill"]`; Rust test `mcp_wildcards_before_skill_in_allowed_tools` matches |
| 5 | `persistSession` + `systemPrompt.excludeDynamicSections` claimed SDK-only / no CLI effect | **FAIL** | live `claude --help` 2.1.177: **both have CLI flags** — see below |
| 6 | registry wiring (`register_builtin_providers` "claude" → `ClaudeProvider`; `get_type`/caps) | **PASS** (1 scoped `[≠]` on UID-0 fallback) | lib tests green; source-read of registry.ts:113-115 |
| 7 | R8 nativeTools cap stays `true`, deferral = logged warning + argv seam | **PASS** (documented UP-1 deferral) | provider.rs:455-464 `tracing::warn!`; argv.rs:394-410 seam; caps unchanged |

---

### #5 — FAIL detail (the load-bearing finding)

The cycle-14 `[≠]` resolutions for `persistSession` (ledger row 398) and `excludeDynamicSections`
(row 399) — and the module-doc rationale at provider.rs:19-31 — assert these are "SDK-only fields
with **no CLI flag**". **That premise is false.** `claude --help` (claude-code 2.1.177, the binary the
CLI-delegation port shells out to) exposes:

- **`--no-session-persistence`** — "Disable session persistence - sessions will not be saved to disk
  and cannot be resumed (only works with `--print`)". The port already passes `--print` (TRANSPORT_FLAGS).
  This is exactly `persistSession: false`.
- **`--exclude-dynamic-system-prompt-sections`** — "Move per-machine sections (cwd, env info, memory
  paths, git status) from the system prompt into the first user message." This is exactly
  `systemPrompt.excludeDynamicSections: true`.

**Consequence (silent capability downgrade):** a caller that sets `persistSession: false` or
`excludeDynamicSections: true` has that request **silently dropped** — the CLI default (persist =
on; dynamic sections = included) is used instead. This is a no-downgrade violation: the delegation
model **can** express both, the port just doesn't.

**Exact divergence:**
- Input: `SendQueryOptions { persist_session: Some(false), .. }` → Rust argv: (no flag). Faithful CLI
  argv must contain `--no-session-persistence`.
- Input: `SystemPromptInput::Preset(SystemPromptPreset { exclude_dynamic_sections: true, .. })` → Rust
  argv: (no flag). Faithful CLI argv must contain `--exclude-dynamic-system-prompt-sections`.

**Required porter fix (routes back to rust-port-porter):** in `build_claude_argv` (argv.rs),
1. emit `--no-session-persistence` when `request_options.persist_session == Some(false)`;
2. emit `--exclude-dynamic-system-prompt-sections` when the resolved `systemPrompt` is a preset with
   `exclude_dynamic_sections == true` (replace the "no CLI flag equivalent documented → seam" comment
   at argv.rs:181). Update module doc provider.rs:19-31 and ledger rows 398-399 from `[≠]` to `[x]`
   once the flags are emitted + differentially verified. (Note: `persistSession: true` and
   `excludeDynamicSections: false` correctly need NO flag — they ARE the CLI default; only the
   non-default values were being dropped.)

> This is fail-closed: these two fields were the **only** material divergence. Everything else is
> parity-clean. Once the porter emits the two flags and re-verifies, the unit flips to PASS.

---

### #3 — PASS with one documented benign `[≠]` (empty-matchers-array)

Differential vs live-bun `buildSDKHooksFromYAML`. 6 of 7 cases byte-identical after canonicalization
(object-key order + `5000` vs `5000.0` are JSON-serialization artifacts the claude-code settings
parser ignores — not behavior). The one divergence is **proven benign**:

- Input `{"PostToolUse": []}` (empty matcher array — reachable: `workflowNodeHooksSchema` allows
  `z.array(...).optional()`, and `[]` passes).
- **Source:** returns `{ PostToolUse: [] }` (non-empty map; `Object.keys.length === 1`).
- **Rust:** returns `None` (empty-matchers `continue` → `any=false`).
- **Why benign (no-op in the source's OWN consumer):** `applyNodeConfig` merges
  `[...[], ...existing] === existing` (verified via merge-oracle) — the empty matcher array
  contributes ZERO effective hooks. In CLI mode, source `{PostToolUse: []}` → empty hooks list for
  that event = no hooks fire; Rust `None` → no `--settings` file = no hooks fire. **Identical end
  state: zero declarative hooks.** Test asserts the equivalence (`KNOWN_BENIGN_EMPTY_MATCHERS`).
  Add as `[≠]` (benign, no capability loss).

### #6 — PASS with one scoped `[≠]` (UID-0 factory fallback)

Source claude factory is `() => new ClaudeProvider()` (registry.ts:115); the constructor **throws** on
UID-0 without `IS_SANDBOX=1` and the factory does NOT catch it, so `getAgentProvider` **propagates the
throw at resolve time**. Rust factory (lib.rs:338-351) **catches** the UID-0 error and returns an
`UnimplementedProvider` stub that **panics on `send_query`** (use time). Failure shape/timing differs
(resolve-time throw → use-time panic). **Structurally forced:** the Rust factory type is
`Box<dyn Fn() -> Arc<dyn AgentProvider>>` (har-contract lib.rs:665) — no `Result` channel, so the
closure can only panic or return a stub; the port chose the fail-closed stub. Defensible but it IS a
behavior change → must be a documented `[≠]` on the registry row, not an unflagged equivalence.

---

### Durable artifacts committed under the crate
- `crates/har-provider/tests/parity_cycle14_hooks.rs` (differential, 1 test) + fixture
  `crates/har-provider/tests/fixtures/claude/hooks/source-oracle.json` (live-bun captured).
- `crates/har-provider/tests/parity_cycle14_orchestration.rs` (6 orchestration scenarios).
- Test-only constructors `new_for_test` / `new_for_test_with_delay` behind a `test-util` feature
  (provider.rs + Cargo.toml) so integration tests can drive `send_query` without the UID guard.

### Archon source
Transient oracles (`packages/providers/.parity-oracle-c14/`) deleted. `provider.ts` untouched —
Archon pristine (verified `git status`).

---

### Symbol-map rollup (this cycle)
- `- [x]` PR-03 `ClaudeProvider` (struct/ctors/UID-guard/Default/get_type/get_capabilities) — verified.
- `- [x]` PR-03 `ClaudeProvider::send_query` orchestration control flow — verified (6 scenarios).
- `- [x]` PR-03 `build_hooks_settings_json` (`buildSDKHooksFromYAML`) — verified (+1 benign `[≠]`).
- `- [x]` PR-03 registry wiring `register_builtin_providers` "claude" → `ClaudeProvider`.
- `- [≠]` (NEW, benign) hooks empty-matchers-array `{event:[]}` → `None` (zero-hooks equivalent).
- `- [≠]` (NEW, scoped) registry UID-0 factory fallback to stub (resolve-throw → use-panic; forced
  by non-Result factory type).
- `- [!]` (STILL OPEN — FAIL) `persistSession` / `excludeDynamicSections`: NOT SDK-only — CLI flags
  `--no-session-persistence` / `--exclude-dynamic-system-prompt-sections` exist and are unemitted →
  silent downgrade. Rows 398-399 must NOT flip to `[x]`; reopened from `[≠]` to `[!]`.

### Unit verdict
**FAIL — unit held open.** PR-03 orchestration core (send_query, hooks, registry, allowedTools order
fix) is parity-clean, but the two SDK-field `[≠]` resolutions are based on a **false "no CLI flag"
premise** and constitute a silent capability downgrade. Porter must emit the two CLI flags. After the
fix + re-verify, PR-03-the-UNIT is complete **except** the R8 native-tools sidecar (cycle 15).

---

## 2026-06-14 — RE-VERIFY of behavior #5 only (porter fix landed): **PASS**

**Scope:** behavior #5 ONLY (`persistSession` / `excludeDynamicSections`). The other 6 behaviors
were untouched by the fix (the change is confined to `build_claude_argv` argv emission) and remain
PASS from the verdict above — confirmed by re-running the full suite (no regression).

**Method:** source-read of the exact emit conditions in `provider.ts` + `types.ts`; live
`claude --help` (claude-code 2.1.177) for flag spellings; Rust source-read of `argv.rs`; full
`cargo test -p har-provider` + `cargo clippy -p har-provider --all-targets --all-features`.

### 1. Emit CONDITIONS match the source SDK-options semantics — CONFIRMED

- **`persistSession`** — source `buildBaseClaudeOptions` (provider.ts:527-529) passes
  `persistSession` to the SDK **iff `requestOptions?.persistSession !== undefined`**, preserving the
  value. The SDK→CLI mapping emits `--no-session-persistence` **only for `false`** (`true` is the CLI
  default = persist). So the observable CLI contract is: **flag iff `persistSession === false`**.
  Rust (argv.rs:220-223): `if persist == Some(false) { push("--no-session-persistence") }`. `Some(true)`
  and `None` → no flag. **Exact match.** Precondition "only works with `--print`" is satisfied —
  `--print` is in `TRANSPORT_FLAGS` and always emitted (argv.rs:65-66, test `transport_flags_always_present`).
- **`excludeDynamicSections`** — source field lives **only on `SystemPromptPreset`** (types.ts:229-234,
  `type:'preset'`, `preset:'claude_code'`), `excludeDynamicSections?: boolean`. When the resolved
  systemPrompt is a preset with this `true` → SDK emits `--exclude-dynamic-system-prompt-sections`;
  `false`/absent, or a **non-preset** (string / string[]) systemPrompt → no flag.
  Rust (argv.rs:176-189): only the `SystemPromptInput::Preset(preset)` arm checks
  `preset.exclude_dynamic_sections == Some(true)`; the `Single`/`Multi`/`None` arms never emit it.
  Type fidelity confirmed (har-contract lib.rs:330-341: field is `Option<bool>` on the `Preset` struct
  only, serde `excludeDynamicSections`). **Exact match.**

### 2. Flag SPELLINGS — CONFIRMED against `claude --help` (2.1.177)

- `--no-session-persistence` — "Disable session persistence … (only works with `--print`)". ✓ byte-exact.
- `--exclude-dynamic-system-prompt-sections` — "Move per-machine sections (cwd, env info, memory
  paths, git status) from the system prompt into the first user message." ✓ byte-exact.

### 3. No regression — CONFIRMED

- **Default-value cases emit NO flag** (proven by dedicated tests, all green):
  `persist_session_true_does_not_emit_flag`, `persist_session_absent_does_not_emit_flag`,
  `exclude_dynamic_sections_false_does_not_emit_flag`, `exclude_dynamic_sections_absent_does_not_emit_flag`,
  `exclude_dynamic_sections_on_string_prompt_does_not_emit_flag`. The two positive cases
  (`persist_session_false_emits_no_session_persistence`, `exclude_dynamic_sections_true_emits_flag`)
  assert the flags ARE present. All 6 boundary conditions covered.
- **The 6 previously-PASSED behaviors are untouched:** the fix is localized to argv emission inside
  `build_claude_argv`; orchestration (#1), env overlay (#2), hooks-settings (#3), allowedTools order
  (#4, `[!]` resolved), registry (#6), R8 nativeTools cap/seam (#7) all unchanged. Full suite
  re-run green: **`cargo test -p har-provider` → 312 passed, 1 ignored** (was 305; +7 new #5 tests).
  **`cargo clippy --all-targets --all-features` → clean.**

### #5 final verdict: **PASS** — silent downgrade eliminated, both flags emitted on exactly the
non-default values, byte-exact spellings, default values correctly silent. Ledger rows 398-399 now
legitimately flip from `[!]`/`[≠]` to **`[x]`** (faithfully emitted CLI flags, differentially-grounded).

### Symbol-map rollup — UPDATED
- `- [x]` (FLIPPED from `- [!]`) `persistSession` → `--no-session-persistence` on `Some(false)`.
- `- [x]` (FLIPPED from `- [!]`) `excludeDynamicSections` → `--exclude-dynamic-system-prompt-sections`
  on Preset+`Some(true)`.
- The two prior `[≠]` resolutions (false "no CLI flag" premise) are **superseded** — these are now
  faithful `[x]` flag emissions, not intentional divergences.

### CYCLE 14 — FINAL GATE: **PASS (no-downgrade gate cleared)**
All 7 behaviors PASS. The two genuine `[≠]` rows remaining (hooks empty-matchers-array → `None` =
zero-hooks equivalent; registry UID-0 factory stub = resolve-throw → use-panic, forced by the
non-`Result` factory type) are documented, benign/scoped, owner-visible — not silent downgrades.

### PR-03 status: **the UNIT is complete EXCEPT the R8 native-tools sidecar (cycle 15).**
PR-03 orchestration core (send_query control flow, build_hooks_settings_json, registry wiring,
full argv surface incl. allowedTools order + persistSession + excludeDynamicSections) is
parity-verified and may be committed. The only outstanding PR-03 work is the **R8 nativeTools
sidecar** (the MCP-config/`--allowed-tools` wildcard sidecar — seam present at argv.rs:1019+,
cap stays `true`, deferred per UP-1) which is **cycle 15**.

### Archon source
Re-checked pristine: `git status --short packages/providers/` empty; no `.parity-oracle*` dirs
present. No transient oracle to clean — already removed in the prior verdict. Archon untouched.
