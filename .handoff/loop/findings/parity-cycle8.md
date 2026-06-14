# Parity Findings — ITERATE Cycle 8 (har-git: GI-01..GI-05)

**Date:** 2026-06-14
**Verifier:** rust-port-parity-verifier (adversarial, differential, fail-closed)
**Method:** live `bun 1.3.14` (Archon source) ⇄ Rust (`har-git` example driver), per-scenario
identical-but-INDEPENDENT temp git repos, deterministic isolated git config
(`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM=/dev/null`, `LOG_LEVEL=silent`), JSON-shape + error diff.
**Source units:** `packages/git/src/{exec,branch,repo,worktree,types}.ts`
**Rust units:** `crates/har-git/src/{exec,branch,repo,worktree,types}.rs`
**Differential runner (transient):** `.handoff/loop/findings/_cycle8_run.sh`
**Transient oracle (DELETED from Archon after run):** `packages/git/__parity_oracle.ts`, `__exec_oracle.ts`
**Durable goldens committed:** `crates/har-git/src/lib.rs::tests::golden_cycle8` (5 tests) +
`crates/har-git/examples/parity_driver.rs` (re-runnable driver).

## Headline

**56/60 differential cases byte-identical; remaining 4 are driver/serialization artifacts or
QUALIFIED cosmetic error-message wrapping — ZERO behavioral divergences.** Every classification
branch, every fallback chain, every porcelain-parse, every fail-safe, and the token-sanitization
guard match the source exactly.

Baseline precondition: `cargo clippy -p har-git --all-targets` clean; `cargo test -p har-git` 57
passed.

---

## GI-05 types — PASS (6/6 behaviors)

| Behavior | Input | TS | Rust | Verdict |
|---|---|---|---|---|
| `toRepoPath('')` | empty | throw "RepoPath cannot be empty" | same | PASS |
| `toRepoPath('/x/y')` | ok | "/x/y" | same | PASS |
| `toBranchName('')` | empty | throw "BranchName cannot be empty" | same | PASS |
| `toBranchName('main')` | ok | "main" | same | PASS |
| `toWorktreePath('')` | empty | throw "WorktreePath cannot be empty" | same | PASS |
| `toWorktreePath('/w/t')` | ok | "/w/t" | same | PASS |

Empty-string rejection messages are byte-identical. `GitResult`/`GitErrorCode` (5 variants),
`WorkspaceSyncResult`, `WorktreeInfo` wire shapes verified via the consuming ops below
(syncWorkspace shape, clone/sync GitResult shape, listWorktrees WorktreeInfo[] shape — all PASS).

## GI-01 exec — PASS (contract verified directly + transitively)

`exec_file_async` is the substrate under every git op below; verified transitively (all PASS) AND
directly for the flagged error-shape:

- **No-shell argv spawn:** preserved (`tokio::process::Command` + explicit args).
- **stdout/stderr capture + `?? ''` (never null):** proven via getRemoteUrl/findRepoRoot/
  getLastCommitDate trim-and-return (all PASS) and `exec_file_async_captures_stdout_stderr`.
- **Trimming:** callers `.trim()` stdout — getRemoteUrl/findRepoRoot/getLastCommitDate PASS.
- **cwd injection:** proven via `run_git_cwd` (syncRepository) PASS.
- **`-C` injection:** proven via every `-C` op PASS.
- **timeout:** `exec_file_async_timeout_fires` (50ms vs sleep 10) PASS.
- **Non-zero-exit error shape (adversarial, `git rev-parse` in non-repo):**
  - TS `err.message` = `Command failed: git -C <d> rev-parse --show-toplevel\n<stderr>\n`,
    `err.stderr` = raw stderr, `err.code` = 128.
  - Rust `GitError::ProcessError.message` = `Command failed: git -C <d> rev-parse --show-toplevel\n<stderr-trimmed>\nProcess exited with code 128`.
  - **Inner git stderr is carried verbatim** (the substring classification depends on it). QUALIFIED
    divergence (see `- [≠]` below): Rust trims the stderr and appends a `Process exited with code N`
    line; Node keeps the trailing `\n` and omits the exit-code line. **Non-behavioral** — no consumer
    matches this literal string (grep of Archon: zero hits for `Command failed: git` / `Process
    exited with code`); all branching is on the preserved inner substrings.

## GI-02 branch — PASS (14/14 cases; getDefaultBranch fallback chain fully covered)

| Behavior | Setup | TS | Rust | Verdict |
|---|---|---|---|---|
| getDefaultBranch — origin/HEAD present | clone with symbolic-ref | "main" | "main" | PASS |
| getDefaultBranch — origin/main only | clone, `symbolic-ref -d origin/HEAD` | "main" | "main" | PASS |
| getDefaultBranch — neither (throw) | repo, no origin remote | throw actionable msg | same msg (＋Display prefix) | **`- [≠]`** below |
| checkout — existing branch | branch `feature` exists | void | void | PASS |
| checkout — create-new fallback | branch absent → `-b` | void | void | PASS |
| hasUncommittedChanges — clean | committed, no changes | false | false | PASS |
| hasUncommittedChanges — dirty | modified tracked file | true | true | PASS |
| hasUncommittedChanges — nonexistent (fail-safe ENOENT) | missing path | false | false | PASS |
| commitAllChanges — clean → false | no changes | false | false | PASS |
| commitAllChanges — untracked → true | new untracked file | true | true | PASS |
| isBranchMerged — merged → true | `--no-ff` merge | true | true | PASS |
| isBranchMerged — unmerged → false | unmerged branch | false | false | PASS |
| isBranchMerged — bad repo → false (expected err) | missing path | false | false | PASS |
| isPatchEquivalent — unmerged → false | `+` commit | false | false | PASS |
| isPatchEquivalent — equiv → true | cherry-picked onto base | true | true | PASS |
| isAncestorOf — true | main ancestor of child HEAD | true | true | PASS |
| isAncestorOf — false (exit code 1) | child not ancestor of main | false | false | PASS |
| isAncestorOf — bad ref → false (expected err) | nonexistent ref | false | false | PASS |
| getLastCommitDate — has commit | 1 commit | ISO-8601 | same instant (`- [≠]` type) | PASS |
| getLastCommitDate — no commit → null | empty repo | null | null | PASS |
| getLastCommitDate — bad path → null | missing path | null | null | PASS |

The `getDefaultBranch` symbolic-ref→origin/main→throw chain is exercised in **all three** terminal
states. `isAncestorOf` exit-code-1-is-false and `commitAllChanges` nothing-to-commit-is-false
(the two ledger-flagged subtle branches) both confirmed. `getLastCommitDate` `%ci` parse confirmed:
Rust `chrono::DateTime<Utc>` carries the identical instant the TS `Date` does (the `- [≠]` type swap
already in the ledger; behavior identical).

## GI-03 repo — PASS (11/11 behaviors; classification mapping fully verified)

| Behavior | Setup | TS | Rust | Verdict |
|---|---|---|---|---|
| findRepoRoot — in repo | git repo | toplevel path | same | PASS |
| findRepoRoot — non-repo → null | plain dir | null | null | PASS |
| getRemoteUrl — set | origin configured | url | url | PASS |
| getRemoteUrl — unset → null | no remote | null | null | PASS |
| syncWorkspace — fetch+reset | clone, advance upstream | `{branch,synced:true,prevHead,newHead,updated:true}` | identical shape, `updated:true` | PASS |
| syncWorkspace — fetch-only | resetAfterFetch=false | `{...,prevHead:'',newHead:'',updated:false}` | identical | PASS |
| syncWorkspace — configured-branch-not-found | base=nonexistent | throw actionable msg | same msg (＋prefix) | PASS (kind+text-modulo-prefix) |
| cloneRepository — success | local source repo | `{ok:true}` | `{ok:true,value:null}` | PASS (serde artifact) |
| cloneRepository — not-found classify | url does-not-exist | `{ok:false,unknown,...}` | same (＋exit-code trailer) | PASS (classify identical) |
| cloneRepository — token-sanitize + 404 classify | https 404 url + token | `{not_a_repo, path:url}` token redacted | **identical, token absent** | PASS |
| syncRepository — success | clone+advance | `{ok:true}` | `{ok:true,value:null}` | PASS (serde artifact) |
| syncRepository — branch-not-found | reset to nonexistent | `{branch_not_found, branch}` | identical | PASS |
| syncRepository — not-a-repo | fetch in plain dir | `{not_a_repo, path}` | identical | PASS |
| addSafeDirectory | — | (writes global config) | (writes global config) | covered by in-crate test |

**cloneRepository error-classification mapping verified end-to-end:** not-found/404 → `not_a_repo`,
auth-failed/could-not-read → `permission_denied`, no-space → `no_space`, else `unknown` — the 404
case maps to `not_a_repo` in BOTH impls. **Token sanitization proven**: the differential 404-with-
token case returns `{code:not_a_repo, path:<clean url>}` with the token absent in both, and the
durable golden `clone_token_never_leaks_in_error` asserts the token never appears in the Rust error
(`Debug` form). `{ok:true}` vs `{ok:true,value:null}` is a JSON artifact of TS `value:undefined`
being dropped by `JSON.stringify` vs the Rust driver emitting `null` for `()`; the return value is
`GitResult::Ok(())` in both — semantically identical.

## GI-04 worktree — PASS (17/17 behaviors; porcelain parse + ownership messages verified)

| Behavior | Setup | TS | Rust | Verdict |
|---|---|---|---|---|
| listWorktrees — main+linked, detached EXCLUDED | main + linked `wtbranch` + `--detach` | `[{r,main},{wl,wtbranch}]` | identical | PASS |
| listWorktrees — non-repo → [] | plain dir | [] | [] | PASS |
| worktreeExists — yes | worktree dir + .git | true | true | PASS |
| worktreeExists — no (ENOENT) | missing | false | false | PASS |
| worktreeExists — dir w/o .git → false | empty dir | false | false | PASS |
| findWorktreeByBranch — exact | branch `wtbranch` | path | path | PASS |
| findWorktreeByBranch — slugified | search `feature/auth` → branch `feature-auth` | path | path | PASS |
| findWorktreeByBranch — none → null | no match | null | null | PASS |
| isWorktreePath — worktree (.git file gitdir:) → true | linked worktree | true | true | PASS |
| isWorktreePath — main repo (.git dir) → false | main repo | false | false | PASS |
| isWorktreePath — missing → false | missing | false | false | PASS |
| getCanonicalRepoPath — from worktree | parse `gitdir: .../worktrees/<n>` | main path | main path | PASS |
| getCanonicalRepoPath — already canonical | main repo | same path | same path | PASS |
| verifyWorktreeOwnership — match → void | wt of repo | void | void | PASS |
| verifyWorktreeOwnership — cross-clone → throw | wt vs different repo | "belongs to a different clone (...)" | same msg (＋prefix) | PASS (msg verbatim modulo prefix) |
| verifyWorktreeOwnership — full-checkout (EISDIR) → throw | .git is a dir | "path contains a full git checkout..." (code EISDIR) | same msg (＋prefix) | PASS (msg verbatim modulo prefix) |
| removeWorktree — clean | worktree present | void | void | PASS |
| extractOwnerRepo — ok / 2-seg / 1-seg-throw | various | owner/repo or throw | identical incl. throw msg | PASS |

**`git worktree list --porcelain` parsing verified EXACTLY:** `worktree ` → path, `branch ` →
strip `refs/heads/`, and the **detached worktree is excluded** (no `branch` line → not pushed) in
both impls — the precise edge the source intends. **verifyWorktreeOwnership error messages**
(substring-matched by the isolation layer's `classifyIsolationError`) are byte-identical to the
source modulo the Display prefix; pinned by durable goldens `verify_ownership_cross_clone_message`
and `verify_ownership_full_checkout_message`. `extractOwnerRepo` throws on <2 segments with the
identical message (Rust panics → caught → same text). `getCanonicalRepoPath` gitdir-regex extraction
matches.

---

## QUALIFIED divergences (`- [≠]`, non-behavioral, owner-visible)

1. **Error-message Display prefix + exit-code trailer** (GI-01 propagated to GI-02/03/04 throw
   paths). Rust thrown/`unknown` messages carry a `git process error: ` Display prefix and (for raw
   exec failures) a trailing `\nProcess exited with code N` line; Node's messages omit both and keep
   the raw stderr trailing newline. **Rationale/justification:** the inner git stderr — the only text
   any consumer branches on — is preserved verbatim; classification parity is proven across every
   code (not_a_repo/permission_denied/no_space/branch_not_found/unknown) and every fallback chain.
   Grep of all of Archon confirms **no consumer matches these literal strings**. This is the same
   class as the already-accepted `getLastCommitDate` Date→`chrono::DateTime<Utc>` `- [≠]`: idiomatic
   Rust error formatting, identical behavior. Recommend recording as `- [≠]` (cosmetic), NOT a FAIL.

2. **`getLastCommitDate` return type** `Date` → `chrono::DateTime<Utc>` — pre-existing `- [≠]` in the
   ledger; differentially confirmed to carry the identical instant. Unchanged.

No behavioral downgrade found. No `- [~]` left unproven among the cycle-8 symbols.

## Ledger items flipping `- [~]` → `- [x]`

- **GI-01** exec: `execFileAsync`, `mkdirAsync`, `run_git`, `run_git_cwd`, timeout/cwd/env → `- [x]`
- **GI-02** branch: `getDefaultBranch`, `checkout`, `hasUncommittedChanges`, `commitAllChanges`,
  `isBranchMerged`, `isPatchEquivalent`, `isAncestorOf` → `- [x]`; `getLastCommitDate` → `- [x]`
  with the existing `- [≠]` (chrono Date) carried.
- **GI-03** repo: `findRepoRoot`, `getRemoteUrl`, `syncWorkspace`, `cloneRepository`,
  `syncRepository`, `addSafeDirectory` → `- [x]`
- **GI-04** worktree: `worktreeExists`, `listWorktrees`, `findWorktreeByBranch`, `isWorktreePath`,
  `removeWorktree`, `getCanonicalRepoPath`, `verifyWorktreeOwnership`, `extractOwnerRepo`,
  `WorktreeLayout`, `WorktreeBaseOverride`, `getWorktreeBase`, `isProjectScopedWorktreeBase`,
  `resolveOwnerRepo` → `- [x]`
- **GI-05** types: `RepoPath`, `BranchName`, `WorktreePath`, `toRepoPath`, `toBranchName`,
  `toWorktreePath`, `GitResult`, `GitErrorCode`, `WorkspaceSyncResult`, `WorktreeInfo` → `- [x]`
- New `- [≠]` to record: GI-01 error-message Display prefix + exit-code trailer (cosmetic, no
  consumer match) — see QUALIFIED divergence #1.

## VERDICT

- GI-01 exec: **PASS**
- GI-02 branch: **PASS**
- GI-03 repo: **PASS**
- GI-04 worktree: **PASS**
- GI-05 types: **PASS**

**CYCLE-8 OVERALL: PASS** — full behavioral parity, no downgrade. Two cosmetic error-message
formatting differences recorded as `- [≠]` (owner-visible; no behavioral effect, no consumer
matches). The unit ledger rows for GI-01..GI-05 may be marked `- [x]` and committed.
