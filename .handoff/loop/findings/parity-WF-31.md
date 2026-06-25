# Parity verdict — WF-31 `validate_structured_output` (Ajv 8 → `jsonschema` 0.46)

## 2026-06-25 — VERDICT: **PASS**

**Verifier:** rust-port-parity-verifier (differential, fail-closed).
**Source oracle:** LIVE Ajv 8.20.0 via bun 1.3.14, importing the real
`validateStructuredOutput` / `formatSchemaErrors` from
`packages/providers/src/shared/structured-output.ts`.
**Target:** `crates/har-provider/src/shared/structured_output.rs` (`jsonschema` 0.46.6, Draft-07 pinned).
**Durable artifacts:**
- Ajv oracle harness: `meta-yard/Archon/packages/providers/src/shared/wf31-oracle.ts`
- Rust oracle harness: `crates/har-provider/examples/wf31_oracle.rs` (`cargo run -p har-provider --example wf31_oracle`)
- Golden outputs: scratchpad `oracle.json` / `rust.json`.

### Differential matrix — 26 (schema,value) probes, BOTH engines run

| Probe | Ajv | Rust | verdict | path | hook |
|---|---|---|---|---|---|
| valid-object | valid | valid | ✓ | — | — |
| missing-required | invalid | invalid | ✓ | `(root)` | — |
| wrong-type-string-vs-number | invalid | invalid | ✓ | `/count` | — |
| optional-absent | valid | valid | ✓ | — | — |
| extra-prop-still-valid | valid | valid | ✓ | — | — |
| enum-member / enum-nonmember | valid/invalid | valid/invalid | ✓ | `/kind` | — |
| integer-accepts-1.0 | valid | valid | ✓ | — | — |
| integer-rejects-1.5 | invalid | invalid | ✓ | `/n` | — |
| number-accepts-int | valid | valid | ✓ | — | — |
| nested-valid / nested-error | valid/invalid | valid/invalid | ✓ | `/outer/inner` | — |
| array-item-valid / array-item-error | valid/invalid | valid/invalid | ✓ | `/items/1` | — |
| **tuple-items-valid / tuple-2nd-wrong** | valid/invalid | valid/invalid | ✓ | `/pair/1` | — |
| type-union string/null/violation | valid/valid/invalid | = | ✓ | `/x` | — |
| anyOf-match / anyOf-violation | valid/invalid | valid/invalid | ✓ | `/v` | — |
| defs-ref-valid / defs-ref-error | valid/invalid | valid/invalid | ✓ | `/a` | — |
| **failsafe-bad-ref** (`#/$defs/missing`) | valid + hook | valid + hook | ✓ | — | ✓ |
| **failsafe-bad-ref-url** (unresolvable `$ref`) | valid + hook | valid + hook | ✓ | — | ✓ |
| **failsafe-malformed-type** (`type:12345`) | valid + hook | valid + hook | ✓ | — | ✓ |

**Result: 26/26 verdicts match · 26/26 hook-fired states match · 10/10 Invalid JSON-Pointer paths byte-identical.**

### Kill-criterion checks (no-downgrade risks) — all CLEARED
- **Draft-07 pin holds.** The decisive divergence probe — `items: [tuple]` — yields tuple
  semantics (`/pair/1` Invalid) on BOTH. Under 2020-12 `items`-as-array is not tuple validation;
  the Rust pin reproduces Ajv-8 draft-07 exactly.
- **integer semantics match:** `1.0` accepts, `1.5` rejects on both.
- **Fail-SAFE holds (the critical one):** all 3 uncompilable schemas — bad local `$ref`,
  unresolvable URL `$ref`, and a malformed `type:12345` — return **Valid + fire `on_compile_error`**
  on both engines. The Rust validator never REJECTS or PANICS where Ajv fail-safes. (The crate
  rejects malformed `type` at build → `Err` → fail-safe branch, matching Ajv's `.compile` throw.)
- **format_schema_errors shape:** root → `(root)`; scoped → `/count`; missing-required line
  `(root): "summary" is a required property` (starts `(root):`, contains the property name);
  empty error list → `["value does not match the declared schema"]`. All match the source contract.

### `- [≠]` divergences — CONFIRMED non-load-bearing (do NOT block)
- **`WF-31-msg-wording`** — per-error English differs (Ajv `must have required property 'summary'`
  vs crate `"summary" is a required property`; Ajv `must be number` vs crate `… is not of type "number"`).
  **Verified non-load-bearing:** the sole consumer (`packages/workflows/src/dag-executor.ts:1187-1231`)
  only does `validation.errors.join('; ')` into a reask prompt, a log field, and a thrown message —
  **no branch/assert on message text.** Path + property-name presence (the contract) are identical.
- **`WF-31-allerrors-order` (+count)** — anyOf-violation: Ajv emits 3 lines (string/number/anyOf),
  Rust emits 1 (the anyOf summary). Verdict (Invalid) and path (`/v`) identical; no test/consumer
  asserts order or count; reask just lists them. Within the pre-approved envelope.
- **`WF-31-no-cache`** — per-call compile vs Ajv WeakMap. Observably identical (deterministic in the
  schema); confirmed neutral.

### Rollup coverage
- `validate_structured_output`: all branches — Valid, Invalid{errors}, fail-safe `Err`→Valid+hook (×3 schema classes).
- `format_schema_errors`: `(root)`, `/pointer`, missing-required-with-name, empty→generic line.
- `StructuredValidationResult`: both variants exercised; 2-variant union faithful (compile-error → Valid, not a 3rd variant).
- Crate unit tests: **40 passed, 0 failed.**

**Nothing blocks the ledger flip.** Consumer wiring at `dag_executor.rs:4316` is correctly DEFERRED
to WF-09 s4c (out of scope for cycle-38).
