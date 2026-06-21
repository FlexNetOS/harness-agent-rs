# Implementation log: OpenCode SDK HTTP/SSE bind (PR-21)

Replaced the `opencode_sdk_not_bound` seam with a real native embedded runtime + HTTP/SSE
client. The `@opencode-ai/sdk` Node.js dependency is gone — the Rust port now spawns the
`opencode serve` binary and talks to it over reqwest (HTTP) + a hand-rolled SSE parser.

## Changes
- `crates/har-provider/Cargo.toml`: moved `reqwest.workspace` from `[dev-dependencies]` to `[dependencies]`; added `regex.workspace` to `[dependencies]`.
- `crates/har-provider/src/opencode/http_client.rs` (NEW): pure-Rust `OpenCodeClient` (reqwest) + `SseEvent`/`HttpClientError`. Methods: create_session, get_session, prompt_async, subscribe_events (SSE via `async_stream::try_stream!`, frame-split on `\n\n`), get_message, abort_session, dispose_instance. Directory URL-encoded via `url::form_urlencoded`.
- `crates/har-provider/src/opencode/runtime.rs` (REWRITE): `acquire_embedded_runtime` is now `async`, spawns `opencode serve` via `tokio::process::Command` (env_clear + PATH/HOME/OPENCODE_CONFIG_CONTENT/XDG_CONFIG_HOME), parses the listening line (regex hoisted to a `LazyLock` static), port-retry loop, tokio startup timeout. New `RuntimeError` enum. Global state moved to `TokioMutex<Option<EmbeddedRuntimeState>>` owning the child + tempdir (ref-counted). `release_embedded_runtime_for_url` / `reset_embedded_runtime` are async; `release` now `kill().await`s the owned child. `dispose_instance_for_directory` now takes an `OpenCodeClient`. `SdkNotBoundError` removed.
- `crates/har-provider/src/opencode/session.rs`: added `resolve_session_id` (HTTP) and `stream_opencode_session` (SSE event demux → `MessageChunk`s, terminal on `session.idle`, abort-on-cancel). Existing `resolve_session_id_logic` and all event helpers retained unchanged.
- `crates/har-provider/src/opencode/provider.rs` (REWRITE): `send_query` now wires the real flow — acquire runtime → build `OpenCodeClient` → materialize agents + dispose instance (after acquire) → resolve session → resume-fallback `System` warning → build prompt body → `stream_opencode_session`, with the existing classify/enrich retry loop preserved. `RuntimeError` matched to error subtypes (`aborted`, `opencode_binary_not_found`, port-conflict retry, `runtime_start_failed`). `_parsed_model` renamed to `parsed_model_validated` (now used). `reset_embedded_runtime_for_provider` is async.
- `crates/har-provider/src/opencode/mod.rs`: `pub mod http_client`; re-exports `RuntimeError`, `HttpClientError`, `OpenCodeClient`, `SseEvent`; module doc updated (no SDK seam).
- `crates/har-provider/tests/parity_cycle21_opencode_bind.rs` (NEW): 10 unit tests + 1 `#[ignore]`d live test.
- `crates/har-provider/tests/parity_cycle19_opencode.rs`: updated `area9` test (renamed to `area9_send_query_real_runtime_yields_binary_error`) — it asserted the removed `opencode_sdk_not_bound` subtype; now asserts an error result whose subtype is NOT the old seam.

## Engine API (parity contract)
- `runtime::acquire_embedded_runtime(aborted: bool) -> Result<AcquiredRuntime, RuntimeError>` (now async).
- `runtime::release_embedded_runtime_for_url(&str)` / `reset_embedded_runtime()` (now async).
- `runtime::dispose_instance_for_directory(&OpenCodeClient, &str)`.
- `http_client::OpenCodeClient` (new public HTTP/SSE surface) + `SseEvent` + `HttpClientError`.
- `session::resolve_session_id(&OpenCodeClient, Option<&str>) -> Result<ResolvedSession, String>`.
- `session::stream_opencode_session(&OpenCodeClient, &str, &Map, &Arc<dyn CancelToken>) -> Result<Vec<MessageChunk>, String>`.
- New `MessageChunk::System` emission path: resume requested but not honored.

## Tests added
- http_client: `client_url_construction`, `dir_param_encodes_slashes`, `sse_event_struct_fields`.
- runtime: `acquire_with_binary_returns_runtime` (#[ignore], binary-gated), `acquire_aborted_returns_aborted_error`, `reset_embedded_runtime_clears_state` (all async); kept all helper unit tests.
- parity_cycle21: `sse_event_struct_construction`, `runtime_error_*_display` (3), `port_bind_conflict_*` (3), `extract_port_from_url_works`, `opencode_client_construction`, `live_acquire_and_create_session` (#[ignore]).
- provider: `no_opencode_binary_yields_error_result`, `agent_materialization_runs_before_stream`, `session_cwd_uses_node_id_subdirectory` all binary-gated (#[ignore]); model/baseUrl/type/capabilities tests unchanged and still green without the binary.

## Build/test status
- `cargo build -p har-provider` — PASS (0 warnings).
- `cargo clippy -p har-provider --all-targets -- -D warnings` — PASS (no issues; regex-in-loop fixed via `LazyLock` static).
- `cargo fmt -p har-provider --check` — PASS.
- `cargo test -p har-provider` — PASS: 736 lib + all integration suites, 0 failed, 5 ignored (binary-gated live tests).
- NOTE: one transient flake observed on `pi::provider::tests::send_query_yields_warning_for_unknown_tools` (unrelated `pi` module, WebFetch-tool detection) under full-parallel run; passes in isolation and on re-run. Not caused by this change.

## Deviations
- The cycle-19 `area9` integration test asserted the now-removed `opencode_sdk_not_bound` seam AND asserted agent FS materialization happened "before the seam". Both are architecturally invalid post-PR-21: (1) the seam is gone, (2) materialize_agents now runs AFTER `acquire_embedded_runtime`, so with no binary on PATH (CI) it never runs. Updated the test to assert an error result with a non-seam subtype. The binary-gated `agent_materialization_runs_before_stream` provider test now owns the "materialize before stream" side-effect check.

## Handoff notes
- No-C trust boundary preserved: only added `reqwest` (already a workspace dep, rustls/ring per workspace config) and `regex` to `[dependencies]`; no SQLite/OpenSSL/aws-lc pulled in. Verify `cargo tree -p har-provider` shows no aws-lc-rs / native-tls.
- Fail-closed: `acquire_embedded_runtime(true)` returns `RuntimeError::Aborted`; ENOENT on spawn returns `SpawnFailed` (non-retryable) → `opencode_binary_not_found`; cancel during stream aborts the session and returns `"aborted"`. Unit-tested via `acquire_aborted_returns_aborted_error` + `runtime_error_*_display`.
- Live HTTP/SSE path (`live_acquire_and_create_session`, `acquire_with_binary_returns_runtime`) is `#[ignore]`d — requires the `opencode` binary on PATH; gate cannot exercise it in CI. The non-live unit coverage proves construction, error display, and seam removal.
- Child-process ownership: `release_embedded_runtime_for_url` explicitly `kill().await`s the owned child (tokio `Child` does not kill on drop) AND port-scan-kills as backup. Confirm no orphan `opencode` processes if running the live tests locally.
