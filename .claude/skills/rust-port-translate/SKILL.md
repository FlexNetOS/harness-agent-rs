---
name: rust-port-translate
description: >-
  How to port source-language constructs to idiomatic Rust with NO capability downgrade — error
  model, async, ownership, traits, generics, serialization, and dependency equivalents. ALWAYS use
  when writing the Rust for a ported unit, deciding the target architecture/idiom map, or choosing a
  Rust crate equivalent for a source library. Triggers on "port this to Rust", "idiomatic Rust for
  <construct>", "Rust equivalent of <lib>", "how to translate <pattern>". Idiomatic form, identical behavior.
---

# Rust-Port Translate

Re-express source behavior in idiomatic Rust **without losing capability**. A port is not a
transliteration (no `unwrap()` ladders, no god-enums) and not a downgrade (no dropped branches, no
"simpler synchronous version"). Used by `rust-port-architect` (decide once) and `rust-port-porter`
(apply per unit).

## Cross-cutting decisions (made once by the architect, in `target-architecture.md`)

| Concern | Idiomatic Rust |
|---------|----------------|
| Errors / exceptions | `Result<T, E>` + `?`; `thiserror` enums for libs, `anyhow` at bins. Every source `catch`/`except`/early-return → a handled arm. |
| Async / promises | `tokio` + `async/await`; streams → `futures::Stream`/`tokio_stream`. If the source streams, the Rust streams. |
| Interfaces / duck typing | traits + generics or `dyn Trait`; structural typing → explicit trait bounds. |
| Classes / inheritance | structs + traits + composition; avoid deep enums-as-hierarchy without thought. |
| Shared mutable state | ownership + borrows; `Arc<Mutex<_>>`/`RwLock` or channels where the source shares state. |
| Serialization | `serde` (derive); preserve exact wire/JSON shape and field names (parity). |
| Dynamic (decorators, monkey-patch, reflection) | macros, trait objects, or explicit registration — reproduce the *effect*, not the mechanism. |

## Dependency equivalents (record in the dep table; missing ≠ drop)

Map each source lib to a Rust crate (e.g. express→axum, fastapi→axum, prisma/sequelize→sqlx/sea-orm,
pydantic→serde+validator, zod→serde+validator, jest/vitest→cargo test+insta). **No equivalent is
never grounds to drop the feature** — decide vendor / reimplement / FFI and record why. That decision
is the architect's; the porter applies it.

## The no-downgrade rules (porter)

- **Every branch ports.** Each conditional, error path, and early return becomes a handled Rust path.
- **No stubs.** `todo!()`, `unimplemented!()`, default-to-skip, or narrowing a type to dodge a case
  are downgrades — leave the ledger row `- [~]` with what's missing instead of a fake `- [x]`.
- **Behavior matches, form modernizes.** Observable behavior (outputs, error kinds, side-effect
  timing, ordering where contractual) equals the source; the *expression* is idiomatic Rust.
- **Capability parity.** Streaming stays streaming; concurrency stays concurrent; hot-reload/plugin
  systems are designed in, not cut. Deliberate cuts are `- [≠]` rows with owner approval only.
- **Write the behavior tests** alongside the code — they become the parity fixtures.
