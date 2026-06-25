# WF-31 `validate_structured_output` — Target Architecture (Ajv-fidelity decision)

**Status:** DECIDED. Supersedes the `!B3` note in `WF-09-s4-architecture.md`.
**Author:** rust-port-architect · **Date:** 2026-06-25 · **Scope:** design-only (no Rust written).
**Lands in:** existing `crates/har-provider/src/shared/structured_output.rs` (alongside the already-ported
`augment_prompt_for_json_schema` / `try_parse_structured_output`).
**Un-stubs:** `crates/har-dag-executor/src/dag_executor.rs:4316` (`let validation_valid = true;`) — wired in a
LATER cycle (WF-09 s4c), not here.

---

## 0 · Reconciliation — the `!B3` note was wrong (supersede)

The prior `!B3` handoff note said: *"port Archon's OWN hand-rolled validator, not a jsonschema crate."* That
note was written BEFORE reading the source. **The source disproves it.** `validateStructuredOutput`
(`structured-output.ts:278-298`) is NOT hand-rolled — it **delegates to Ajv 8**:

```ts
const ajv = new Ajv({ allErrors: true, strict: false });   // line 243
validate = ajv.compile(schema);                            // line 286
if (validate(value)) return { valid: true };
return { valid: false, errors: formatSchemaErrors(validate.errors) };
```

Only `formatSchemaErrors` (the Ajv-`ErrorObject[]` → `string[]` renderer) is hand-rolled. So the real fork is
**how to reproduce Ajv 8's validation behavior in Rust with no downgrade**, not "transliterate a custom
validator." The `!B3` note is hereby **superseded** by this document.

---

## 1 · DECISION — Approach (A): the `jsonschema` crate + a `format_schema_errors` adapter

**Chosen: (A)** — bind the real `jsonschema` crate as Ajv's Rust equivalent; hand-port ONLY
`format_schema_errors` as a thin adapter over the crate's `ValidationError` type.

**Rejected (B) hand-port a minimal validator:** large, must re-derive Ajv 8 semantics
(type/enum/required/anyOf/$defs/$ref/type-unions/number-vs-integer) by hand — high bug surface for a
behavior a maintained crate already implements. The no-downgrade rule does NOT require byte-exact Ajv
*message English* (see §3); it requires the same VERDICT, which a real validator gives for free.

**Rejected (C) EMBED Ajv via a JS runtime:** introduces a JS artifact + JS runtime dependency into a
pure-Rust leaf crate (`har-provider`) whose whole point is SDK-deps-only. Perfect message fidelity is not
worth a JS runtime on the hot validation path that runs for *every* provider. (No `runtime-constructs.md`
EMBED-precedent file is present in this repo to lean on; option C stays unchosen.)

### Crate-availability — RESOLVED (not a guess)

- `jsonschema = "0.46"` (current `0.46.6`; **`0.46.5` is already cached** in this machine's cargo registry:
  `~/.cargo/registry/.../jsonschema-0.46.5`). It is the maintained Rust JSON-Schema validator (ex-`jsonschema-valid` lineage, Stranger6667).
- It is **NOT yet in the workspace** — `grep -riE 'jsonschema|valico|boon' Cargo.lock Cargo.toml crates/*/Cargo.toml`
  returned empty. The porter MUST add it to `[workspace.dependencies]` in the root `Cargo.toml` and reference it
  `workspace = true` from `crates/har-provider/Cargo.toml`.
- API surface confirmed against the cached source (`jsonschema-0.46.5/src/lib.rs`):
  - `jsonschema::validator_for(&Value) -> Result<Validator, ValidationError<'static>>` — **the compile step**;
    its `Err` is the fail-SAFE trigger (maps to Ajv's `ajv.compile(schema)` throw).
  - `jsonschema::options() -> ValidationOptions` — **draft selection** (see §4 draft-mismatch flag).
  - `Validator::iter_errors(&Value) -> impl Iterator<ValidationError>` — the **`allErrors:true`** equivalent
    (yields every failure, not just the first).
  - `ValidationError.instance_path` — a JSON-Pointer location that `Display`s as `/count`, `` (empty at root) —
    the direct analogue of Ajv's `instancePath`.

**Ranked fallback (only if the porter finds the crate genuinely unbuildable in CI):**
1. (A) `jsonschema` 0.46 — chosen.
2. `boon` (pure-Rust, draft-4→2020-12, no_std-friendly) — equivalent capability, different error type → the
   adapter in §2 changes shape only.
3. (B) hand-port the draft-07 subset in §5 — last resort, byte-exact errors but large.
   `valico` is **not** recommended (older draft-4-era semantics; would be a downgrade vs Ajv 8 draft-07).

---

## 2 · The Rust shape (faithful to the source's discriminated union + side-effect hook)

```rust
/// Port of `StructuredValidationResult` (structured-output.ts:259). EXACTLY two variants —
/// the compile-error case maps to `Valid` (fail-safe), it is NOT a third variant.
pub enum StructuredValidationResult {
    Valid,
    Invalid { errors: Vec<String> },
}

/// Port of `validateStructuredOutput` (278-298).
/// `on_compile_error` is the Ajv `onCompileError?(message)` hook — an OPTIONAL mutating side-effect
/// (the dag-executor logs AND pushes a user-facing warning), so `&mut dyn FnMut(String)` is the
/// faithful Rust idiom (NOT `Fn` — the closure mutates the executor's warning sink).
pub fn validate_structured_output(
    value: &serde_json::Value,
    schema: &serde_json::Value,
    on_compile_error: Option<&mut dyn FnMut(String)>,
) -> StructuredValidationResult {
    // 1. compile (validator_for). Err => fail-SAFE: fire hook, return Valid.
    // 2. iter_errors(value): empty => Valid; else Invalid { errors: format_schema_errors(errs) }.
}

/// Port of `formatSchemaErrors` (306-316). Adapter: jsonschema::ValidationError -> `path: message`.
pub fn format_schema_errors<'a>(
    errors: impl IntoIterator<Item = jsonschema::ValidationError<'a>>,
) -> Vec<String> { /* see §3 mapping */ }
```

**Why the callback over a returned compile-error variant:** the source's type is *exactly* 2 variants and the
compile-error path returns `{valid:true}` *with* a side effect. Adding a 3rd Rust variant would diverge from
the source type and force the dag-executor to treat compile-error as non-valid — the opposite of fail-safe.
The callback preserves both the 2-variant contract AND the "log + warn but don't block" behavior. (If 4c finds
`&mut dyn FnMut` awkward to thread, the sanctioned alternative is to return
`(StructuredValidationResult, Option<String> /*compile_err*/)` while keeping the enum 2-variant — but the
callback is the recommended primary.)

---

## 3 · Fidelity contract (what the parity-verifier MUST hold the porter to)

### BYTE-EXACT REQUIRED (the contract — verbatim against the test oracle `structured-output.test.ts:127-191`)

1. **Verdict across the full matrix:** valid value → `Valid`; missing-required → `Invalid`; wrong-type →
   `Invalid`; enum-violation → `Invalid`; optional-absent → `Valid` (`additionalProperties` NOT required).
2. **Fail-SAFE on uncompilable schema:** an unresolvable `$ref` (`#/$defs/missing`) → `validator_for` returns
   `Err` → return `Valid` **and** fire `on_compile_error` with the message. An un-compilable schema must NEVER
   turn a correct response into a node failure. (Test: `r.valid == true` AND `compileError` is set.)
3. **`format_schema_errors` shape — `path: detail` per line:**
   - empty `instance_path` → **`(root)`** (Ajv empty `instancePath`); non-empty → the JSON-Pointer string
     (e.g. **`/count`** — the wrong-type test asserts the line `startsWith('/count')`).
   - **missing-required** lines must contain the property NAME and start with `(root):` (test asserts
     `line.startsWith('(root):') && line.includes('name')`). With `jsonschema`, the property name lives in the
     message text (the crate has no structured `params.missingProperty`); the `(root): <msg containing 'name'>`
     form satisfies the oracle. The porter must confirm the crate emits the missing-property error at the
     parent-object location (empty path → `(root)`), matching Ajv.
   - null/empty error list → the single generic line **`value does not match the declared schema`**
     (test: `formatSchemaErrors(null|[]) == ['value does not match the declared schema']`).

### BOUNDED `- [≠]` (pre-approved divergences the parity gate must ACCEPT)

- **`- [≠] WF-31-msg-wording`** — the exact English of a per-error message (Ajv `"must have required property
  'summary'"` vs `jsonschema` `"'summary' is a required property"`). The VERDICT and the `path` + the
  property-NAME presence are contractual; the surrounding English is not. These strings feed reask prompts, but
  the reask contract is "tell the model which path failed and why" — the path + name + a human-readable reason
  satisfy it; Ajv's precise phrasing is not load-bearing. **Rationale on file so the verifier does not
  fail-block on string-diff.**
- **`- [≠] WF-31-allerrors-order`** — when multiple errors exist, the ORDER of lines may differ from Ajv's. No
  test asserts order (`.some(...)`); the reask lists them all. Bounded-accept.
- **`- [≠] WF-31-no-cache`** — see §4; observably neutral.

---

## 4 · Risk flags (decisions the porter must apply)

1. **Draft mismatch — MUST pin Draft-07.** Ajv 8 defaults to **draft-07** when a schema omits `$schema` (real
   Archon `output_format` schemas omit it). The `jsonschema` crate, given no `$schema`, defaults to the LATEST
   draft (2020-12). For the subset Archon uses (object/properties/required/type/enum/array+items/anyOf/$defs/$ref/
   type-unions) draft-07 and 2020-12 behave identically — BUT to be faithful and future-proof, **explicitly select
   draft-07** via `jsonschema::options().with_draft(Draft::Draft7).build(schema)` (or the crate's draft7 module).
   Do NOT rely on the default. This is a fidelity REQUIREMENT, not a `- [≠]`.
2. **`strict:false` → ignore unknown keywords/formats.** Ajv `strict:false` makes unknown keywords non-fatal;
   Ajv also does NOT assert `format` (ajv-formats is not imported). The `jsonschema` crate by default treats
   unknown keywords leniently and treats `format` as annotation-only (no assertion) unless
   `should_validate_formats(true)`. **Leave format validation OFF** — matches Ajv. Confirm unknown-keyword
   leniency does not error at build (it must not — Archon author schemas carry dialect drift).
3. **`allErrors:true` → `iter_errors`, not `validate`.** Use `Validator::iter_errors` so every failure is
   surfaced for the reask (the crate's `validate`/`is_valid` short-circuit). Ordering is `- [≠]` (§3).
4. **Schema cache (Ajv `WeakMap` keyed by object identity) — OMIT for cycle-38.** Rust has no GC weak-ref over
   an arbitrary `&Value`, so the WeakMap-by-reference semantics cannot be replicated, AND they are **observably
   irrelevant**: compilation is deterministic in the schema, so per-call `validator_for` produces byte-identical
   output to a cached validator. Recommendation: **per-call compile** for the port (faithful + simplest).
   A perf cache (e.g. `HashMap<canonical-schema-string, Arc<Validator>>` behind a `Mutex`/`OnceLock`) is a
   PARITY-NEUTRAL later optimization — record as `- [≠] WF-31-no-cache` (no behavioral difference). Do NOT block
   cycle-38 on it.
5. **number-vs-integer.** Ajv `type:number` accepts ints+floats; `jsonschema` matches the spec identically.
   The oracle's `count:{type:'number'}` with `2` (valid) / `'two'` (invalid) agrees on both. Low risk.
6. **`$ref` handling = the fail-safe trigger.** Confirm the crate raises an `Err` at `validator_for` build time
   for an unresolvable local `$ref` (it does — unresolved reference is a build/compile error). Map that `Err`
   (not a panic) to the fail-safe Valid + `on_compile_error`. Belt-and-braces: ensure no `.unwrap()` on the
   build result.

### Schema features Archon's `output_format` ACTUALLY exercises (scope the coverage)

From the source normalizer (`normalizeJsonSchemaForOpenAiStrict`) + the test oracle, real schemas use only:
`type:'object'`, `type` UNIONS (`['object','null']`), `properties`, `required`, scalar `type` (`string`/`number`),
`enum`, `array` + `items`, `anyOf`, `$defs`, `$ref`, `additionalProperties`. The `OutputFormat.schema` field is a
`serde_json::Value` (`crates/har-provider/src/pi/native_tools.rs:31`). The chosen crate covers ALL of these in
draft-07 — coverage is scoped to REAL usage, not all of JSON Schema. (B)'s minimal-validator subset, if ever
needed, is exactly this list.

---

## 5 · Cycle-38 sub-plan — ONE cycle (no split)

~40 lines of new code; the only risk is the validator binding + the error adapter, both bounded and test-pinned.
The dag-executor wiring at `:4316`/`:2562` is a **separate later cycle (WF-09 s4c)** per the s4 plan, so cycle-38
is self-contained and does NOT touch `har-dag-executor`. **One cycle.**

**Ordered, parity-verifiable steps:**
1. Add `jsonschema = "0.46"` to root `[workspace.dependencies]`; reference `workspace = true` in
   `crates/har-provider/Cargo.toml`. `cargo build -p har-provider` green (proves crate availability in CI).
2. Add `StructuredValidationResult` (2-variant enum) to `structured_output.rs`.
3. Add `format_schema_errors` (adapter: `instance_path` → `(root)`/`/path`; generic line on empty).
4. Add `validate_structured_output` with the Draft-07 pinned `validator_for`, `iter_errors`, fail-safe `Err`
   branch + `on_compile_error` hook.
5. Port the test oracle 1:1 (the `describe('validateStructuredOutput')` + `describe('formatSchemaErrors')`
   blocks, lines 127-191): valid-passes, missing-required→`summary`, wrong-type→`/count`-prefix, enum, optional-
   absent, uncompilable-`$ref`→fail-safe+hook-fired, root-missing→`(root):`+name, null/empty→generic line.
6. `cargo clippy -p har-provider` + tests green. Parity-verify against `bun test structured-output.test.ts`
   (the 7 validate cases + 2 format cases) — verdict-exact; message-wording diffs allowed under
   `- [≠] WF-31-msg-wording`.

**Exact symbol list to land (cycle-38):**
- `StructuredValidationResult` (pub enum: `Valid` | `Invalid { errors: Vec<String> }`)
- `validate_structured_output(value, schema, on_compile_error: Option<&mut dyn FnMut(String)>) -> StructuredValidationResult`
- `format_schema_errors(errors) -> Vec<String>`
- onCompileError hook = the `Option<&mut dyn FnMut(String)>` param (NOT a new type)
- validator cache = **deferred** (parity-neutral; not in cycle-38)

**Deferred to WF-09 s4c (NOT cycle-38):** wiring `validate_structured_output` into `execute_node_internal` at
`dag_executor.rs:4316`, threading the `on_compile_error` closure to the executor's log + user-warning sink, and
the validate-and-reask loop (TS 1147-1255).
