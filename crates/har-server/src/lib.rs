//! har-server — Axum multi-surface control plane.
//!
//! Ports Archon `packages/server/src/*`:
//!   - `index.ts`             → `build_app()` axum Router + startup logic
//!   - `routes/api.ts`        → REST routes including SSE `/api/stream/:conversationId`
//!   - `routes/schemas/*.ts`  → request/response schema structs (serde)
//!   - `auth/*.ts`            → auth middleware (JWT / session)
//!   - `adapters/web/*.ts`    → web bridge (WebSocket or HTTP long-poll)
//!   - `github-auth-bootstrap.ts` → GitHub OAuth bootstrap
//!
//! Key behaviors:
//!   - SSE `/api/stream/:conversationId` → axum `Sse<impl Stream>` fed from
//!     `tokio::sync::broadcast` subscriber (replaces pg-notify → SSE fan-out)
//!   - Dashboard stream → broadcast channel (not Postgres NOTIFY)
//!   - Durable state via har-ledger (hf); NOT a direct DB connection
//!
//! ADR-0001 R3: the control-plane REST/SSE surface is ported fresh on axum;
//! its event stream is MAP'd onto hf/weave rather than Postgres/pg-notify.
//! If an existing FlexNetOS control-plane substrate is identified, this may become
//! `map-onto-substrate` — flag to owner before wiring the substrate.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 15.
