
---

# Parity Verdict — Cycle 9 (har-isolation foundation) — 2026-06-14

**Verifier:** rust-port-parity-verifier (differential, live bun 1.3.14 ⇄ Rust)
**Units:** IS-01 types, IS-04 factory, IS-05 pr-state, IS-06 worktree-copy, IS-07 errors, IS-08 store
**Method:** transient TS oracles (run via `bun`, using ONLY source modules) emitting canonical JSON ⇄ a Rust example oracle emitting the same shape, then field-by-field diff. Plus LIVE copy-semantics oracles (real temp dir trees, run both `copyWorktreeFiles` impls, diff what landed). Oracles deleted; Archon left pristine (git clean).

## Overall: **PASS** (1 real divergence found and FIXED; 1 intentional scope `- [≠]`)

### IS-01 types — PASS (wire contract proven)
- **IsolationRequest discriminant strategy CONFIRMED CORRECT.** Source `types.ts:57-97` is a structural union where every variant carries a `workflowType: '<literal>'` field. The porter's `#[serde(tag = "workflowType")]` produces **byte-identical wire JSON** to bun for all 5 variants. Differential round-trip (bun-serialize ⇄ serde-serialize) MATCHED exactly:
  - Issue/PR(+prSha)/PR(no prSha → field absent, not null)/Review/Thread/Task(+fromBranch)/Task(no fromBranch → absent)/Issue(+all optionals: codebaseName, description, gitIdentity{email,name}).
  - The tag field is `workflowType` (NOT `type`/`kind`) — verified against source literals. A wrong tag would be wire-incompatible; this is correct.
  - Unknown `workflowType` → serde rejects (Err). Matches the TS exhaustive union (no catch-all variant).
- `isPRIsolationRequest` predicate: `guard_pr=true, guard_issue=false` — MATCH.
- `DestroyResult` nullable fields: source uses `boolean | null` (explicit `null`, not absent) → Rust `Option<bool>` with NO `skip_serializing_if` on `branchDeleted`/`remoteBranchDeleted` → serializes `null` (matches), deserializes `null`→None (matches). Verified by unit + shape read.
- camelCase wire names + optionality for WorktreeEnvironment, IsolationHints, WorktreeCreateConfig, ResolveRequest, WorktreeStatusBreakdown, ResolutionMethod (`type`-tagged snake_case), IsolationResolution (`status`-tagged snake_case) — read both sides; consistent.

### IS-06 worktree-copy — **FAIL → FIXED → PASS** (the load-bearing find)
**REAL DIVERGENCE (path-traversal guard, absolute-path entries):**
- Input: `isPathWithinRoot("/repo", "/etc/passwd")`
  - **TS (source):** `true` — Node `path.join('/repo','/etc/passwd')` === `/repo/etc/passwd` (absolute arg is APPENDED under root, never overrides). `relative('/repo','/repo/etc/passwd')`=`etc/passwd` → within.
  - **Rust (as ported):** `false` — Rust `Path::join("/repo","/etc/passwd")` REPLACES → `/etc/passwd` → `strip_prefix` fails → blocked.
- End-to-end distinguishing case (live): entry `/etc/hosts` with `<src>/etc/hosts` present:
  - **TS:** `copied=["/etc/hosts"]`, copies `<src>/etc/hosts` → `<dst>/etc/hosts`.
  - **Rust (as ported):** `copied=[]`, nothing copied. **Behavioral downgrade.**
- **Root cause:** Rust `Path::join` vs Node `path.join` absolute-arg semantics, in BOTH the guard AND the real copy join (`source_root.join(entry.source)` would also have read the REAL `/etc/hosts`, not the under-root one).
- **Fix applied** (`worktree_copy.rs`): added `node_join()` (strips leading separators so the entry is always appended under root, mirroring Node), used in `is_path_within_root` and `copy_worktree_file`. Re-ran both oracles → **ALL 14 path cases MATCH**; distinguishing copy case now `copied=["/etc/hosts"]`, lands under dst — identical to TS.
- **Also corrected** the porter's test `path_within_root_absolute_escapes` (asserted `!within` for `/etc/passwd`) — that was a porter ASSUMPTION that diverged from source; renamed to `path_within_root_absolute_is_appended_under_root` asserting `within==true`. Added 2 durable regression tests pinning the absolute-entry parity.
- All OTHER copy behavior MATCHED differentially: relative file copy, recursive dir copy, ENOENT-silent skip, parent-dir creation, `../` traversal rejection (`../outside/file`, `../../other/.env`, `sub/../../escape` → blocked on both; `../repo/x`, `./sub/../file`, `../b/c` normalizing back inside → within on both), empty/`.`/trailing-slash roots.
- `parseCopyFileEntry`: trim, empty-reject ("Copy entry cannot be empty"), source==destination — MATCH (incl. whitespace-only → error).

### IS-07 errors — PASS (exact)
- All 13 `ERROR_PATTERNS` (pattern string + exact message + `known` flag) MATCH source. `cannot extract owner/repo` is the only `known:false` (verified both classify-able AND not-known).
- `classifyIsolationError`: 18-case battery incl. combined message+stderr, lowercasing, first-match order (e.g. `"eacces AND timeout"` → permission-denied message wins, matching source array order), and the fallback `**Error:** Could not create isolated workspace (<message>).` — ALL MATCH.
- `isKnownIsolationError`: MATCH (permission-denied→true, cannot-extract→false, unknown→false).
- `IsolationBlockedError` shape: `{message, reason}`, `Display`==message — MATCH.

### IS-05 pr-state — PASS (shape, NEEDS-HUMAN resolved)
- `PrState` enum (Merged/Closed/Open/None) + `get_pr_state` shape verified by reading both: cache-hit early-return, `git remote get-url origin` (10s) → err→debug+cache None+return None, non-`github.com` remote (lowercased `.contains`) → debug+cache None+return None, `gh pr list --head <b> --state all --json state --limit 1` (15s, cwd=repo) JSON parse `[{state?}]`, ENOENT/"command not found"→debug vs other→warn, cache-on-result. Matches source `pr-state.ts:30-91`. (Live `gh` call not differential-tested — soft dependency, as scoped; cache-hit + non-existent-repo→None paths confirmed via Rust unit tests.)

### IS-04 factory — PASS, with one intentional `- [≠]`
- `configure_isolation` (sets loader + nulls provider), `reset_isolation_provider` (nulls provider), configured-loader-is-returned, singleton identity — MATCH source semantics (verified via serial tests).
- **`get_isolation_provider` unconfigured behavior — INTENTIONAL DIVERGENCE `- [≠]`:** source (oracle-confirmed) RETURNS a `WorktreeProvider` (providerType `worktree`), never throws. Rust port **panics** because `WorktreeProvider` (IS-02) is explicitly the NEXT cycle (out of scope). This is a documented, scope-bounded gap (factory.rs:70-84), not a silent downgrade. **MUST be reconciled when IS-02 lands** (panic → construct `WorktreeProvider::new(loader)`); flagged `- [≠]` pending that.

### IS-08 store — PASS (interface shape)
- `IIsolationStore` 5 methods (getById, findActiveByWorkflow, create, updateStatus, countActiveByCodebase) + data shapes (`CreateEnvironmentParams`, `IsolationEnvironmentRow`, `IsolationWorkflowType`) match source interface `store.ts:7-17`. Trait signatures + in-memory test stub behavior verified.

## Symbols flipped to `- [x]` (this cycle)
- IS-01: all 19 symbols `- [x]`
- IS-04: configureIsolation, resetIsolationProvider `- [x]`; getIsolationProvider `- [≠]` (scope, pending IS-02)
- IS-05: PrState, getPrState `- [x]`
- IS-06: parseCopyFileEntry, isPathWithinRoot, copyWorktreeFile, copyWorktreeFiles `- [x]` (after node_join fix)
- IS-07: IsolationBlockedError, classifyIsolationError, isKnownIsolationError, ERROR_PATTERNS `- [x]`
- IS-08: IIsolationStore `- [x]`

## Durable fixtures committed under crate
- `worktree_copy.rs`: `path_within_root_absolute_is_appended_under_root` (corrected), `path_within_root_absolute_entry_is_appended_not_replaced`, `copy_absolute_entry_reads_under_root_not_real_path` — pin the absolute-path / Node-join parity.

## Baseline
- `cargo test -p har-isolation`: 78 passed (was 76 + 2 regression). `cargo clippy -p har-isolation --all-targets`: clean.

## Cycle-9 verdict: **PASS** — all six units behaviorally parity-verified against live source; 1 real divergence found by differential testing and fixed (Node-join absolute-path semantics); 1 intentional scope `- [≠]` (factory get-provider, pending IS-02). The porter's own tests were NOT the oracle — bun was — and one porter test encoded a wrong assumption that the differential caught.
