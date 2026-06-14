# Parity Verdict — Cycle 10 (har-isolation behavioral core: IS-02, IS-03, IS-04)

## 2026-06-14 — Differential parity (live bun ⇄ Rust)

**Source X:** `meta/Archon` `packages/isolation/src/providers/worktree.ts` (IS-02),
`resolver.ts` (IS-03), `factory.ts` (IS-04). Run via bun 1.3.14.
**Rust port:** `crates/har-isolation/src/{providers/worktree.rs, resolver.rs, factory.rs}`.
**Method:** real temp git repos + linked worktrees; identical inputs through the live TS
source and the Rust port; diffed return value, git state, and error behavior. The port's own
tests were NOT the oracle — the live TS source was.

**Durable fixtures committed under the crate:**
- `crates/har-isolation/tests/parity_cycle10_branchnaming.rs` (IS-02; 3 tests, all PASS).
- `crates/har-isolation/tests/parity_cycle10_resolver.rs` (IS-03; 8 scenarios over real git repos;
  4 PASS active, 4 marked `#[ignore]` — they assert the TS golden and FAIL until IS-03 is fixed;
  run with `cargo test -p har-isolation --test parity_cycle10_resolver -- --ignored` to reproduce).

**Baseline after fixtures:** `cargo test -p har-isolation` = 128 passed, 4 ignored;
`cargo clippy -p har-isolation --all-targets` = clean.

**Transient TS oracles** (`__parity_oracle.ts`, `__parity_resolver_oracle.ts`,
`__tmp_repo_owner/`) created under `Archon/packages/isolation/` and **DELETED** — Archon verified
pristine (`git status` clean).

---

## UNIT IS-02 — WorktreeProvider → **PASS**

Branch naming is the highest-risk surface (wrong scheme = wrong worktree). Verified byte-for-byte
against the live TS oracle for every workflow type, including shortHash and slugify edge cases:

| input | TS branch | Rust branch | verdict |
|---|---|---|---|
| issue `42` | `archon/issue-42` | same | PASS |
| issue `` (empty) | `archon/issue-` | same | PASS |
| issue `café-#9` | `archon/issue-café-#9` (NO slugify) | same | PASS |
| review `77` | `archon/review-77` | same | PASS |
| pr `5` same-repo, prBranch `feature/My-Branch` | `feature/My-Branch` (verbatim) | same | PASS |
| pr `5` fork | `archon/pr-5-review` | same | PASS |
| thread `C123.456` | `archon/thread-ef6545d2` | same | PASS |
| thread `` (empty) | `archon/thread-e3b0c442` (sha256 of "") | same | PASS |
| thread `héllo-世界` | `archon/thread-3e4ff432` | same | PASS |
| task `Add New Feature!!` | `archon/task-add-new-feature` | same | PASS |
| task `---Foo Bar---` | `archon/task-foo-bar` (strip lead/trail `-`) | same | PASS |
| task `a`×80 | `archon/task-` + `a`×50 (truncate 50) | same | PASS |
| task `Café Münchën 2024` | `archon/task-caf-m-nch-n-2024` (accents→`-`, runs collapsed) | same | PASS |
| task `!!!@@@###` | `archon/task-` | same | PASS |
| task `foo___bar...baz` | `archon/task-foo-bar-baz` | same | PASS |
| task `x`×50+`YZ` | `archon/task-`+`x`×50 (truncate before YZ) | same | PASS |

- **shortHash** = SHA-256, first 8 hex chars — verified (`hex::encode(&result[..4])` = 8 hex). PASS.
- **slugify** = lowercase → `[^a-z0-9]+`→`-` → strip lead/trail `-` → truncate 50. The Rust impl
  (`to_lowercase()` then keep only `is_ascii_lowercase()||is_ascii_digit()`, runs collapsed) matches
  TS byte-for-byte, including non-ASCII accents (both treat `é` as non-alnum → `-`). PASS.

**getWorktreePath precedence** — verified against the live TS oracle:

| config | TS path | Rust path | verdict |
|---|---|---|---|
| `null` / `{path:""}` | `~/.archon/workspaces/owner/repo/worktrees/<branch>` | same | PASS |
| `{path:".worktrees"}` (repo-local) | `<repoRoot>/.worktrees/<branch>` | same | PASS |
| `{path:"a/b/c"}` (nested repo-local) | `<repoRoot>/a/b/c/<branch>` | same | PASS |
| no codebaseName | owner/repo from last-2 path segments | same | PASS |
| `{path:"/abs/path"}` | throws "must be relative to the repo root (got absolute: …)" | same Err msg | PASS |
| `{path:"../escape"}` | throws "must stay within the repo (got: ../escape)" | same Err msg | PASS |
| `{path:"a/../../escape"}` | throws (escape after normalize) | Err | PASS |

Precedence order config.path repo-local > workspace-scoped default confirmed; the repo-local
join uses the **raw** repo path on both sides (Rust `get_worktree_base` joins raw `repo_path`,
matching TS `join(repoPath, repoLocal)`; canonicalization in `resolve_repo_local_override` is used
only for the escape check, not the produced path). PASS.

**Code-read parity (git-mutation paths)** — symbol-by-symbol read of source vs Rust confirmed
faithful semantics for: `create` (load-config-once, adoption-then-create), `createFromSameRepoPR`
(fetch → `worktree add -b … origin/<branch>`, "already exists" → reuse, set-upstream non-fatal),
`createFromForkPR` (sha-pinned: fetch `pull/N/head` → detached add → `checkout -b` with stale-retry;
no-sha: `fetch pull/N/head:<rb>` with stale-retry → add), `createNewBranch` (task `fromBranch`
guard: existing branch + explicit fromBranch → hard error; else `branch -f` reset to start-point),
`createBranchWithStaleRetry`, `destroy` (best-effort DestroyResult: worktreeRemoved/branchDeleted/
remoteBranchDeleted/directoryClean/warnings; `deleteRemoteBranch` option; leftover-dir `rm -rf`;
post-removal `worktree list --porcelain` verification), `deleteBranchTracked`/`deleteRemoteBranchTracked`
(not-found/already-gone → success; checked-out-elsewhere → warning+false), `copyConfiguredFiles`
(default `[".archon"]` + user dedup via Set; configLoadFailed warning), `initSubmodules` (ENOENT skip,
else throw), `get`/`list`/`adopt`/`healthCheck` (same null/registration semantics). The argv strings,
timeouts (`GIT_OPERATION_TIMEOUT_MS = 300000`), and error-classification substrings match.

**IS-02 verdict: PASS** — every symbol's contract exercised; branch naming + path precedence
proven byte-for-byte differentially; git-mutation paths confirmed by symbol-level source read.

---

## UNIT IS-03 — IsolationResolver → **FAIL (no-downgrade gate)**

The 6-stage cascade *stage selection* on the happy path matches (existing → workflow_reuse →
linked_issue_reuse → branch_adoption → created). But the differential found **4 material
behavioral divergences** vs the live TS source. The port is NOT a faithful port of `resolver.ts`.

### Cascade stage selection (PASS where noted)
| scenario | TS golden | Rust | verdict |
|---|---|---|---|
| stage1 existing, worktree on disk | `resolved/existing` | `resolved/existing` | PASS |
| stage3 workflow reuse (real wt) | `resolved/workflow_reuse` | same | PASS |
| stage4 linked-issue reuse (`linkedIssues:[99]`) | `resolved/linked_issue_reuse:99` | same | PASS |
| stage6 create new | `resolved/created` | same | PASS |

### Confirmed FAILs (each is a real divergence, reproduced over real git repos)

**FAIL-1 — stage 1 stale path: `stale_cleaned` never emitted (CRITICAL).**
- Input: `existingEnvId` points to a row whose `working_path` does NOT exist on disk.
- TS (`checkExisting` + `resolve`): `worktreeExists(working_path)` is false → `markDestroyedBestEffort`
  (calls `store.updateStatus(id,'destroyed')`) → returns `{ status: 'stale_cleaned', previousEnvId }`.
- Rust (`resolve_existing_environment`): gates on `row.status == Active` then `provider.health_check()`;
  on miss returns `Ok(None)` → `resolve()` falls through (to no-codebase `None` or to create). The
  `IsolationResolution::StaleCleaned` variant exists in `types.rs` but is **never produced** anywhere
  in `resolver.rs`. Observed: `resolved/existing` (mock health=true) / fall-through (real provider) —
  never `stale_cleaned`. The caller's "clear the stale ref and retry" contract is silently dropped.
- Also: Rust uses `provider.health_check` where TS uses `worktreeExists` directly, AND adds a
  `status==Active` precondition the source `checkExisting` does not have.

**FAIL-2 — stage 2 no-codebase cwd.**
- TS: `{ status: 'none', cwd: '/workspace' }`. Rust: `IsolationResolution::None { cwd: "" }`.
- Wrong default working directory for the no-isolation path.

**FAIL-3 — base-branch warning: wrong default + wrong string.**
- `collectBaseBranchWarnings`: TS returns `[]` when no `baseBranch` hint is present. Rust defaults
  `base_branch` to `"main"` and always runs `is_ancestor_of` → emits warnings where source emits none.
- With a mismatching hint (`baseBranch:"nonexistent-base"`), the warning text differs byte-for-byte:
  - TS: `Worktree branch 'feature/x' is not based on 'nonexistent-base'. Recreate with: archon complete feature/x --force`
  - Rust: `Your workspace may be out of date with \`nonexistent-base\`. Run \`git merge origin/nonexistent-base\` inside <wt> to update.`

**FAIL-4 — branch_adoption drops persisted metadata.**
- TS `tryBranchAdoption`: `store.create({ …, metadata: { adopted: true, adopted_from: 'skill' } })`.
- Rust `try_branch_adoption`: `store.create(CreateEnvironmentParams { …, metadata: None })`.
- The adopted-from-skill provenance is lost in the DB row.

### Additional divergence found by source-read (not yet in a runnable assertion)

**`markDestroyedBestEffort` does the wrong operation.** TS calls `store.updateStatus(envId,'destroyed')`
(marks the DB row destroyed). Rust `mark_destroyed_best_effort` calls `provider.destroy(env_id)`
(removes the physical worktree). These are different side effects on different subsystems; the Rust
`update_store_status_best_effort` helper that WOULD match is `#[allow(dead_code)]` and unused.

**Ownership-failure classification differs (stage 3/4/5).** TS `assertWorktreeOwnership` **throws** on
cross-clone mismatch (propagates → blocked/crash, preserving the other clone's row). Rust
`check_worktree_ownership` failures are **swallowed** → `Ok(None)` → falls through to create a new
env. This is a no-downgrade violation on the cross-clone guard (not yet covered by a runnable
differential because it needs two clones of one remote; recorded for the porter).

**IS-03 verdict: FAIL** — 4 reproduced differential FAILs + 2 source-read divergences. Routed back to
the porter as the precise missing behaviors above. IS-03 stays `- [~]`/`- [ ]` (unproven/not-done).

---

## UNIT IS-04 — factory `get_isolation_provider()` → **PASS (closes `- [≠]`)**

- Source `getIsolationProvider()`: `provider ??= new WorktreeProvider(configuredLoader)` — returns a
  working WorktreeProvider, no throw, when unconfigured (no-op loader → null config).
- Rust `get_isolation_provider()`: the prior panic was replaced with
  `state.provider = Some(Arc::new(WorktreeProvider::new(state.loader.clone())))` — constructs a real
  `WorktreeProvider` from the configured (default no-op) loader and returns it. Singleton reset on
  `configure_isolation`, `reset_isolation_provider` clears it — matches source semantics.
- Verified green: factory tests pass; the WorktreeProvider it now returns is the IS-02-PASS impl.

**IS-04 verdict: PASS** — `get_isolation_provider` `- [≠]` row closes to `- [x]`.

---

## Ledger updates

- `- [x]` **IS-02** `WorktreeProvider` → `har_isolation::providers::worktree::WorktreeProvider`
  (branch naming + path precedence proven byte-for-byte; git-mutation paths confirmed by source read).
- `- [x]` **IS-04** `getIsolationProvider` → `get_isolation_provider()` (closes its `- [≠]`; now
  returns a real WorktreeProvider, matching source unconfigured behavior).
- **IS-03** `IsolationResolver` stays **OPEN** (`- [ ]`/`- [~]`) — 4 differential FAILs + 2 source-read
  divergences. DO NOT flip to `- [x]`; DO NOT commit IS-03 as done.

## Overall cycle-10 verdict

**MIXED — IS-02 PASS, IS-04 PASS, IS-03 FAIL.** The cycle does not fully pass: IS-03 must go back to
the porter to fix (in priority order) FAIL-1 (emit `stale_cleaned` via FS `worktreeExists`, drop the
`status==Active`/`health_check` substitution, restore `markDestroyedBestEffort = store.updateStatus`),
FAIL-3 (no-hint → no warning; exact warning string), FAIL-4 (persist `{adopted:true,
adopted_from:'skill'}`), FAIL-2 (`/workspace` cwd), and the swallowed-ownership-error → must propagate.
Re-run `cargo test -p har-isolation --test parity_cycle10_resolver -- --ignored` after the fix; IS-03
flips to `- [x]` only when all 8 scenarios pass with the `#[ignore]` removed.

---

## 2026-06-14 — IS-03 RE-VERIFICATION (cycle 10, post-porter-fix) → **FAIL (1 NEW divergence)**

**Method:** rebuilt a fresh TS oracle (`__parity_resolver_oracle_c10.ts` + `__parity_orphan_oracle_c10.ts`)
driving the LIVE `resolver.ts` (sha256 `6f2d1813…3fc4`, bun 1.3.14) over REAL temp git repos +
real linked worktrees; diffed stage + resolution + **side effects** (store.updateStatus vs
provider.destroy call traces, create-call counts) against the Rust port. The port's tests were NOT
the oracle. Side-effect-only paths (FAIL-1, FAIL-5, orphan cleanup) were instrumented on both sides,
not inferred from return values. Transient oracles created under `Archon/packages/isolation/src/` and
**DELETED** — Archon verified pristine (`git status` clean).

### The 6 requested fixes — all CONFIRMED faithful to live source

| # | fix | TS golden (live) | Rust | verdict |
|---|---|---|---|---|
| FAIL-1 | stale → `stale_cleaned` via FS `worktreeExists`; `markDestroyedBestEffort` = `store.updateStatus` | `stale_cleaned`, updateStatus=[(id,destroyed)], destroy=[], row→destroyed (resolver.ts:96/236/420) | `StaleCleaned{id}`, update_status=[(id,destroyed)], destroy=[] | **PASS** ✓ |
| FAIL-2 | no-codebase cwd | `{none, cwd:'/workspace'}` (resolver.ts:101) | `None{cwd:"/workspace"}` | **PASS** ✓ |
| FAIL-3 | no baseBranch → `[]`; exact warning string | no-hint ⇒ `warnings` key ABSENT (resolver.ts:206); byte-exact `…not based on '<base>'. Recreate with: archon complete <branch> --force` (resolver.ts:214-216) | no-hint ⇒ `warnings:None`; byte-exact string match | **PASS** ✓ |
| FAIL-4 | adoption metadata + trigger = `prBranch` ONLY | adopt persists `{adopted:true, adopted_from:'skill'}` (resolver.ts:402); suggestedBranch-only ⇒ `resolved/created`, NO adoption (resolver.ts:170) | same metadata; suggestedBranch-only ⇒ `Created` | **PASS** ✓ |
| FAIL-5 | cross-clone ownership PROPAGATES (no fall-through to create) | two-clone reuse THROWS, createCalls=0 (resolver.ts:300-305) | `Err`, create_calls=0 | **PASS** ✓ |
| (stages) | existing→workflow_reuse→linked_issue_reuse→branch_adoption→created + 4 originals | all match | all match | **PASS** ✓ |

Differential coverage: 12 scenarios. The 8 committed golden tests
(`parity_cycle10_resolver.rs`) now pass un-ignored (0 ignored); 4 additional verifier
differentials (`parity_cycle10_sidefx_verify.rs`) prove the side-effect-only paths
(FAIL-1 store-vs-provider, FAIL-5 cross-clone propagation, FAIL-3 no-hint, FAIL-4 trigger).
clippy `--all-targets` clean; `cargo test -p har-isolation` = 123 + 3 + 8 + 4 pass.

### NEW divergence introduced by the FAIL-1 fix (stage-6 orphan cleanup) — **FAIL**

**Where:** `resolver.rs:432`, inside `create_new_environment`'s `store.create()`-failure branch.
The FAIL-1 fix correctly repurposed `mark_destroyed_best_effort` → `store.update_status` for the
STALE paths (lines 141/181/245, stages 1/3/4 — all verified correct). But the **stage-6 orphan-cleanup**
path still calls that same now-repurposed helper, so it does the WRONG side effect.

- **TS source (resolver.ts:536-541):** `await this.provider.destroy(isolatedEnv.workingPath,
  { canonicalRepoPath, branchName, force: true })` — physically removes the orphaned worktree.
- **Rust (resolver.rs:432):** `self.mark_destroyed_best_effort(&working_path)` →
  `store.update_status(working_path, "destroyed")` — marks a DB row, passing the **filesystem path**
  `/new/wt` where an env-id is expected.

**Differential (instrumented, both sides, store.create forced to fail):**
- TS golden: `destroyCalls=["/orphan/wt"]`, `updateStatusCalls=[]`.
- Rust: `destroyCalls=[]`, `updateStatusCalls=[("/new/wt","destroyed")]`.

**Impact (no-downgrade violation):** the orphaned physical worktree is NEVER removed (orphan leak),
AND against the real SQL store `update_status("/new/wt", …)` updates zero rows (path ≠ id). The
resolver.rs:425 comment even says "destroy the physical worktree" while the code marks a DB row.
This is the FAIL-1 bug mirrored onto the orphan path.

**Reproduction:** `cargo test -p har-isolation --test parity_cycle10_sidefx_verify -- --ignored`
(`stage6_orphan_cleanup_uses_provider_destroy_not_store_update`, marked `#[ignore]` so the baseline
stays green; asserts the TS golden, FAILs until fixed).

**Required fix (routed to porter):** at resolver.rs:432, call
`self.provider.destroy(&working_path, Some(DestroyOptions{ … canonical_repo_path, branch_name,
force:true }))` (a best-effort try-catch that logs but does not mask the original store error), NOT
`mark_destroyed_best_effort`. Keep lines 141/181/245 on `mark_destroyed_best_effort` (those are
correct). After the fix, remove the `#[ignore]`; re-run all 12 scenarios.

### IS-03 verdict: **FAIL** — 6 requested fixes PASS, but 1 new side-effect divergence (stage-6
orphan cleanup: store.update_status instead of provider.destroy). IS-03 stays `- [~]`/`- [ ]`
(do NOT flip to `- [x]`; do NOT commit IS-03 as done). 5/6 + originals proven; the single open item
is resolver.rs:432.

---

## 2026-06-14 (re-verify #2) — FINAL: stage-6 fix-induced regression confirmed CLOSED

**Trigger:** the 2nd fix-induced regression (FAIL-1 fix had repurposed `mark_destroyed_best_effort`
onto the stage-6 orphan path). Porter has now fixed resolver.rs stage-6. Re-verified differentially.

### Stage-6 orphan cleanup — NOW MATCHES
- **SOURCE** `resolver.ts:536-541`: `await this.provider.destroy(isolatedEnv.workingPath,
  { canonicalRepoPath, branchName, force: true })`; cleanup is best-effort (try/catch logs at
  551-556); `throw err` at 559 re-propagates the ORIGINAL store error.
- **RUST** `resolver.rs:435-456`: `self.provider.destroy(&working_path,
  Some(DestroyOptions{ force:Some(true), branch_name, canonical_repo_path, ..Default::default() }))`;
  `if let Err(destroy_err) = … { error!(…) }` logs without masking; line 454 returns the wrapped
  ORIGINAL store error. **Behavioral match.**
- **Differential (instrumented Fx, store.create forced to fail, both sides):**
  - TS golden: `destroyCalls=["<orphan/wt>"]`, `updateStatusCalls=[]`, original store error thrown.
  - Rust now: `destroy_calls==["/new/wt"]`, `update_status_calls.is_empty()`, `res.is_err()`. **MATCH.**
- Golden test `stage6_orphan_cleanup_uses_provider_destroy_not_store_update`
  (`parity_cycle10_sidefx_verify.rs:401`) is **un-ignored** and **PASSES** (asserts all three: the
  destroy path, the absence of update_status, and original-error propagation).

### Exhaustive side-effect call-site re-scan (resolver.rs ⇄ resolver.ts) — no other mis-use
| TS site | Semantics | Rust site | Verdict |
|---|---|---|---|
| 248 (stage 1 stale) | `markDestroyedBestEffort` → updateStatus | resolver.rs:141 `mark_destroyed_best_effort` | OK (unchanged) |
| 315 (stage 3 reuse stale) | `markDestroyedBestEffort` | resolver.rs:181 | OK (unchanged) |
| 360 (stage 4 linked-issue stale) | `markDestroyedBestEffort` | resolver.rs:245 | OK (unchanged) |
| 418-420 (helper) | `store.updateStatus(id,'destroyed')` | resolver.rs:612-620 | OK |
| 537 (stage 6 orphan) | `provider.destroy(force:true)` | resolver.rs:435 | **FIXED — now matches** |
TS has exactly ONE `provider.destroy` (537) and ONE `markDestroyedBestEffort` helper; Rust mirrors
1:1. No hidden stale-cleanup destroy path on either side (grep over `.destroy`/`updateStatus`/
`auto_cleanup` is exhaustive). The stages-1/3/4 `mark_destroyed_best_effort` rows (141/181/245)
correctly remain on `store.update_status` (those map to `markDestroyedBestEffort`, which IS
updateStatus). No new mis-use introduced.

### No regression of the 6 prior fixes
Full `cargo test -p har-isolation`: **139 passed, 0 failed, 0 ignored** across all suites
(`parity_cycle10_resolver` 123, `parity_cycle10_branchnaming` 3, `parity_cycle10_sidefx_verify` 8,
lib). The cycle-10 sidefx suite's other 4 differential tests still pass: stale→update_status
(`fail1`), cross-clone reuse propagates with no create (`fail5`), no-base-branch-hint→no warning
(`fail3`), suggested-branch-only→no adopt (`fail4`). adoption metadata / no-codebase cwd /
cross-clone all green in `parity_cycle10_resolver`. **No prior fix regressed.**

### Baseline
`cargo clippy --workspace --all-targets` → **No issues found.** Archon working tree clean; no
transient oracle/parity-tmp artifacts left behind (cleaned/none).

### IS-03 FINAL VERDICT: **PASS** — flip to `- [x]`
All 6 original FAILs fixed and confirmed, the 1 fix-induced stage-6 regression fixed and confirmed,
exhaustive side-effect call-site map verified 1:1 with the source, 0 ignored / 0 failed, baseline
green. Every IS-03 symbol contract exercised differentially. No downgrade. **IsolationResolver is
parity-verified.** IS-02 and IS-04 remain **PASS** (unchanged this cycle). Cycle-10 is clear to
commit.
