# Parity Findings — ITERATE Cycle 7 — UNITS PA-01 / PA-06 / PA-07 (har-paths)

**Date:** 2026-06-13
**Verifier:** rust-port-parity-verifier (adversarial, differential, fail-closed)
**Source X:** `/home/drdave/Desktop/meta/Archon` `packages/paths/src/{archon-paths.ts, env-loader.ts, strip-cwd-env.ts, strip-cwd-env-boot.ts}` — run under **bun 1.3.14**, dotenv **17.3.1**, Archon v0.4.1.
**Rust port:** `crates/har-paths/src/{archon_paths.rs, env_loader.rs, strip_cwd_env.rs}` — dotenvy 0.15.7.
**Method:** Live differential, one fixture per OS process (env isolation, identical controlled env on both sides). Transient TS oracle `Archon/__parity_oracle_cycle7.ts` (now **DELETED**, Archon pristine) and Rust harness `crates/har-paths/examples/parity_oracle_cycle7.rs` emitted canonical JSON / captured before-after env snapshots / captured stderr; diffed with `jq -cS` (key-order-insensitive) and `diff` (byte-exact for stderr). The port's own 55 unit tests were NOT used as the oracle.

## Overall verdict: **PASS** (19/19 symbols resolved: 17 `- [x]`, 2 `- [≠]`)

One **real divergence found and fixed** mid-verification (warning-text indentation, below); re-verified to byte-identical. No remaining divergences.

---

## Divergence found & fixed (no-downgrade gate fired)

**PA-07 `stripCwdEnv` — nested-Claude-Code warning text dropped its 3-space indent.**

- **Input:** `CLAUDECODE=1`, `ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING` unset → stderr warning emitted before deletion.
- **TS (oracle):** continuation lines 2–5 begin with `   ` (3 spaces): `⚠  Detected CLAUDECODE=1 — …\n   If workflows…\n   Workaround…\n   Suppress…\n   Details: https://github.com/coleam00/Archon/issues/1067\n`
- **Rust (before fix):** continuation lines began with **zero** leading spaces (`If workflows…`, `Workaround…`, …). Hex-confirmed: TS line 2 = `20 20 20 49 66` (`   If`), Rust = `49 66` (`If`).
- **Root cause:** the Rust `\`-line-continuation in the string literal swallows the newline **and** the next source line's leading whitespace, eating the intended 3-space indent. The `⚠`/`—` glyphs and the issue URL were already correct.
- **Fix:** `crates/har-paths/src/strip_cwd_env.rs` — warning extracted to `pub const NESTED_CLAUDE_WARNING` written with explicit `\n   ` sequences (no `\`-continuations). Re-diff vs `bun`: **byte-identical**. Durable regression golden added: `strip_cwd_env::tests::nested_claude_warning_exact_bytes` (pins the exact bytes + per-line 3-space indent). Re-exported from `lib.rs`.

This was invisible to the port's existing tests because every strip test set the suppress flag and none asserted the warning content — a coverage gap now closed.

---

## Per-behavior results

### PA-01 `archon-paths.ts` (`get_archon_home`/`is_docker` + pure path builders)

| Behavior | Env input | TS = Rust | Verdict |
|---|---|---|---|
| isDocker: `WORKSPACE_PATH=='/workspace'` | `WORKSPACE_PATH=/workspace` | `/.archon`, isDocker=true | PASS |
| isDocker: `HOME=='/root' && WORKSPACE_PATH` truthy | `HOME=/root WORKSPACE_PATH=/some/path` | `/.archon` | PASS |
| isDocker: `ARCHON_DOCKER=='true'` | `ARCHON_DOCKER=true` | `/.archon` | PASS |
| isDocker false: `HOME=/root` no WS | `HOME=/root` | default `~/.archon` | PASS |
| isDocker false: `ARCHON_DOCKER=1` (not 'true') | `ARCHON_DOCKER=1` | default | PASS |
| ARCHON_HOME set | `ARCHON_HOME=/custom/archon` | `/custom/archon` | PASS |
| ARCHON_HOME tilde-expand | `ARCHON_HOME=~/.custom-archon` | `<home>/.custom-archon` | PASS |
| **`"undefined"` literal guard** | `ARCHON_HOME=undefined` | both `{ok:false}` w/ byte-identical error message | **PASS** |
| default | (none) | `<home>/.archon` | PASS |
| expandTilde: `~`, `~/x`, `~\x`, `~foo`, `/abs`, `rel`, `~/` | — | all 7 identical | PASS |
| **parseOwnerRepo** accept/reject (18 cases incl. `.hidden/repo` ACCEPT, `öwner` REJECT, `+`/space REJECT, `.`/`..` segs REJECT, 3-part REJECT, trailing-slash REJECT) | — | all 18 identical (return shape `{owner,repo}` / `null`) | **PASS** |
| path builders: workspaces/worktrees/config/home-{workflows,commands,scripts}/legacy/archon-env/repo-archon-env/project-{root,source,worktrees,artifacts,logs}/run-{artifacts,log}/web-dist | `ARCHON_HOME=/tmp/test-archon` | all 17 paths identical | PASS |
| getCommandFolderSearchPaths (none/dup1/dup2/empty/custom) + getWorkflowFolderSearchPaths | — | all identical | PASS |
| resolveProjectRootFromCwd (under-deep/under-exact/one-seg/not-under) | `ARCHON_HOME=/tmp/test-archon` | all identical | PASS |
| getDefaultCommandsPath / getDefaultWorkflowsPath | — | **QUALIFIED `- [≠]`** — TS `import.meta.dir` (built-binary dir) has no differential analog vs `bun`-from-source; Rust seam = `ARCHON_APP_BASE`/exe-path; composition (`<base>/.archon/{commands,workflows}/defaults`) matches source `join(...)` exactly | QUALIFIED |

### PA-06 `env-loader.ts` (`loadArchonEnv`, `isVerboseBoot`)

| Behavior | Setup | TS = Rust | Verdict |
|---|---|---|---|
| **override precedence: project beats user on overlapping key** | `~/.archon/.env: SHARED=user_value, USER_ONLY` + `<cwd>/.archon/.env: SHARED=project_value, PROJECT_ONLY` | `SHARED=project_value`; all 3 keys set | **PASS** |
| **override:true clobbers PRE-SET system env (user scope)** | preset `PRESET_KEY=preset_system_value`, user `.env: PRESET_KEY=from_user_env_file` | both → `from_user_env_file` (file wins, matching `config({override:true})`) | **PASS** |
| **override:true clobbers PRE-SET system env (project scope)** | preset `PROJ_PRESET=...`, project `.env` overrides | both → file value | **PASS** |
| no env files present | neither exists | no-op; preset survivors kept | PASS |
| only user scope present | user `.env` only | loaded; repo skipped | PASS |
| quoted value w/ spaces | `QUOTED="hello world"` | both → `hello world` | PASS |
| verbose-boot stderr (counts + path + "repo scope, overrides user scope") | `ARCHON_VERBOSE_BOOT=1` | byte-identical | PASS |
| displayPath `~` shortening (file under HOME) | home-relative env path | byte-identical `~/...` rendering | PASS |
| isVerboseBoot (`=1`/`LOG_LEVEL=debug`/`trace`/`info`) | — | matches (covered via verbose-stderr gating) | PASS |

> Override-precedence — the porter-flagged likely-divergence — is **CONFIRMED at parity**: Rust `dotenvy::from_path_iter` + unconditional `set_var` reproduces TS dotenv `config({override:true})` exactly (file value wins over already-set process env; later file wins over earlier).

### PA-07 `strip-cwd-env.ts` + `strip-cwd-env-boot.ts`

| Behavior | Setup | TS = Rust | Verdict |
|---|---|---|---|
| strip CWD `.env` keys | preset `CWD_LEAK_A/B`, `<cwd>/.env` lists them | both removed; SURVIVOR kept | PASS |
| strip across all 4 BUN_AUTO files | `.env`/`.env.local`/`.env.development`/`.env.production` | all 4 keys removed; stripped-files stderr byte-identical | PASS |
| CLAUDECODE + `CLAUDE_CODE_*` prefix removal, **3 auth vars preserved** | `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SOMETHING`, + OAUTH_TOKEN/USE_BEDROCK/USE_VERTEX | non-auth removed; exactly the 3 auth vars survive | PASS |
| NODE_OPTIONS + VSCODE_INSPECTOR_OPTIONS unconditional removal | both preset | both removed; OTHER kept | PASS |
| CLAUDECODE present but `=0` (not '1') still deleted, no warning | `CLAUDECODE=0` | deleted, no stderr | PASS |
| clean env — no-op | nothing set | no-op | PASS |
| strip by-key regardless of preset≠file value | `.env: MISMATCH=file_val`, preset `=different` | removed by key | PASS |
| **CLAUDECODE=1 warning text (exact Unicode/URLs/indent)** | `CLAUDECODE=1`, no suppress | **byte-identical after fix** (was the divergence) | **PASS** |
| suppress flag silences warning | `ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING=1` | both emit 0 stderr bytes | PASS |
| BUN_AUTO_LOADED_ENV_FILES / CLAUDE_CODE_AUTH_VARS membership | — | exact list/set match | PASS |
| strip_cwd_env_boot (= strip_cwd_env(cwd())) | — | wrapper of verified strip_cwd_env | PASS |

---

## Symbols flipped to `- [x]` (17) in `.handoff/loop/symbol-map.md`

PA-01: getArchonHome, isDocker, expandTilde, getArchonWorkspacesPath, getRunArtifactsPath, getProjectLogsPath, getWorkflowFolderSearchPaths, getCommandFolderSearchPaths, getHomeCommandsPath, getHomeWorkflowsPath, parseOwnerRepo (11).
PA-06: loadArchonEnv, isVerboseBoot (2).
PA-07: stripCwdEnv, BUN_AUTO_LOADED_ENV_FILES, CLAUDE_CODE_AUTH_VARS, stripCwdEnv(boot) (4).

## Symbols `- [≠]` (2, intentional idiom divergence, recorded)

PA-01: getDefaultCommandsPath, getDefaultWorkflowsPath — TS `import.meta.dir` → Rust `ARCHON_APP_BASE`/exe-path seam; path composition verified identical.

## Build/health gate (precondition)

- `cargo test -p har-paths` → **56 passed** (was 55; +1 the new byte-exact warning golden).
- `cargo clippy -p har-paths --all-targets` → **clean**.

## Durable fixtures committed under the crate

- `crates/har-paths/examples/parity_oracle_cycle7.rs` — differential harness (re-runnable against `bun`).
- `crates/har-paths/src/strip_cwd_env.rs::tests::nested_claude_warning_exact_bytes` + `pub const NESTED_CLAUDE_WARNING` — regression golden for the fixed divergence.

## Source hygiene

Transient TS oracle `Archon/__parity_oracle_cycle7.ts` DELETED; `git status` on Archon = clean.

**Cycle-7 verdict: PASS — units PA-01, PA-06, PA-07 are at behavioral parity (no downgrade). 1 divergence caught & fixed.**
