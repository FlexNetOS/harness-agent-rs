# Parity Cycle 21 — OpenCode SDK-seam → live-binding gate

**Date:** 2026-06-21
**Gate:** rust-port-parity-verifier (differential, against a LIVE `opencode serve` server)
**Unit:** PR-11 `community/opencode/*` — the `opencode_sdk_not_bound` seam replaced by a
pure-Rust server-spawn + native HTTP/SSE client (`runtime.rs` + `http_client.rs` + `session.rs`).

**Verdict: PASS** (after the gate corrected two forced single-site divergences the source
left zero design choice on — see D1/D2). Binding works E2E vs the live server; the
`opencode_sdk_not_bound` seam is GONE from production code; opencode `send_query` is really bound.

---

## 0. THE #1 JOB — auth contradiction RESOLVED EMPIRICALLY: **the porter is correct**

The porter (NO auth) and prior research (Basic auth required) disagreed. Resolved by RUNNING
the server exactly as the binding spawns it, then probing with/without auth.

Spawn (binding's exact recipe: `env -i` + PATH/HOME + `OPENCODE_CONFIG_CONTENT` = the config
`build_embedded_server_config` produces INCLUDING `server.password`, `XDG_CONFIG_HOME`=tempdir):

```
stdout: "Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured."
        "opencode server listening on http://127.0.0.1:23530"

PROBE 1  GET  /session?directory=   NO  auth header  ->  HTTP 200  body []
PROBE 2  GET  /session?directory=   Basic opencode:pw ->  HTTP 200  body []
PROBE 3  POST /session?directory=   NO  auth header  ->  HTTP 200  {"id":"ses_...",...}
PROBE 4  POST /session?directory=   Basic opencode:pw ->  HTTP 200  {"id":"ses_...",...}
```

**Decisive finding:** the server enforces Basic auth ONLY when the **`OPENCODE_SERVER_PASSWORD`
env var** is set — NOT when `server.password` is in the config. The config password is INERT for
auth enforcement. Confirmed by the inverse probe:

```
(with env OPENCODE_SERVER_PASSWORD=… set, no "unsecured" warning)
PROBE A  GET /session  NO auth                 ->  HTTP 401
PROBE B  GET /session  Basic opencode:<pw>     ->  HTTP 200
```

This reproduces the research's 401 — and proves it came from `OPENCODE_SERVER_PASSWORD` being
present in that probe's environment, NOT from the config. The binding's `env_clear()`
(runtime.rs:267) strips that var, so the binding's own spawn produces an UNSECURED server that
accepts the binding's own unauthenticated requests. **Self-consistent.**

**Code-side corroboration** (SDK @opencode-ai/sdk@1.15.11):
- `createOpencode(opts)` → `createOpencodeServer(opts)` spawns the binary with
  `OPENCODE_CONFIG_CONTENT` (incl. `server.password`), then
  `createOpencodeClient({ baseUrl: server.url })` — **no `auth`/`security` field**.
- `createOpencodeClient` (dist/client.js) sets no Authorization header; the only auth machinery
  (`getAuthToken`, auth.gen.js) fires solely when a `security` array + `auth` callback is wired,
  which `createOpencode` never does. All other `Authorization` refs are OAuth-for-model-providers.

=> The SDK client created by `createOpencode` sends NO Authorization header — EXACTLY matching
the Rust binding (no header, `env_clear`). **Porter PASS. Research's 401 explained, not a defect.**

---

## 1. Spawn / lifecycle (live) — PASS

`acquire_embedded_runtime` spawns `opencode serve --hostname=127.0.0.1 --port=N`, parses
`opencode server listening on <url>` via `on\s+(https?://\S+)`, honors the 5000 ms timeout +
3× port retry. Verified live: `live_spawn_create_get_dispose` acquires a runtime with
`server_url == http://127.0.0.1:<port>`, then exercises create/get/dispose/abort; release kills
the child. PASS.

## 2. Endpoints (live HTTP method + status, diffed vs client.gen.js + live server) — PASS

| Binding call            | Method | URL (`?directory=<url-enc cwd>`)        | Live status | Match |
|-------------------------|--------|-----------------------------------------|-------------|-------|
| create_session          | POST   | /session                                | 200 + {id}  | ✓ |
| get_session             | GET    | /session/{id}                           | 200         | ✓ |
| prompt_async            | POST   | /session/{id}/prompt_async              | **204**     | ✓ (unit_or_error Ok) |
| subscribe_events        | GET    | /event                                  | SSE stream  | ✓ |
| get_message (bad id)    | GET    | /session/{id}/message/{messageID}       | 404 → Err   | ✓ |
| abort_session           | POST   | /session/{id}/abort                     | 200         | ✓ |
| dispose_instance        | POST   | /instance/dispose                       | 200         | ✓ |

`?directory=<url-encoded cwd>` present on every call (http_client.rs `dir_param` via
`form_urlencoded::byte_serialize`). SDK `event.subscribe` → `/event` (NOT `/global/event`) — binding matches.

## 3. SSE decode (live frames) — PASS

Real `/event` frames fed through the binding decoder:
```
data: {"id":"evt_…","type":"server.connected","properties":{}}\n\n
data: {"id":"evt_…","type":"session.error","properties":{"sessionID":"ses_…","error":{"name":"UnknownError","data":{"message":"Model not found: nonexistent/nope."}}}}\n\n
data: {"id":"evt_…","type":"session.idle","properties":{"sessionID":"ses_…"}}\n\n
```
`data: <json>\n\n` framing → `SseEvent{type, properties}` → existing parsers. PASS.
prompt_async with an INVALID model: live server emits `session.error` (not a hang); binding
surfaces it as `Err`. Verified by `live_invalid_model_surfaces_real_error`.

## 4. Abort path (live) — PASS

`CancelToken` cancelled → loop calls POST /abort → returns `Err("aborted")`.
`live_abort_path` PASS.

## 5. No-downgrade — PASS

`opencode_sdk_not_bound` is GONE from production code (grep: zero hits in
`crates/har-provider/src/opencode/**` except a historical doc comment). Streaming, tool chunks,
result tokens/cost, error, abort, resume (`resolve_session_id` get→create), and
agent-materialization-before-stream are all real, native code paths.

---

## DIVERGENCES FOUND (live differential) — both CORRECTED by the gate (forced single-site)

### D1 — session.error dropped the structured error message (DOWNGRADE) — FIXED
The live `session.error` payload is an OBJECT `{name, data:{message}}`, but
`stream_opencode_session` read `properties.error.as_str()` → `None` → always surfaced
`"session.error: Unknown session error"`, **losing the real model error**. TS (session.ts:241-242)
runs `errorMessage(isRecord(error) ? error : properties)` → returns the nested `data.message`
(errors.ts:34). The binding ALREADY had the faithful `errors::error_message_from_value` (errors.rs:80)
but session.rs wasn't calling it. Live test `live_invalid_model_surfaces_real_error` failed:
got `"Unknown session error"`, expected `"Model not found: nonexistent/nope."`.
**Fix (session.rs):** use `error_message_from_value(isRecord ? error : properties)`. Now surfaces
the real message. (The source leaves zero design choice; corrected in-place per the cycle-19 precedent.)

### D2 — session.idle/session.error not filtered by sessionID (DOWNGRADE) — FIXED
The embedded server is SHARED across directories/sessions; TS skips idle/error events whose
`properties.sessionID !== sessionId` (session.ts:237-239, 248). The binding acted on the FIRST
idle/error regardless of session → a concurrent run in another directory could prematurely
terminate/error the wrong stream. (Note: `process_message_updated`/`process_message_part_updated`
DID filter — only the terminal idle/error branches missed it.)
**Fix (session.rs):** added the `event.properties.sessionID != session_id → continue` guard to
both the `session.error` and `session.idle` branches.

---

## Evidence / harness

- New durable live test: `crates/har-provider/tests/parity_cycle21_opencode_live.rs` —
  discovers the binary (PATH or `/tmp/opencode-bin-*/opencode`, spliced onto PATH), runs the
  binding's real code path E2E; 3 legs (spawn+create+get+dispose, invalid-model session.error,
  abort) all PASS; SKIP-clean if no binary.
- Existing `parity_cycle21_opencode_bind.rs::live_acquire_and_create_session` also runs green
  with the binary on PATH.
- `cargo clippy -p har-provider --all-targets -- -D warnings`: **clean (exit 0)**.
- `cargo test -p har-provider`: **965 passed / 0 failed / 7 ignored** (ignored = Windows kill path +
  older superseded `#[ignore]` live legs; live coverage now provided by the new file).
- Archon kept pristine; temp configs / XDG dirs / the PATH symlink all cleaned up; no writes to Archon.
