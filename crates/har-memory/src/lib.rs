//! har-memory — Cross-run memory surface. MAP'd onto `icm`.
//!
//! Ports whatever cross-run memory Archon persists (scope confirmed by cartographer).
//! The exact Archon units depend on `core/` memory/session-context paths — to be confirmed
//! in the next cartographer sweep.
//!
//! ADR-0001 MAP: `MemoryStore` trait is implemented over `icm` (cross-session memory substrate).
//! This crate drives `icm` via its CLI subprocess; it does NOT implement its own KV store.
//!
//! Status: STUB — NEEDS-HUMAN: cartographer to confirm which Archon paths use cross-run memory
//! before this unit is expanded. Will be filled in ITERATE cycle 8.
