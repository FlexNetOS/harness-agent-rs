# Parity findings — ITERATE cycle 12 (Claude provider sub-units)

Source X: `meta/Archon` v0.4.1 `packages/providers/src/claude/{binary-resolver,config,native-tools}.ts`
Target:   `harness-agent-rs` `crates/har-provider/src/claude/{binary_resolver,config,native_tools}.rs`
Method:   DIFFERENTIAL — live `bun` 1.3.14 oracle ⇄ Rust, NOT the port's own tests.
Oracle:   transient `__parity_oracle_c12.ts` + forced-miss capture scripts (created in Archon, RUN, then DELETED; Archon left pristine — `git status` clean).
Durable:  `crates/har-provider/tests/parity_cycle12.rs` (43 differential cases) + `tests/fixtures/claude_install_instructions.golden.txt` (byte-exact bun golden).

---

## 2026-06-14 — Verdict block

### PR-04 binary-resolver — PASS (after porter-fix)

Differential set (11 on-disk fixtures, real `path_kind` via `std::fs::metadata`, `#[serial]` env):

| Behavior | Input | TS (bun) | Rust | Result |
|---|---|---|---|---|
| env file wins (binary mode) | `CLAUDE_BIN_PATH`=real file | returns it | returns it | PASS |
| env file wins (dev mode) | same, is_binary_mode=false | returns it | returns it | PASS |
| env > config > autodetect | env+config both real | env file | env file | PASS |
| env missing → file error | `/nonexistent/cli.js` | exact "…but the file does not exist.\n…" | identical | PASS |
| empty env = unset, dev → None | `CLAUDE_BIN_PATH=""`, dev | undefined | `None` | PASS |
| dev + no env, config ignored → None | config set, dev | undefined | `None` | PASS |
| config file (binary mode) | real file | returns it | returns it | PASS |
| config missing → file error | `/nonexistent/cli.js` | exact `assistants.claude.claudeBinaryPath…` | identical | PASS |
| env dir → expand inner | dir containing `claude` | inner path | inner path | PASS |
| config dir → expand inner | dir containing `claude` | inner path | inner path | PASS |
| config dir missing inner → dir error | empty dir | exact "…which is a directory, but it does not contain claude.\n…" | identical | PASS |
| env dir missing inner → dir error | empty dir | exact CLAUDE_BIN_PATH dir error | identical | PASS |
| binary mode, all miss → install instructions | env unset, no config, HOME w/o `.local/bin/claude` | exact INSTALL_INSTRUCTIONS | **see FIX below** | PASS after fix |

Precedence + dev-vs-binary gating confirmed: `is_binary_mode` param faithfully replaces `BUNDLED_IS_BINARY`
(env honored in BOTH modes; config + autodetect + Err only in binary mode; dev+no-env → `None`).

**FAIL → FIXED (no-downgrade gate caught a real port divergence):**
`INSTALL_INSTRUCTIONS` constant diverged from source on TWO axes:
1. **Lost indentation** — the Rust constant used `"\`-line-continuation, which strips the leading
   whitespace of each continued line. Every indented TS line (`  macOS…`, `    curl…`, `    assistants:`,
   `      claude:`, `        claudeBinaryPath:`) came out FLUSH-LEFT. User-facing error-text downgrade.
2. **Windows backslash over-escaping** — Rust emitted `$env:USERPROFILE\\.local\\bin\\claude.exe`
   (double backslashes at runtime); TS source resolves to SINGLE backslashes `\.local\bin\claude.exe`.

Both were corrected in `binary_resolver.rs` (explicit `\n` segments with `\u{20}` space escapes to
survive line-continuation; single Windows backslashes). Re-verified byte-exact vs the bun golden;
the install-instructions differential is now exercised deterministically (HOME override forces the
autodetect-miss branch regardless of host — the host's real `~/.local/bin/claude` previously masked it).

PR-04: 6/6 symbols `- [x]`.

### PR-05 config — PASS

`parse_claude_config` — 16 differential cases vs `parseClaudeConfig` (bun), all 0-divergence:

| Case | Input | TS == Rust |
|---|---|---|
| empty | `{}` | `{}` |
| model string | `{model:"…"}` | passthrough |
| model non-string | `{model:42}` | dropped |
| settingSources both | `["project","user"]` | both |
| project-only / user-only | … | as-is |
| invalid-only | `["invalid","nope"]` | omitted (empty post-filter) |
| mixed valid/invalid | `["project","invalid","user"]` | `["project","user"]` |
| empty array | `[]` | omitted |
| non-array | `"project"` | dropped |
| **duplicates** | `["user","user","project"]` | **`["user","user","project"]` — NO dedup** (Rust `Vec`, no dedup — matches) |
| non-string members | `["project",5,true,"user"]` | `["project","user"]` |
| claudeBinaryPath string / non-string | … | passthrough / dropped |
| all three | … | all three |
| extra keys | `{model,unknownFutureProp,anotherProp}` | only `model` (unknown keys NOT forwarded; `extra` bag empty) |

PR-05 parseClaudeConfig: `- [x]`. (CLAUDE_CAPABILITIES already `- [x]` from PR-02/c11.)

### PR-06 native-tools — PASS (with one SCOPED `- [≠]`)

`validate_and_convert_schema` (ports `jsonSchemaToZodShape`) — 15 differential cases vs the LIVE Zod
shape (real `buildArchonMcpServer` → SDK `_registeredTools[…].inputSchema`, introspected to
`{name,kind,required,description}`), all 0-divergence:

| Case | TS == Rust |
|---|---|
| valid full (enum+string+bool, descriptions, required) | identical field shapes |
| enum without `type` | string_enum (enum checked before type) |
| **enum with non-string members** `["x",5,"y",true]` | filtered → `["x","y"]` (both filter_map/`isString`) |
| enum all non-string `[1,2,3]` | exact "enum for 'a' must be non-empty strings" |
| empty enum `[]` | exact "…must be non-empty strings" |
| no `required` array | all optional |
| **required with non-string members** `["x",7,null]` | filtered → only `x` required |
| empty properties `{}` | zero fields |
| unsupported `number` / `object` | exact "unsupported type for '…' (only string / string-enum / boolean)" |
| prop with neither type nor enum | exact "unsupported type…" |
| non-object schema (`type:string`) | exact "native tool inputSchema must be an object schema with `properties`" |
| missing `properties` / `properties:null` | exact "…must be an object schema with `properties`" |
| **enum precedence over type** (`type:boolean`+`enum`) | string_enum wins, description forwarded |

Server descriptor metadata verified vs live SDK object:
`name="archon"` (== `srv.name`), `version="1.0.0"` (== `server._serverInfo.version`),
`always_load=true` (== `_meta["anthropic/alwaysLoad"]`), `ARCHON_TOOL_SERVER="archon"`. 0-divergence.

- `ARCHON_TOOL_SERVER` → `- [x]`
- `jsonSchemaToZodShape`/`validate_and_convert_schema` → `- [x]`
- `buildArchonMcpServer`/`build_archon_mcp_server` → **`- [≠]` (SCOPED, CORRECT)**:
  source `createSdkMcpServer` constructs a LIVE in-process SDK MCP server (handler closures);
  Rust returns a serializable `McpServerDescriptor` (CLI-delegation model, ADR-0001 — no in-process
  SDK object). VERIFIED that the **deterministic conversion logic is faithful** (schema→fields +
  descriptor metadata, 0-divergence). The ONLY deferred piece is runtime server-process construction,
  routed to the PR-03 NEEDS-HUMAN (R8 sidecar). **No conversion behavior lost.** The `- [≠]` is
  correctly scoped — it does NOT fail PR-06.

PR-06: 3/3 symbols accounted (2 `- [x]`, 1 scoped `- [≠]`).

---

## Cycle-12 rollup

- **PR-04 binary-resolver: PASS** — 6/6 `- [x]` (after porter-fix to INSTALL_INSTRUCTIONS).
- **PR-05 config: PASS** — parseClaudeConfig `- [x]` (CLAUDE_CAPABILITIES already `- [x]`).
- **PR-06 native-tools: PASS** — 2 `- [x]` + 1 SCOPED `- [≠]` (createSdkMcpServer → PR-03 R8).

Evidence: `cargo test -p har-provider` → 131 passed (incl. 43 cycle-12 differential).
`cargo clippy -p har-provider --all-targets` → clean. Archon source pristine (git clean).

**OVERALL CYCLE-12 VERDICT: PASS** — all three units ready to commit. The gate caught and corrected
one genuine no-downgrade violation (install-instructions error-text indentation + Windows-path
escaping) before commit; everything else matched the live bun oracle exactly.
