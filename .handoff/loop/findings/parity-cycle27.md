# Parity verdict — cycle 27: CO-01 `Database` trait + `SqliteAdapter`

---

## ⟳ RE-VERIFICATION (2026-06-22) — after porter fixed D1 + D2 — **VERDICT: PASS**

Re-ran the differential oracle: **live bun 1.3.14** (`Archon/packages/core/src/db/adapters/sqlite.ts`)
vs the Rust `har_db::SqliteAdapter`, same battery, fresh temp DB per case, diffed rows + rowCount +
**full error-message text**. The bun oracle was rebuilt fresh (transient `_oracle_c27_reverify.ts`,
**DELETED** after the run — Archon source confirmed pristine, `git status` clean). The durable Rust
oracle `crates/har-db/examples/oracle_cycle27.rs` was extended to assert the **full** D1 message
byte-for-byte and the D2 PRAGMA/EXPLAIN fall-through.

### D1 — CLOSED ✅ (RETURNING-on-UPDATE/DELETE error embeds the CONVERTED `?` sql, byte-for-byte)

The porter added `convert_sql_for_error()` (`sqlite.rs:211-239`, the surviving `$N→?` +
`::jsonb`/`::INTERVAL` strip) and `dispatch_query` now builds `query_prefix` from
`convert_sql_for_error(sql).chars().take(100)` (`sqlite.rs:281-285`). Differential, full message text:

| input | bun 1.3.14 (oracle) | Rust (port) | byteMatch |
|-------|---------------------|-------------|-----------|
| `UPDATE … SET name = $1 … RETURNING id` | `…Query: UPDATE remote_agent_codebases SET name = ? RETURNING id... Hint:…` | identical | **true** |
| `DELETE … WHERE id = $1 RETURNING id` | `…Query: DELETE FROM remote_agent_codebases WHERE id = ? RETURNING id... Hint:…` | identical | **true** |

Both embed `?` (not `$1`), matching bun's `convertedSql.substring(0,100)`. The oracle asserts the
ENTIRE message string (leading sentence + `Query:` body + `Hint:`), not a substring — `byteMatch: true`.

### D2 — CLOSED ✅ (`PRAGMA`/`EXPLAIN` via `query()` falls through to the mutation path)

The porter reverted `is_select` to **SELECT/WITH only** (`sqlite.rs:269`, no `|| PRAGMA`). Differential:

| input | bun 1.3.14 (oracle) | Rust (port) | Match |
|-------|---------------------|-------------|-------|
| `query("PRAGMA table_info('remote_agent_users')")` | `rows=[], rowCount=0` | `rows=[], rowCount=0` | ✅ |
| `query("EXPLAIN SELECT 1")` | `rows=[], rowCount=0` | `rows=[], rowCount=0` | ✅ |

Internal `migrate_columns` is **UNAFFECTED** — it introspects via `pragma_table_info()`
(`sqlite.rs:436`, `sqlx::query(...).fetch_all()`) which bypasses the public dispatch, exactly as the
TS source calls `db.prepare("PRAGMA …").all()` directly. Proven by the **open-twice idempotency proxy**
(the old `query("PRAGMA …")` introspection can no longer be used as a probe — it's empty now by parity —
so the proxy USES a migrated column instead):

| input | bun (oracle) | Rust (port) | Match |
|-------|--------------|-------------|-------|
| open→close→open, then `INSERT … (id, role)` + `SELECT role` | `role="admin", rowCount=1` | `role="admin", rowCount=1` | ✅ |

If migration hadn't re-run idempotently on the 2nd open, the migrated `role` column wouldn't exist and
the INSERT would error. Both sides: column present exactly once, INSERT+SELECT succeed → idempotency green.

### No regression on the previously-PASSED battery (re-diffed live)

INSERT RETURNING ✅ · plain INSERT rowCount ✅ · UPDATE/DELETE rowCount ✅ · SELECT ✅ · WITH/CTE ✅ ·
json_patch/json_extract/instr/julianday/jsonArrayContains dialect exprs ✅ · convertPlaceholders proof
(out-of-order `$2 … $1` ✅, out-of-order INSERT `$2,$1,$3` ✅, repeated `$1 … $1` ✅) · transaction
commit ✅ · transaction rollback ✅ · schema_init idempotency (open twice) ✅. The only `[≈]` carry
remains B1 (`nowMinusDays` REAL → bun `1` vs serde `1.0`; same number, int-vs-float rendering — benign,
same class as the accepted rowCount `[≈]`).

### Build / clippy / fmt / test (exact, re-run 2026-06-22)

| Gate | Result |
|------|--------|
| `cargo build --workspace` | clean (0 errors) |
| `cargo clippy -p har-db --all-targets -- -D warnings` | **No issues found** |
| `cargo fmt --check` | clean (exit 0) |
| `cargo test -p har-db` | **31 passed; 0 failed; 0 ignored** (was 29 — +2 porter D1/D2 unit tests) |
| `cargo test --workspace` | **1596 passed; 0 failed; 11 ignored** (per-`test result:` aggregation; 11 ignored = pre-existing live-SDK seam tests, unrelated to CO-01) |

### Re-verification verdict

**PASS.** Both behavior-changing divergences are closed against the live source: D1 message is now
byte-identical to bun on parameterized mutations, D2 dispatch matches bun (PRAGMA/EXPLAIN empty via
`query()`) while internal migration is intact and idempotent. No previously-passing case regressed.
The convertPlaceholders elimination remains proven-safe (carried). CO-01 SQLite-bound symbols are now
clear to flip `- [x]` (the gate does not edit symbol-map.md / ledger / loop_state per the owner rule;
this verdict authorizes the orchestrator to do so). pg/connection items stay deferred to cycle 28.

Oracle artifacts: bun oracle transient + **DELETED**; Rust oracle durable at
`crates/har-db/examples/oracle_cycle27.rs` (extended with full-message D1 + D2-empty + migrate-proxy
assertions; fmt + clippy clean). No commit performed (owner rule).

---

## (original gate run — superseded by the re-verification above)


> Gate run 2026-06-21 by the parity-verifier (the GATE). Differential oracle:
> live bun 1.3.14 `SqliteAdapter` (from `Archon/packages/core/src/db/adapters/sqlite.ts`)
> vs the Rust `har_db::SqliteAdapter` — SAME battery, fresh temp DB per case, diffed
> rows + rowCount + error text. Oracle = built independently by the gate; the porter's
> own green tests were NOT trusted as the oracle.

## VERDICT: **FAIL** (2 behavior-changing divergences, neither an approved `[≠]`)

Both divergences are direct consequences of the porter's two query-path port decisions.
Build/clippy/fmt/tests are all green, but a green build is necessary, not sufficient.
Route back to the porter to fix D1 and D2. The convertPlaceholders drop (the highest-priority
risk) is **PROVEN SAFE** — see below.

---

## Build / clippy / fmt / test numbers (exact)

| Gate | Result |
|------|--------|
| `cargo build --workspace` | clean (0 errors) |
| `cargo clippy -p har-db --all-targets -- -D warnings` | **No issues found** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **No issues found** |
| `cargo fmt -p har-db --check` | clean |
| `cargo fmt --all --check` | clean |
| `cargo test -p har-db` | **29 passed; 0 failed; 0 ignored** |
| `cargo test --workspace` | **1594 passed; 0 failed; 11 ignored** (11 ignored = pre-existing live-SDK seam tests from PR-09/10/11, unrelated to CO-01) |

The build is green. The FAIL is the **live differential diff**, not the port's own tests.

---

## Case-by-case diff table (bun = oracle/source, rust = port)

| # | Case | bun (source) | rust (port) | Match? |
|---|------|--------------|-------------|--------|
| 1 | INSERT … RETURNING | rows=[{id:cb-ret,name:rettest}] rowCount=1 | same | ✅ |
| 2 | plain INSERT | rows=[] rowCount=1 (changes) | same | ✅ |
| 3a | UPDATE | rows=[] rowCount=1 | same | ✅ |
| 3b | DELETE | rows=[] rowCount=1 | same | ✅ |
| 4a | SELECT | rows=[{id,name}] rowCount=1 | same | ✅ |
| 4b | WITH/CTE SELECT | rows=[{id,name}] rowCount=1 | same | ✅ |
| 5a | RETURNING on UPDATE → throw | msg embeds `…SET name = ? RETURNING id…` | msg embeds `…SET name = $1 RETURNING id…` | ❌ **D1** |
| 5b | RETURNING on DELETE → throw | msg embeds `…id = ? RETURNING id…` | msg embeds `…id = $1 RETURNING id…` | ❌ **D1** |
| 6a | json_patch | `{"a":1,"b":2}` rowCount=1 | same | ✅ |
| 6b | json_extract | `value` | same | ✅ |
| 6c | instr | pos=7 | same | ✅ |
| 6d | julianday('now')>2400000 | gt=1 | gt=1 | ✅ |
| 6e | nowMinusDays diff (datetime('now','-'||$1||' days')) | diff=`1` | diff=`1.0` | ⚠️ **B1** (benign `[≈]`) |
| 6f | jsonArrayContains (instr(json_extract())>0) | contained=1 | contained=1 | ✅ |
| 7a | out-of-order `$2 … $1` SELECT | a="two", b="one" | same | ✅ **convertPlaceholders proof** |
| 7b | out-of-order `$2,$1,$3` INSERT | id=the-id, name=the-name | same | ✅ **convertPlaceholders proof** |
| 7c | repeated `$1 … $1` | a="hello", b="hello" | same | ✅ **convertPlaceholders proof** |
| 8 | migrateColumns idempotency via `query("PRAGMA table_info…")` | rowCount=0, rows=[] | rowCount=14, rows=[…cols…] | ❌ **D2** |
| 8b | schema init idempotency (open twice) | no error | no error | ✅ |
| 9 | transaction commit | ret="done", row found | same | ✅ |
| 10 | transaction rollback | row absent (rowCount=0) | same | ✅ |

---

## convertPlaceholders proof (HIGHEST-PRIORITY RISK) — **PASS**

The porter eliminated Archon's `convertPlaceholders` ($N→? rewrite + param reorder),
claiming sqlx-sqlite resolves `$N` by index natively. The gate proved this INDEPENDENTLY,
not from the report:

- **Out-of-order `$2 … $1` (SELECT projection)** — bun (uses the TS reorder) → `a="two", b="one"`;
  Rust (relies on sqlx) → `a="two", b="one"`. **Identical.**
- **Out-of-order in an INSERT** (`VALUES ($2, $1, $3)` with params `[the-name, the-id, /ooo]`) —
  both store `id=the-id, name=the-name`. **Identical** — sqlx binds `$2→args[1]`, `$1→args[0]`.
- **Repeated `$1 … $1`** — both yield `a="hello", b="hello"`. **Identical** — sqlx reads `args[0]` twice.

Conclusion: dropping `convertPlaceholders` is a **faithful elimination, not a downgrade**.
The `::jsonb`/`::INTERVAL` strip is moot for SQLite-routed SQL (SqliteDialect emits clean SQLite).
This is the one decision the spec flagged as the real parity risk — it is **CLEARED**.

---

## Divergences

### D1 (FAIL) — RETURNING-on-UPDATE/DELETE error message embeds the WRONG query text
- **Severity: behavior-changing (contractual error text).** Spec item 5 explicitly requires the
  exact message text be diffed and "Rust must match… flag any divergence."
- **Source** (`sqlite.ts:78-83`) builds the message from **`convertedSql`** (after `$1→?`):
  `… Query: UPDATE remote_agent_codebases SET name = ? RETURNING id... Hint: …`
- **Rust** (`sqlite.rs:231`, `error.rs:33-41`) builds `query_prefix` from the **raw** SQL's first
  100 chars (`sql.trim().chars().take(100)`), so it embeds `$1`:
  `… Query: UPDATE remote_agent_codebases SET name = $1 RETURNING id... Hint: …`
- **Root cause:** since the port dropped `convertPlaceholders`, there is no longer a `convertedSql`,
  so the porter substituted the raw SQL. The leading sentence + `Hint:` match (the substrings the
  unit tests pin), so the port's own tests pass — but the **embedded query string diverges** on any
  parameterized mutation, which is exactly the diff the gate is for.
- **Why it's not a benign `[≈]`:** the message is a contractual string (`error.rs` doc-comment itself
  says "Exact error messages are preserved… contractual"). The bytes differ on a real input. No
  `localize:` directive is in effect, and this is not inexpressible/non-contractual/superset — it is a
  faithfulness miss.
- **Fix (porter):** make the error embed the placeholder-normalized query, i.e. apply the same
  `\$(\d+) → ?` substitution to the first-100-chars slice before formatting the message (the only
  surviving purpose of `convertPlaceholders` after the binding-reorder became unnecessary). One regex
  replace on the prefix string reproduces the source byte-for-byte. After the fix, re-run the oracle
  and confirm cases 5a/5b match.

### D2 (FAIL) — `query()` PRAGMA/EXPLAIN dispatch silently broadened (is_select includes PRAGMA)
- **Severity: behavior-changing (query dispatch contract), currently dormant for Archon callers but a
  real no-downgrade deviation — not an approved `[≠]`.**
- **Source** (`sqlite.ts:53-54`): `isSelect = trimmed.startsWith('SELECT') || startsWith('WITH')`.
  A `PRAGMA …` or `EXPLAIN …` issued through the public `query()` therefore takes the **mutation
  `.run()` path**, which **discards returned rows** → `{ rows: [], rowCount: changes }`. Probed live:
  `query("PRAGMA table_info(...)")` → `rowCount=0, rows=[]`; `PRAGMA journal_mode` → `rowCount=0,
  rows=[]`; `EXPLAIN SELECT 1` → `rowCount=0, rows=[]`.
- **Rust** (`sqlite.rs:217-219`) ADDED `|| trimmed_upper.starts_with("PRAGMA")` to `is_select`, so the
  same `query("PRAGMA table_info(...)")` returns the **full 14 introspection rows** (`rowCount=14`).
  Different observable output for the same input through the same public method.
- **Root cause:** the porter broadened the dispatch (comment: "PRAGMA can return rows") — a reasonable
  instinct, but it is a **deliberate deviation from the source dispatch logic** that was never recorded
  as a `[≠]` with owner approval. It changes the documented `query` contract.
- **Reachability check (done):** `grep` over all of `Archon/packages/**` (excluding node_modules) found
  **zero** production callers issuing `PRAGMA`/`EXPLAIN` through `query()` — the only hits were the
  gate's own transient oracle/probe. The internal `migrateColumns` introspection is UNAFFECTED on both
  sides: TS uses `db.prepare(...).all()` directly and Rust uses `sqlx::query(...).fetch_all()` via
  `pragma_table_info` (`sqlite.rs:389`), both bypassing the public dispatch. So **no current behavior
  is broken** — but the public `query()` method's behavior now differs from source, and a future
  caller (e.g. cycle-28 `connection.ts`, or a store method that introspects schema via `query()`) would
  silently get divergent results. Per the no-downgrade / "cover the contract, not the happy path" rule,
  a silent contract broadening that is observable must be either reverted to match source OR recorded as
  an approved `[≠]` with rationale + owner sign-off. As-is it is an unflagged divergence → FAIL.
- **Fix options (porter, either is acceptable):**
  1. **Match source (recommended):** remove `|| starts_with("PRAGMA")` from `is_select` so `PRAGMA`/
     `EXPLAIN` via `query()` take the execute path and return `rows=[], rowCount=changes`, byte-identical
     to bun. (Internal migration is unaffected — it never goes through dispatch.)
  2. **Keep the broadening but make it an approved `[≠]`:** record a ledger `[≠]` row stating "Rust
     `query()` returns rows for PRAGMA/EXPLAIN where the source discards them — a strict superset, no
     current caller relies on the source's empty result" and obtain owner approval. (Superset claim is
     defensible since the source path is lossy, but it still needs to be an explicit, signed `[≠]`, not
     a silent change — otherwise the next porter can't tell intent from accident.)

  After the fix, re-run the oracle; case 8 must either match bun (option 1) or be reclassified `[≠]`
  (option 2). The gate leans **option 1** (faithful to source; zero ambiguity).

### B1 (benign `[≈]`, carry) — nowMinusDays REAL value renders `1` (bun) vs `1.0` (rust)
- The `nowMinusDays`/`daysSince` dialect expressions produce a SQLite `REAL`. The numeric value is
  identical (exactly 1.0 day). bun's JS serializer prints the integral float as `1`; serde_json prints
  the `f64` as `1.0`. Same number, different JSON int-vs-float rendering — the same class as the
  already-accepted `rowCount: number` (TS) vs `u64` (Rust) `[≈]`. **Benign, behavior-preserving.** No
  fix required; carry as `[≈]` (REAL value rendering) alongside the rowCount `[≈]`.

---

## Symbol coverage (for the orchestrator; the gate did NOT edit symbol-map.md per owner rule)

Exercised and PASS (differentially): INSERT/INSERT…RETURNING/UPDATE/DELETE/SELECT/WITH dispatch,
native RETURNING on INSERT, `$N` binding incl. out-of-order + repeated (convertPlaceholders proof),
json_patch/json_extract/instr/julianday/nowMinusDays/jsonArrayContains dialect execution,
with_transaction commit + rollback, createSchema idempotency, migrateColumns idempotency (columns
present exactly once), object-safety of `dyn Database`.

Blocking the unit `- [x]`:
- **D1** — `ReturningNotSupportedOnMutation` message text symbol (`error.rs` / `sqlite.rs:231`).
- **D2** — `query()` dispatch symbol (`sqlite.rs:dispatch_query`, the PRAGMA broadening).

Until D1 and D2 are fixed (or D2 reclassified as an approved `[≠]`), CO-01 driver-bound SQLite items
must **NOT** be flipped `- [x]`; leave them open. pg/connection items stay `- [ ]` (correctly deferred
to cycle 28 — genuine scope boundary, connection.ts constructs both adapters).

---

## Oracle artifacts

- **bun oracle** (`Archon/packages/core/_oracle_cycle27.ts`) + **probe** (`_probe_pragma.ts`):
  transient, **DELETED** after the run. Archon source confirmed untouched (`git status` clean).
- **Rust oracle**: kept as a durable differential harness at
  `crates/har-db/examples/oracle_cycle27.rs` (run: `cargo run -p har-db --example oracle_cycle27`).
  fmt + clippy clean.
