# Parity verdict — WF-09 sub-cycle 4b (WF-18 script-discovery + execute_script_node)

**Verifier:** rust-port-parity-verifier (no-downgrade gate) · **Date:** 2026-06-25
**Method:** differential — oracle built from LIVE source (bun 1.3.14, uv 0.11.18, node 22) vs Rust port.
**Source:** `meta/Archon/packages/workflows/src/{script-discovery.ts, dag-executor.ts:1683-1945}`
**Target:** `crates/har-dag-executor/src/{script_discovery.rs, dag_executor.rs}`

> **[RE-VERIFY 2026-06-25 — see verdict block at bottom] → PASS.** Both divergences fixed and
> re-confirmed byte-identical against live source. The FAIL below is the original (pre-fix) record.

## Verdict: FAIL (unit stays `- [~]`) — content + executor parity hold; **two divergences in error/order contract**

Bulk parity (content, precedence, depth cap, extension map, dup string, argv matrix, env overlay,
stdout strip, not-found string, events) is **clean**. Two divergences block the flip — one observable,
one latent.

## Durable artifacts
- Oracle (Rust): `crates/har-dag-executor/examples/wf18_oracle.rs` — `cargo run -p har-dag-executor --example wf18_oracle -- $ROOT`
- Oracle (TS):   `crates/har-dag-executor/tests/fixtures/wf09_4b_script_discovery_oracle.ts`
- Goldens:       `crates/har-dag-executor/tests/golden/wf09_4b_discovery_{ts,rs}.json`
- Fixture builder: scratchpad `wf18/build_fixtures.sh` (home+repo+dup+empty+nonexistent+unreadable scopes)

---

## A · WF-18 script-discovery (live differential)

Fixtures: home scope (`shared.ts`,`home_only.py`); repo `.archon/scripts` (`shared.py`,`foo.ts`,
`quux.js`,`pyroot.py`,`m1.ts`/`m2.js`/`m3.py`, `triage/bar.ts` depth-1, `a/b/baz.ts` depth-2,
`ignored.sh`/`ignored.rb`/`README.md`); plus dup/empty/nonexistent/unreadable dirs.

| Probe | Oracle (TS) | Rust | Verdict |
|---|---|---|---|
| `discover_scripts_for_cwd` repo>home precedence | `shared`→repo `.py`/**uv**; 9 entries | identical keys/paths/runtimes | PASS |
| depth cap (MAX=1) | `bar` (d1) found, `baz` (d2) absent | identical | PASS |
| extension→runtime | `.ts`/`.js`→bun, `.py`→uv; `.sh`/`.rb`/`.md` skipped | identical | PASS |
| `get_runtime_for_extension` (via discovery) | as above | identical | PASS |
| duplicate name (same scope) | `Duplicate script name "foo": found "…/foo.py" and "…/foo.ts". Script names must be unique across extensions.` | byte-identical | PASS* |
| empty / nonexistent dir | empty map (no error) | empty map | PASS |
| `get_default_scripts` | size 0 | size 0 | PASS |
| **discovery error (unreadable dir)** | `Directory read error: EACCES: permission denied, scandir '<path>' (EACCES)` | `Directory read error: Permission denied (os error 13) (13)` | **FAIL — D-ERR** |
| **map iteration order** | insertion = readdir order (`m3,m2,m1,bar,pyroot,quux,foo,shared`) | HashMap random (`shared,m1,foo,bar,m3,m2,pyroot,quux`) | **DIVERGE — D-ORDER** |

\* dup string matched byte-for-byte here, but the `found "X" and "Y"` ordering is **readdir-order
dependent on both sides** (matched by FS coincidence); not a guaranteed-stable contract across
platforms. Note for robustness, not a hard fail.

### D-ERR (observable FAIL) — `ScriptDiscoveryError::DirReadError` string
- **Input:** scripts dir with mode `000` (EACCES on readdir).
- **Expected (TS):** `Directory read error: EACCES: permission denied, scandir '<path>' (EACCES)`
  — Node errno message body + **symbolic** code `EACCES`.
- **Actual (Rust):** `Directory read error: Permission denied (os error 13) (13)` — `io::Error`
  Display body + **numeric** code `13`.
- **Where it surfaces:** `execute_script_node` named-script discovery branch wraps this verbatim:
  `Script node '<id>': failed to discover scripts — <DirReadError>` → the `node_failed` error string
  (and the `safeSendMessage` + JSONL log) a user sees **differs**. (`dag_executor.rs:2777`.)
- **Fix (route to porter):** in `script_discovery.rs:117-125` map `raw_os_error()` to the symbolic
  code (13→`EACCES`, 2→`ENOENT`, …) and format the message as Node does
  (`<CODE>: <strerror>, scandir '<path>'`). Or owner-approved `- [≠]` "OS error string format is
  runtime-specific."

### D-ORDER (latent downgrade) — `discover_scripts` returns `HashMap` not insertion-ordered
- TS returns a `Map` (insertion = readdir order). Rust `HashMap` randomizes (per-process `RandomState`).
- **Not observable in 4b's executor** (`execute_script_node` only does `scripts.get(name)` — single
  lookup). So script-node behavior is unaffected.
- **IS observable once a real consumer lands:** `validator.ts:723-726 listScripts` does
  `[...scripts.values()].map(...)` — returns an ordered array. Ported as-is over a Rust `HashMap`,
  the listing order becomes non-reproducible (TS is FS-stable/process-stable). Latent WF-18
  return-type contract downgrade.
- **Fix (route to porter):** use `indexmap::IndexMap` for the discovery map to preserve
  insertion (readdir) order. Low-risk.

---

## B · execute_script_node (dag-executor.ts:1683-1945 → dag_executor.rs:2605-3147)

`WorkflowDeps` has a private `get_agent_provider` fn-pointer and no public/test constructor, so the
full async fn is **not invocable from a harness** (same constraint sub-cycle 4a accepted). Verified
via (1) real argv groundings on live bun/uv, (2) line-by-line ladder read vs TS, (3) reuse of
4a-verified helpers.

| Contract behavior | Check | Verdict |
|---|---|---|
| inline-bun argv `['--no-env-file','-e',script]` | ran live → `inline-bun-hello\nsecond` + stderr | PASS |
| inline-uv argv `['run',(--with d)*,'python','-c',script]` | ran live (+`--with cowsay` → `Installed 1 package`/`with-dep-ok`) | PASS |
| named-bun argv `['--no-env-file','run',path]` | ran live → `named-bun-out`+stderr | PASS |
| named-uv argv `['run',(--with d)*,path]` (uses `scriptDef.runtime`) | ran live → `named-uv-out` | PASS |
| narrow env overlay (only ARTIFACTS_DIR/LOG_DIR/BASE_BRANCH + env_vars; NO USER_MESSAGE/LOOP_*) | code 2721-2730 == TS 1739-1745; grounded (`ARTIFACTS_DIR` set, `USER_MESSAGE` unset) | PASS |
| stdout single-trailing-`\n` strip (`strip_suffix('\n')`, not `trim_end`) | `hello\n\n`→`hello\n` (4a tests) | PASS |
| stderr surface ``Script node '<id>' stderr:\n```\n{trim}\n``` `` | code 2920-2936 == TS 1859-1867 | PASS |
| not-found string `… named script '<name>' not found in .archon/scripts/ or ~/.archon/scripts/` | code 2831 == TS 1810 | PASS |
| discovery error in OWN try/catch (distinct from exec EACCES) | structurally present 2774-2824 == TS 1774-1806 | PASS (string carries D-ERR) |
| ENOENT `'<cmd>'` single-quote format | code 2036 == TS 1908 | PASS |
| EACCES `permission denied (check cwd permissions)` | code 2038-2043 == TS 1909-1910 | PASS |
| timeout `… timed out after <ms>ms` | code 2985 == TS 1906 | PASS |
| other → `format_subprocess_failure().user_message` | reused 4a helper | PASS |
| node_started event carries `runtime` field | code 2641 `{"type":"script","runtime":runtime_str}` == TS 1709 | PASS |
| returns `Failed{output:''}` / `Completed{output}` | all arms | PASS |
| dispatch arm `DagNode::Script` wired (not Skipped) | `dag_executor.rs:3623` before `_=>Skipped` | PASS |

### 4a reused helpers in script-node context (spot-check: reused, not duplicated)
`run_subprocess`, `log_node_start/complete/error`, `format_subprocess_failure`, `safe_send_message`,
`is_inline_script`, `substitute_workflow_variables`, `substitute_node_output_refs` — **single
definitions**, called from `execute_script_node`. argv groundings exercise `run_subprocess` behavior
identically. `is_inline_script` regex `[;(){}&|<>$\`"' ]`+newline already cross-verified (cycle5).
**Confirmed: 4a helpers behave correctly in the script-node context.**

---

## What blocks the ledger flip
1. **D-ERR** — `DirReadError` string mismatch (symbolic-vs-numeric code, Node-vs-io::Error body),
   observable in the `execute_script_node` discovery-error `node_failed` string. → porter fix or
   owner `- [≠]`.
2. **D-ORDER** — `discover_scripts` returns unordered `HashMap`; drops the TS `Map` insertion-order
   guarantee a future `listScripts` consumer relies on. → porter switch to `IndexMap`.

Everything else (content parity across all scopes, precedence, depth cap, extension map, the full
bun/uv argv matrix incl. `--with` deps, narrow env overlay, stdout strip, not-found, events, dispatch
wiring) is **PASS**. Fix the two divergences and re-run `wf18_oracle` for a clean flip.

---

# RE-VERIFY verdict — 2026-06-25 (post-porter-fix)

**Method:** re-ran the FULL battery myself — re-captured the live-TS oracle (`bun 1.3.14`/`uv 0.11.18`)
and re-ran `wf18_oracle` over rebuilt fixtures (now incl. an ENOTDIR file-as-dir probe). Did NOT trust
the porter's description. Goldens refreshed: `tests/golden/wf09_4b_discovery_{ts,rs}.json`.

## Verdict: PASS — every WF-18 probe + execute_script_node contract branch matches; both divergences fixed.

### D-ERR (`node_readdir_error`) — FIXED, byte-identical vs live TS
| errno | Oracle (TS) | Rust | Verdict |
|---|---|---|---|
| EACCES (mode-000 dir) | `Directory read error: EACCES: permission denied, scandir '<path>' (EACCES)` | identical | PASS |
| ENOTDIR (file-as-dir) | `Directory read error: ENOTDIR: not a directory, scandir '<path>' (ENOTDIR)` | identical | PASS |

ENOENT is **unreachable** as a `DirReadError` (readdir-ENOENT → early `Ok(empty)`, script-discovery.ts:73);
the porter's `2→ENOENT` mapping is dead-but-harmless for-completeness, so the second live errno probed is
ENOTDIR. Surfaces correctly through `execute_script_node` named-script discovery-error branch by
composition: unchanged wrapper `dag_executor.rs:2777` (`failed to discover scripts — {DirReadError}`) +
now-byte-identical inner Display.

### D-ORDER (`ScriptMap = IndexMap`) — FIXED, insertion order byte-identical vs live TS Map
| scope | order TS | order RS | Verdict |
|---|---|---|---|
| repo_scope | `m3,m2,m1,bar,pyroot,quux,foo,shared` | identical | PASS |
| home_scope | `home_only,shared` | identical | PASS |
| for_cwd (merge) | `home_only,shared,m3,m2,m1,bar,pyroot,quux,foo` | identical | PASS |

Merge semantics confirmed: `shared` stays at home's index 1 while resolving to the **repo** entry
(runtime=uv) — `IndexMap::insert` keeps existing index = TS `Map.set`. (Exact cross-runtime order match
holds here because both read the same ext4 dir; the verified contract is "insertion order preserved",
which both now satisfy.)

### No regression (D-ORDER touched shared return type) — re-confirmed
precedence (shared→uv repo-wins), depth cap (d1 found/d2 absent), extension map, duplicate string,
empty/nonexistent, `get_default_scripts`=0 → all **PASS** (content diff clean across all 8 scopes).
`cargo build --examples` clean; 16/16 `script_discovery` lib tests pass; named `.get()` lookup over
IndexMap resolves correctly (shared→uv, foo→bun).

### execute_script_node argv matrix — unchanged by the fix, re-confirmed PASS
argv shapes are built from runtime+path (independent of map type); all four (inline-bun/inline-uv/
named-bun/named-uv + `--with` deps) grounded live in the original pass; named-script `.get()` over the
new `IndexMap` still resolves (verified above). stdout strip / stderr surface / not-found / ENOENT /
EACCES / timeout / events / dispatch wiring — all PASS (unchanged).

## Symbol + branch coverage (for ledger flip)
**All 4b symbols verified:** WF-18 — `ScriptDefinition`, `ScriptRuntime`, `get_runtime_for_extension`,
`MAX_SCRIPT_DISCOVERY_DEPTH`, `scan_script_dir`, `discover_scripts`, `discover_scripts_for_cwd`,
`get_default_scripts`, `normalize_sep`, `ScriptDiscoveryError` (DirReadError + DuplicateScriptName +
PathError), `node_readdir_error`, `ScriptMap`. Executor — `execute_script_node` (all error-ladder
branches + inline/named × bun/uv argv + discovery-error own try/catch + events) and the
`DagNode::Script` dispatch arm (wired, not Skipped). 4a reused helpers behave correctly in script
context. **Nothing left on a placeholder. WF-18 + Script dispatch arm may flip to `- [x]`.**
