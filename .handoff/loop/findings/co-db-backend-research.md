# CO DB Backend Research — Cycle 27 Decision File

> Research date: 2026-06-21
> Researcher: rust-port-researcher agent (sonnet)
> Feeds: cycle 27 implementer, architect
> Question: Which Rust DB driver(s) implement the `Database` trait (`query`, `with_transaction`, LISTEN/NOTIFY) for the SQLite + Postgres backends, and is there a pure-Rust-native upgrade option that preserves full behavior?

---

## 1. Context recap (what cycle 26 established)

`har-db` cycle 26 ported the **dialect layer only**: `QueryResult`, `Dialect`, `SqlDialect` trait +
`PostgresDialect` / `SqliteDialect` impls, and the `DbNotificationListener` trait shape.

Cycle 27 must add:
- The `Database` trait (port of `IDatabase` in `types.ts`) with `query<T>`, `with_transaction`, `close`, `dialect`, `sql`.
- Concrete `SqliteAdapter` and `PostgresAdapter` impls.
- The `connect` / `getDatabase` + `getDialect` factory logic (port of `connection.ts`).
- The `LISTEN/NOTIFY` mechanism wired into the Postgres adapter (port of `postgres.ts` lines 163–231).
- The WAL/busy-timeout/foreign-key PRAGMA init sequence (port of `sqlite.ts` lines 28–41).
- The `convertPlaceholders` concern (see §4 below).

---

## 2. Hard constraint (governing frame)

The adapter abstraction is **SQL-string-pass-through**: `query(sql: &str, params)` takes raw SQL
strings including backend-specific functions:

| Backend | Functions in live queries |
|---------|--------------------------|
| SQLite  | `json_patch`, `json_extract`, `julianday('now')`, `datetime('now', '-' \|\| $N \|\| ' days')`, `instr` |
| Postgres | `jsonb \|\|`, `col->'path' ? $N`, `NOW()`, `EXTRACT(EPOCH FROM ...)`, `INTERVAL` |

A backend MUST execute these SQL strings unchanged. Any candidate that cannot run these functions
is a **downgrade** (forbidden).

---

## 3. Candidate evaluation

### 3.1 sqlx (recommended — primary)

| Dimension | Assessment |
|-----------|-----------|
| Both SQLite + Postgres | YES — single crate, feature flags `sqlite` + `postgres`. One dep tree. |
| Executes EXACT dialect SQL | YES — both drivers pass raw SQL strings to the underlying engine. SQLite uses bundled C-SQLite (3.49+). `json_patch`, `json_extract`, `julianday`, `instr`, `datetime` are all SQLite builtins — executed natively. Postgres passes raw SQL to the server. |
| Async | YES — tokio-native from the ground up. |
| LISTEN/NOTIFY | YES — `sqlx::postgres::PgListener` in the `postgres` feature. Dedicated connection, auto-reconnect, `recv()` / `try_recv()` / `into_stream()`. Exact behavioral match to Archon's `listen()` API. |
| Compile-time checked | YES (optional `query!` macro) — not needed for the raw-SQL-string adapter pattern, but available. |
| Pure Rust | PARTIALLY — Postgres driver is pure Rust. SQLite driver wraps **bundled C-SQLite** (or system SQLite with `sqlite-unbundled`). Not pure Rust for the SQLite path. |
| Maturity | VERY HIGH — current version **0.9.0** (released 2026-05-06), 0.8.x stable series before it. Industry standard for Rust async SQL. |
| Transaction model | `Pool::begin()` → `Transaction`; `Acquire` trait covers both Pool + Transaction. Not object-safe as `dyn`, but the `Database` trait wrapping it can use `impl Acquire` or a boxed-connection approach. See §5 (transaction design note). |
| License | MIT/Apache-2.0 |

**Citation:** https://crates.io/crates/sqlx (v0.9.0, 2026-05-06); https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgListener.html

### 3.2 rusqlite (SQLite only — insufficient)

Mature C-SQLite binding, synchronous only. No async. No Postgres.

| Dimension | Assessment |
|-----------|-----------|
| Both SQLite + Postgres | NO (SQLite only) |
| Async | NO |
| LISTEN/NOTIFY | N/A |

**Verdict: OUT.** Does not meet the dual-backend + async requirement. Useful only as a reference
for PRAGMA sequencing, but sqlx-sqlite covers that.

**Citation:** https://crates.io/crates/rusqlite; https://aarambhdevhub.medium.com/rust-orms-in-2026-... (2026 comparison)

### 3.3 Turso / limbo (pure-Rust SQLite rewrite — DEFER, not production-ready)

The turso crate (`0.7.0-pre.10`, 2026-06-18; formerly codenamed "limbo") is Turso's full
pure-Rust rewrite of SQLite, async-native (tokio). The COMPAT.md confirms:

- `json_patch()` — YES, fully supported (RFC-7396 merge patch)
- `json_extract()` — YES
- `julianday()` — YES (with partial modifier support)
- `datetime()` — YES
- `instr()` — YES

Postgres equivalent: NOT provided — this is a SQLite engine only.

| Dimension | Assessment |
|-----------|-----------|
| Executes EXACT SQLite dialect SQL | YES for the listed functions |
| Both SQLite + Postgres | NO (SQLite only — still needs sqlx/pg for the Postgres side) |
| Async | YES (tokio) |
| LISTEN/NOTIFY | N/A |
| Pure Rust | YES — no C compiler needed |
| Maturity | BETA — `0.7.0-pre.10` (pre-release). "May still contain bugs and unexpected behavior." Turso themselves advise backups and caution with production data. |
| File format | Compatible with SQLite file format |

**Verdict: DEFER.** Turso/limbo is architecturally the right pure-Rust SQLite engine and covers
all required functions, but it is in pre-release beta as of June 2026. A `turso-rs` feature flag
behind the `Database` trait could be added once 1.0.0 ships — the trait boundary allows a
pluggable swap. NOT a blocker for cycle 27.

**Citation:** https://crates.io/crates/turso (v0.7.0-pre.10, 2026-06-18);
https://raw.githubusercontent.com/tursodatabase/limbo/main/COMPAT.md

### 3.4 libsql / turso cloud SDK (NOT the pure-Rust rewrite — different product)

`libsql` is an **open-source fork of C-SQLite** with embedded replica and remote-access features.
It is C-backed, not pure Rust. It requires a C compiler and depends on the SQLite C codebase.
Cloud-sync adds operational complexity unsuited to the har-db standalone use case.

**Verdict: OUT.** Not pure Rust; adds operational overhead; not relevant to the dual-backend
portability goal.

**Citation:** https://lib.rs/crates/libsql; https://docs.rs/libsql

### 3.5 GlueSQL (pure-Rust SQL over sled/redb/memory — DOES NOT cover required functions)

GlueSQL v0.19.0 (2026-01-11) is a pure-Rust SQL engine supporting sled, redb, memory, Redis,
and other backends. However:

- Its function coverage is its own SQL dialect — it is NOT SQLite-compatible.
- No evidence of `json_patch`, `json_extract`, `julianday`, or `instr` in its function set
  (these are SQLite built-ins; GlueSQL issue #549 requested JSON functions in 2022 and was
  closed, but the docs surface no SQLite-specific JSON/date functions).
- Its Date/Time category provides `EXTRACT` but not SQLite's `julianday`/`datetime` forms.
- It does NOT support Postgres JSONB operators (`col->'path' ? $N`, `jsonb ||`).

Running Archon's raw SQL strings through GlueSQL would silently fail or error on every
JSON/date/interval expression — a downgrade.

**Verdict: OUT.** GlueSQL cannot execute Archon's dialect SQL. Using it would require
reimplementing every SQL expression — a prohibited behavior downgrade.

**Citation:** https://crates.io/crates/gluesql (v0.19.0); https://github.com/gluesql/gluesql/issues/549;
https://gluesql.org/docs/0.16.0/

### 3.6 redb / sled / native_db (KV stores — NOT SQL)

`redb` (pure-Rust B-tree KV), `sled` (pure-Rust LSM-tree KV), `native_db` (struct-typed KV with
indexes). None provide a SQL engine. They cannot execute `SELECT … WHERE json_extract(col, '$.x') = $1`.

GlueSQL *uses* these as pluggable storage backends, but GlueSQL itself fails the dialect test (§3.5).

**Verdict: OUT.** Not SQL engines; using any of them would require a SQL layer on top, and the
only available SQL layer (GlueSQL) does not support Archon's dialect.

**Citation:** https://www.redb.org/; https://lib.rs/database

### 3.7 rust+lua angle (mlua / rlua)

The "rust+lua" option was likely thinking of Lua as a stored-procedure/scripting layer inside the
DB (similar to Redis scripting). This is not applicable here: Archon uses no stored procedures,
no Lua scripting, and the entire adapter contract is raw SQL strings over a standard relational DB.

`mlua` / `rlua` are Lua binding crates for embedding the Lua interpreter in Rust programs — not
a database. rlua is deprecated in favor of mlua. Neither provides a SQL execution engine.

**Verdict: OUT / irrelevant.** No database functionality; not a SQL engine.

**Citation:** https://github.com/mlua-rs/mlua

---

## 4. The `convertPlaceholders` concern — KEY FINDING

Archon's `sqlite.ts` implements a `convertPlaceholders($N → ?)` step because `bun:sqlite` uses
`?` / `?1` positional placeholders, not `$N`.

**sqlx-sqlite natively uses `$1, $2, ...` syntax.** From the sqlx docs:
> "SQLite technically supports MySQL's syntax as well as others, but we recommend using this
> syntax [$N] as SQLx's SQLite driver is written with it in mind."

This means:
1. The dialect methods in `SqliteDialect` — which emit `$N` (e.g. `json_patch(col, $3)`) —
   are **already in the correct form** for sqlx-sqlite. No translation step needed.
2. The `::jsonb` and `::INTERVAL` cast-stripping in Archon's `convertPlaceholders` IS still
   needed for SQLite (SQLite does not understand `::jsonb` type casts). The sqlx-sqlite driver
   does not auto-strip Postgres type casts. Cycle 27 must ensure that `$N::jsonb` and
   `$N::INTERVAL` fragments do not appear in SQLite-routed SQL — handled by the dialect methods
   (which already emit clean SQLite SQL), so this is NOT an issue in practice.
3. The `convertPlaceholders` *as a whole function* is NOT needed in the Rust port when using
   sqlx. The sqlx-sqlite layer accepts `$N` directly, and the dialect methods already emit
   backend-clean SQL.

**Cite:** https://docs.rs/sqlx/latest/sqlx/fn.query.html ("SQLite technically supports MySQL's
syntax as well as others, but we recommend using this syntax as SQLx's SQLite driver is written
with it in mind."); https://github.com/oven-sh/bun/discussions/11142 (bun:sqlite uses `?1`/named,
confirming the TS code's `?` target was bun-specific).

---

## 5. Transaction design note — object-safety gap

Archon's `withTransaction` takes a callback: `fn(query) -> Promise<T>` where `query` is a
transaction-scoped function with the same `query<T>(sql, params)` signature as the outer adapter.

sqlx's `Executor` trait is **NOT object-safe** (`dyn Executor` is not allowed). The standard
sqlx pattern is generics (`impl Executor<Database = Sqlite>`) or `Acquire` trait. The Rust port
must either:

- (a) Make `Database::with_transaction` generic over the callback (HRTB / `for<'c>` + `impl Executor`).
- (b) Use a concrete connection type inside the closure (e.g. `SqliteConnection` or `PgConnection`)
  rather than the abstract `Database`.
- (c) Use `sqlx::any::AnyConnection` for a runtime-erased single-driver path (at cost of compile-time
  checking).

Option (a) is the idiomatic sqlx approach. The `async_trait` macro will be required for
the outer `Database` trait to remain `dyn`-capable for the caller-facing `IDatabase` equivalent.

This is a design decision the architect must make in cycle 27. It is flagged here as a known
open question, not a blocker on choosing sqlx.

---

## 6. Recommendation table

| Option | Exact dialect SQL? | Both backends? | Async? | LISTEN/NOTIFY? | Pure Rust? | Maturity | Compile-checked? |
|--------|-------------------|----------------|--------|----------------|------------|----------|-----------------|
| **sqlx** (PRIMARY) | YES | YES | YES | YES (PgListener) | Partial (pg=yes, sqlite=C) | v0.9.0 stable | YES (optional) |
| rusqlite | YES (SQLite only) | NO | NO | NO | NO (C) | v0.32.x stable | NO |
| Turso / limbo | YES (SQLite only) | NO | YES | NO | YES | v0.7.0-pre.10 BETA | NO |
| libsql | YES (SQLite fork) | NO | YES | NO | NO (C fork) | v0.6.x | NO |
| GlueSQL | PARTIAL — no json_patch/julianday/jsonb | NO (own dialect) | YES | NO | YES | v0.19.0 | NO |
| redb/sled | NOT SQL | NO | YES | NO | YES | stable | NO |
| mlua/lua | NOT SQL | NO | N/A | NO | NO | stable | NO |

---

## 7. PRIMARY recommendation

**Use sqlx `0.9.x` with feature flags `sqlite` + `postgres` + `runtime-tokio-native-tls`.**

Rationale:
- Only option that covers BOTH backends under a single `Database` trait.
- Executes the exact dialect SQL (Postgres JSONB, SQLite json_patch/julianday/instr) by passing
  raw strings to the real SQLite engine (bundled C-SQLite) and real Postgres server.
- `PgListener` provides exact behavioral parity with Archon's `LISTEN/NOTIFY` seam.
- `$N` placeholder syntax is sqlx-sqlite's native form — `convertPlaceholders` is eliminated.
- Industry standard; 0.9.0 just shipped (2026-05-06); well-maintained.
- Not pure Rust for the SQLite path (bundled C-SQLite), but that is acceptable — the
  constraint was "prefer pure-Rust where it PRESERVES behavior"; no pure-Rust SQLite
  alternative is production-ready as of this date (Turso is beta).

**Workspace dependency block (cycle 27 Cargo.toml additions):**
```toml
sqlx = { version = "0.9", features = ["runtime-tokio-native-tls", "sqlite", "postgres", "uuid", "chrono"] }
```

---

## 8. Pure-Rust native upgrade path

**Recommendation: DEFER to post-1.0 Turso.**

When Turso reaches `1.0.0` (pure-Rust SQLite with full `json_patch`/`julianday`/`instr`
coverage, stable async API), the SQLite adapter can be swapped to Turso behind the same
`Database` trait without touching calling code. The trait boundary is the seam.

No pure-Rust backend qualifies TODAY:
- Turso: right functions, wrong maturity (beta pre-release as of 2026-06-18).
- GlueSQL: wrong functions (no SQLite dialect compatibility).
- redb/sled/native_db: not SQL engines.

The upgrade path is: land sqlx now → define a `DbBackend` feature flag later → swap in Turso
when it ships 1.0. Mark this as a deferred TODO in `har-db/src/lib.rs`.

---

## 9. ruvector ruling

Per owner's standing rule: read `RUVECTOR-CRATE-LEDGER.md`, `RUVECTOR-RUNBOOK.md`,
`RUVECTOR-RESEARCH.md` before ruling.

**Finding:** RuVector is a distributed cognitive vector DB + AGI runtime (HNSW, SIMD,
quantization, REDB as storage substrate, RVF binary vector format, hypergraph DB, LLM serving).
It has no relational SQL engine. Its storage substrate (`redb`) is a KV B-tree. The `rvf-runtime`
crate is a binary vector store (segments + HNSW + witness chain), not a SQL layer.

**Source:** `/home/drdave/Desktop/meta/RUVECTOR-CRATE-LEDGER.md` line 1 ("314 crates... vector
DB + AGI runtime"), `RUVECTOR-RESEARCH.md` lines 23–29 ("Vector core — ruvector-core (HNSW, SIMD/
SimSIMD, quantization, REDB)... NOT 'just' a vector DB... no SQL layer").

**Verdict: OUT for this use case.** RuVector provides no SQL execution capability and cannot run
Archon's dialect SQL. It is the correct substrate for vector/embedding storage (a different
har-memory concern), not the relational DB layer.

---

## 10. Open questions for architect (cycle 27)

1. **Transaction object-safety design**: How does `Database::with_transaction` present the
   inner query function? Options: (a) HRTB generic closure over sqlx `Executor`, (b) concrete
   connection type in closure, (c) channel-based. This shapes the `Database` trait definition.
   Recommend (a) with `async_trait`.

2. **Feature flags**: Should `sqlite` and `postgres` be separate Cargo features in `har-db`
   (compile-time backend selection) or both always compiled? Archon's runtime is runtime-
   selection (`DATABASE_URL` env var). Runtime selection favors "both always compiled" (simpler,
   matches source semantics). Compile-time selection reduces binary size but diverges from source.

3. **TLS choice**: `runtime-tokio-native-tls` vs `runtime-tokio-rustls`. rustls is pure Rust
   (no OpenSSL dep); native-tls links platform TLS. For a server environment, either works.
   Rustls preferred for no-C purity on the Postgres side.

4. **Turso upgrade marker**: Should the TODO comment naming Turso as the future SQLite backend
   live in `har-db/src/adapters.rs` or in a `LESSONS.md` entry?

---

## Sources

- sqlx v0.9.0 changelog: https://docs.rs/crate/sqlx/latest/source/CHANGELOG.md
- sqlx placeholder docs: https://docs.rs/sqlx/latest/sqlx/fn.query.html
- sqlx PgListener: https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgListener.html
- sqlx transaction/Acquire pattern: https://github.com/launchbadge/sqlx/discussions/1442
- Turso (limbo) crate: https://crates.io/crates/turso (v0.7.0-pre.10, 2026-06-18)
- Turso COMPAT.md: https://raw.githubusercontent.com/tursodatabase/limbo/main/COMPAT.md
- GlueSQL v0.19.0: https://crates.io/crates/gluesql
- GlueSQL JSON issue #549: https://github.com/gluesql/gluesql/issues/549
- Rust ORMs 2026 comparison: https://aarambhdevhub.medium.com/rust-orms-in-2026-...
- bun:sqlite placeholder discussion: https://github.com/oven-sh/bun/discussions/11142
- RuVector ledger: /home/drdave/Desktop/meta/RUVECTOR-CRATE-LEDGER.md
- RuVector research: /home/drdave/Desktop/meta/RUVECTOR-RESEARCH.md
- Archon postgres.ts dialect (lines 237-261): /home/drdave/Desktop/meta/Archon/packages/core/src/db/adapters/postgres.ts
- Archon sqlite.ts dialect + convertPlaceholders (lines 119-145, 522-550): /home/drdave/Desktop/meta/Archon/packages/core/src/db/adapters/sqlite.ts
- Archon connection.ts auto-detect: /home/drdave/Desktop/meta/Archon/packages/core/src/db/connection.ts
- har-db cycle 26 state: /home/drdave/Desktop/meta/harness-agent-rs/crates/har-db/src/adapters.rs
