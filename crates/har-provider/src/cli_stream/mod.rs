//! Shared CLI-subprocess helper — reusable by all CLI-delegated providers.
//!
//! Architecture §6.6: `cli_stream/` is the substrate that every CLI-delegated provider
//! (claude, codex, community) uses to spawn a CLI, stream its NDJSON stdout, classify
//! stderr, handle first-event timeout, and retry with exponential backoff.
//!
//! Sub-modules:
//! - `spawner`  — `Spawner` trait: real (`RealSpawner`) and fake (`FakeSpawner`) impls
//! - `stream`   — line-framed NDJSON reader over stdout bytes
//! - `stderr`   — stderr line classification (info banner vs error)
//! - `cancel`   — `CancellationToken` → child kill
//! - `retry`    — retry loop + exponential backoff + first-event timeout

pub mod cancel;
pub mod mcp_sidecar;
pub mod retry;
pub mod spawner;
pub mod stderr;
pub mod stream;

// Re-export the key public surface.
pub use cancel::{CancelGuard, TokioCancelToken};
pub use mcp_sidecar::{
    start_loopback, write_mcp_config_merged, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    McpHttpServer, McpSidecar,
};
pub use retry::{
    classify_and_enrich_error, classify_subprocess_error, with_first_message_timeout, EnrichedError,
    ErrorClass, FirstEventError, RetryConfig, RetryError,
};
pub use spawner::{
    FakeChildOutput, FakeSpawnScript, FakeSpawner, RealSpawner, SpawnOutcome, Spawner,
};
pub use stderr::{classify_stderr_line, StderrClass};
pub use stream::{NdjsonStream, StreamError};

/// Unified view of a child's output channels, supporting both real and fake spawns.
///
/// The retry loop drives this enum to produce a stream of NDJSON lines.
pub enum ChildOutput {
    Real(tokio::process::Child),
    Fake(FakeChildOutput),
}
