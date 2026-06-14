# Parity Verdict — ITERATE Cycle 11 — PR-02 Provider Registry

**Unit:** PR-02 `packages/providers/src/registry.ts` + `index.ts` + `errors.ts` + per-provider `capabilities.ts` + community `registration.ts`
**Rust:** `crates/har-provider/src/lib.rs`
**Method:** DIFFERENTIAL — live `bun` 1.3.14 source oracle ⇄ live `har-provider` registry, byte-identical JSON diff. The port's own unit tests are NOT the oracle.

---

## 2026-06-14 — VERDICT: **PASS** (19/19 symbols verified)

Build precondition: `cargo clippy -p har-provider --all-targets` → clean; `cargo test -p har-provider` → 38 passed (35 unit + 3 differential).

**Oracle provenance:** transient `packages/providers/src/__parity_oracle_cycle11.ts` drove the ACTUAL source registry (registerBuiltin/Community → real provider registrations) under bun; output captured as the committed golden `crates/har-provider/tests/fixtures/parity_cycle11_source_golden.json`. Oracle DELETED from Archon afterward (git status clean — source pristine).

**Durable differential test:** `crates/har-provider/tests/parity_cycle11.rs` (3 tests) drives the live Rust registry through identical scenarios and diffs against the committed source golden. **Anti-tautology proven:** injecting `pi.native_tools true→false` made the gate FAIL with `pi.nativeTools: source=true rust=false`; reverted → green.

### Per-behavior results (source bun value ⇄ Rust value)

| # | Behavior | Source (bun) | Rust | Verdict |
|---|----------|--------------|------|---------|
| B1 | `registerProvider` duplicate THROWS | `threw:true, "Provider 'dup' is already registered"` | identical | PASS |
| B1b | `registerBuiltinProviders` twice IDEMPOTENT | `threw:false, count:2` | identical | PASS |
| B2 | insertion order preserved (zebra,alpha,mike) | `["zebra","alpha","mike"]` | identical | PASS |
| B3a | `getAgentProvider(unknown)` error | `name:UnknownProviderError, "Unknown provider: 'zzz'. Available: a, b, c"` | identical | PASS |
| B3b | `getProviderCapabilities(unknown)` error | `"Unknown provider: 'zzz'. Available: a, b, c"` | identical | PASS |
| B3c | empty-registry unknown | `"Unknown provider: 'missing'. Available: "` (trailing space, empty join) | identical | PASS |
| B4 | **capability table 5×14** | see below | **identical, all 70 cells** | PASS |
| B5a | full registration order + builtIn | claude(T),codex(T),opencode(F),pi(F),copilot(F) | identical | PASS |
| B5b | `ProviderInfo` projection keys | `["builtIn","capabilities","displayName","id"]` (no factory) | identical | PASS |
| B5c | community-only order | `["opencode","pi","copilot"]` | identical | PASS |
| B6 | `clearRegistry` semantics | `before:5, after:0, isRegisteredAfter:false` | identical | PASS |
| B7 | `isRegisteredProvider` | `present:true, absent:false` | identical | PASS |

### B4 — CAPABILITY TABLE diff (re-derived from source, NOT from porter's table)

Every flag independently read from each `capabilities.ts` and diffed against the live Rust registry. **All 5 providers MATCH on all 14 flags (70/70 cells).**

| flag | claude | codex | copilot | pi | opencode |
|------|:------:|:-----:|:-------:|:--:|:--------:|
| sessionResume | ✓T | ✓T | ✓T | ✓T | ✓T |
| mcp | ✓T | ✓T | ✓T | ✓F | ✓T |
| hooks | ✓T | ✓F | ✓F | ✓F | ✓T |
| skills | ✓T | ✓T | ✓T | ✓T | ✓T |
| agents | ✓T | ✓F | ✓T | ✓F | ✓T |
| toolRestrictions | ✓T | ✓F | ✓T | ✓T | ✓T |
| structuredOutput | ✓enforced | ✓enforced | ✓best-effort | ✓best-effort | ✓enforced |
| envInjection | ✓T | ✓T | ✓T | ✓T | ✓T |
| costControl | ✓T | ✓F | ✓F | ✓F | ✓F |
| effortControl | ✓T | ✓F | ✓T | ✓T | ✓F |
| thinkingControl | ✓T | ✓F | ✓T | ✓T | ✓F |
| fallbackModel | ✓T | ✓F | ✓F | ✓F | ✓F |
| sandbox | ✓T | ✓F | ✓F | ✓F | ✓F |
| nativeTools | ✓T | ✓F | ✓F | ✓T | ✓F |

Per-provider capability-table result: **claude MATCH · codex MATCH · copilot MATCH · pi MATCH · opencode MATCH.**

### Notable contract fidelity points (verified, not assumed)
- **`structuredOutput` third tier:** source type is `'enforced' | 'best-effort' | false` — the unsupported tier is the JS literal `false` (boolean), NOT the string `'none'` mentioned loosely in some task framings. Rust `StructuredOutputCapability::None` serializes to wire `"false"` (`#[serde(rename = "false")]`) and the differential maps it to the JS `false` literal. Covered by `structured_output_unsupported_tier_serializes_as_false_literal`. (No provider currently uses this tier, so it does not appear in B4 — but the mapping is exercised directly.)
- **`UnknownProviderError` message format:** prefix `Unknown provider: '`, single-quoted id, `'. Available: `, `, `-joined registered ids in **insertion order**, including the trailing space on empty registry. EXACT match (B3a/B3b/B3c).
- **register sets/order:** builtins `{claude, codex}` (claude first); community `opencode → pi → copilot`; combined `claude, codex, opencode, pi, copilot`. EXACT.
- **builtIn flags:** claude/codex `true`; opencode/pi/copilot `false`. EXACT.
- **Idempotent vs throw:** manual `registerProvider` re-register THROWS; `registerBuiltinProviders` and each community registration SKIP-IF-PRESENT (no throw). EXACT.
- **`getRegistration` shape divergence (justified, not a downgrade):** Rust `get_registration_info` returns the `ProviderInfo` projection rather than the full `ProviderRegistration` because the Rust registration holds a non-Clone factory closure. The factory is reachable via `get_agent_provider` (B3a confirms factory is invoked: `get_type()=="claude"`). Behavior-equivalent; recorded.

### Symbols flipped to `- [x]` in symbol-map.md (19/19, all of PR-02)
registerProvider · getRegisteredProviders · isRegisteredProvider · getProviderCapabilities · getAgentProvider · getRegistration(→get_registration_info) · getProviderInfoList · clearRegistry · registerBuiltinProviders · registerCommunityProviders · registerCopilotProvider · registerOpencodeProvider · registerPiProvider · UnknownProviderError · CLAUDE_CAPABILITIES · CODEX_CAPABILITIES · COPILOT_CAPABILITIES · PI_CAPABILITIES · OPENCODE_CAPABILITIES

### Artifacts (committed under the crate)
- `crates/har-provider/tests/parity_cycle11.rs` — durable differential test (3 tests)
- `crates/har-provider/tests/fixtures/parity_cycle11_source_golden.json` — bun-captured source golden

**Unit PR-02 → eligible for ledger `- [x]` + commit. No downgrade detected.**
