//! har-paths — Path resolution, env loading, structured logging, telemetry, and update-check.
//!
//! Ports Archon `packages/paths/src/*`:
//!   - `archon-paths.ts`   → `paths::resolve_*`, `get_archon_home()`, `is_docker()`, etc.
//!   - `env-loader.ts`     → `EnvLoader`
//!   - `strip-cwd-env.ts`  → path-stripping helpers for env vars
//!   - `logger.ts`         → `create_logger()` (wraps tracing-subscriber)
//!   - `telemetry.ts`      → telemetry capture fns (reqwest POST)
//!   - `update-check.ts`   → async version-check fn
//!   - `bundled-build.ts`  → `is_binary_build()` flag
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 2.
