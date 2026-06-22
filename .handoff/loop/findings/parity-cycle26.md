# Parity Verdict — Cycle 26 (UNIT CO-01, dialect layer)

**Date:** 2026-06-21
**Verifier:** rust-port-parity-verifier (differential gate, fail-closed)
**Unit:** CO-01 dialect layer → `crates/har-db` (`adapters.rs` + `lib.rs`)
**Source of truth:** `Archon/packages/core/src/db/adapters/{types.ts,postgres.ts,sqlite.ts}` (kept PRISTINE — confirmed `git status` clean)

## VERDICT: **PASS** (dialect layer)

All 6 dialect symbols differentially verified against the **live TS source** (run via `bun 1.3.14`, not the porter's reported strings). Deferred items confirmed as a genuine scope boundary (`- [ ]`, no stubs). Build health green (re-run independently).

---

## 1. Differential dialect-string diff (LIVE TS oracle vs Rust)

Method: a throwaway `bun` script imported the live `postgresDialect` / `sqliteDialect` and called every method over an input matrix (columns `metadata`/`col`/`created_at`, paths `related_issues`/`p`/`a.b`, param indices **1, 3, 10, 42** incl. multi-digit). A throwaway `cargo --example` ran the Rust equivalents over the identical matrix. Outputs diffed character-for-character.

**Result: 56/56 deterministic lines IDENTICAL — ZERO DIFF** (both dialects, full matrix). Every space, `$N`, quote, paren, and tail (`::jsonb`, `::INTERVAL`, `/ 86400`, `> 0`, `json_patch`, `julianday`, `instr`, `json_extract`) matched.

| method × input | TS oracle | Rust | match |
|---|---|---|---|
| **PG** `now()` | `NOW()` | `NOW()` | ✅ |
| **PG** `jsonMerge(metadata,3)` | `metadata \|\| $3::jsonb` | same | ✅ |
| **PG** `jsonMerge(col,10)` (multi-digit) | `col \|\| $10::jsonb` | same | ✅ |
| **PG** `jsonMerge(col,42)` (multi-digit) | `col \|\| $42::jsonb` | same | ✅ |
| **PG** `jsonArrayContains(metadata,related_issues,3)` | `metadata->'related_issues' ? $3` | same | ✅ |
| **PG** `jsonArrayContains(metadata,a.b,10)` (dotted path, multi-digit) | `metadata->'a.b' ? $10` | same | ✅ |
| **PG** `nowMinusDays(10)` | `NOW() - ($10 \|\| ' days')::INTERVAL` | same | ✅ |
| **PG** `daysSince(created_at)` | `EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400` | same | ✅ |
| **SQLite** `now()` | `datetime('now')` | same | ✅ |
| **SQLite** `jsonMerge(metadata,3)` | `json_patch(metadata, $3)` | same | ✅ |
| **SQLite** `jsonMerge(col,10)` (multi-digit) | `json_patch(col, $10)` | same | ✅ |
| **SQLite** `jsonArrayContains(metadata,related_issues,10)` | `instr(json_extract(metadata, '$.related_issues'), $10) > 0` | same | ✅ |
| **SQLite** `jsonArrayContains(col,a.b,3)` (dotted) | `instr(json_extract(col, '$.a.b'), $3) > 0` | same | ✅ |
| **SQLite** `nowMinusDays(42)` | `datetime('now', '-' \|\| $42 \|\| ' days')` | same | ✅ |
| **SQLite** `daysSince(created_at)` | `(julianday('now') - julianday(created_at))` | same | ✅ |

**Multi-digit confirmation:** TS `String(10)`→`"10"`, `String(42)`→`"42"`; Rust `usize` Display→`"10"`/`"42"`. Confirmed identical for `$10`/`$42` placeholders across all methods — no off-by-one or zero-pad divergence.

## 2. generateUuid — UUID v4 shape parity (can't byte-match; verify format+version)

- **TS** `crypto.randomUUID()` (confirmed `typeof === "function"`): sample `f07f5bfb-4bcf-4bde-af6d-273ae6d011a4` / `5ab9aeeb-d56b-4cd3-a802-5062b2cf71c1`. Regex `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$` → **v4_format=true**, lowercase, len 36. (`crypto.randomUUID` is RFC 4122 v4 by spec.)
- **Rust** `Uuid::new_v4()`: sample `717bbe36-ee95-4c9e-bbf7-5c5bcc7ee277` / `6640091e-98e5-4f57-8a26-05cfa8d8e7d2`. `get_version_num() == 4`, lowercase, len 36. Version nibble `4`, variant nibble ∈ `[89ab]` on both.
- **Parity:** both produce lowercase hyphenated 8-4-4-4-12 **v4** with correct version/variant bits. SHAPE-parity confirmed. `uuid` workspace dep has `features=["v4","serde"]` — v4 enabled.

## 3. Structural fidelity vs types.ts

- **QueryResult<T>** — `rows: Vec<T>` (TS `rows: readonly T[]`), `row_count: u64` (TS `rowCount: number`, per port's row-count idiom), `#[serde(rename="rowCount")]`. Round-trip test pins `"rowCount":3`. TS `readonly` → owned fields (Rust immutability via move/borrow). ✅ Faithful.
- **Dialect enum** — `{Postgres, Sqlite}`, `#[serde(rename_all="lowercase")]` → serializes to exactly `"postgres"`/`"sqlite"`; `as_str()` matches; round-trip both directions tested. ✅ Matches TS `'postgres' | 'sqlite'`.
- **SqlDialect trait** — independently counted **6/6** methods present in trait def AND implemented by both dialects: `generate_uuid`, `now`, `json_merge`, `json_array_contains`, `now_minus_days`, `days_since`. None dropped, none invented. Object-safe (smoke-tested via `Vec<Box<dyn SqlDialect>>`). ✅
- **DbNotificationListener trait** — shape mirrors types.ts:59-72: `listen(channel, on_notify: Box<dyn Fn(String)+Send+Sync>, on_error: Box<dyn Fn(NotificationError)+Send+Sync>) -> Box<dyn FnOnce()+Send>`. `listen`→returns unsubscribe (`Promise<()=>void>` → `FnOnce`), Postgres-only impl correctly deferred. Faithful idiom map; no behavior invented. ✅

## 4. No-downgrade / scope check

- **Stub scan:** `grep -E 'todo!|unimplemented!|panic!|unreachable!'` over `crates/har-db/src/` → **NONE**. No fake-done stubs.
- **Ledger discipline:** deferred items (CO-01a/b adapters, `Database::query`/`with_transaction`, pg `LISTEN/NOTIFY` impl, `getDatabaseType()`) are marked `- [ ]` in `parity-ledger.md:1131-1136` — NOT `- [x]`. The deferral is a **genuine scope boundary** (driver-crate decision pending in cycle 27), not a silent drop. The dialect surface is self-complete and driver-independent (pure string construction + UUID), so it ports at full parity now. This is a legitimate scope cut, not a `- [≠]` downgrade.
- **Method count:** 6 SqlDialect methods independently counted and all present. ✅

## 5. Build health (re-run independently — not trusted from porter)

- `cargo test -p har-db` → **10 passed, 0 failed** (2 suites).
- `cargo clippy -p har-db --all-targets -- -D warnings` → **clean (exit 0)**.
- `cargo build -p har-db` → **Finished, 0 errors**.

## Porter discrepancies caught

**None.** The porter's reported dialect strings, the source comments in `adapters.rs`, the ledger `- [ ]` deferral markings, and the symbol contracts all matched the live source and the differential run exactly. No invented behavior, no narrowing, no disguised feature-skip.

## `[≈]` / `[≠]` carries

- **`[≈]`** `QueryResult.row_count`: TS `number` → Rust `u64`. Benign — row counts are non-negative integers; consistent with the port's established row-count idiom. No observable divergence in serialized shape (`"rowCount": N`).
- **`[≠]`**: none.

## Symbol-map action

The 6 CO-01 dialect symbols (rows in `parity-ledger.md:1124-1129`) are differentially PROVEN and may flip `- [~]` → `- [x]`. The unit-level `symbol-map.md` CO-01 row (`IDatabaseAdapter` trait, line 589) stays `- [ ]` because the `Database`/`IDatabaseAdapter` trait surface itself (query/with_transaction) is the cycle-27 deferred portion — this cycle only completed the dialect sub-layer of CO-01, which the ledger correctly tracks as a partial. Unit CO-01 is NOT fully done (deferred items remain open); cycle 26's dialect-layer slice is PASS.

## Cleanup

Throwaway oracle script (`/tmp/oracle_cycle26.ts`) and throwaway Rust example (`crates/har-db/examples/oracle_cycle26.rs` + dir) DELETED. Archon tree confirmed pristine (`git status` clean). No commit made.
