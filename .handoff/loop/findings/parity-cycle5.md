# Parity findings — Cycle 5 (UNIT WF-11: executor-shared.ts)

**Date:** 2026-06-13
**Source X:** `meta/Archon` `packages/workflows/src/executor-shared.ts` (+ `command-validation.ts`,
`packages/paths/src/archon-paths.ts`), run via `bun` 1.3.14.
**Rust port:** `crates/har-dag-executor/src/executor_shared.rs`.
**Method:** DIFFERENTIAL (live `bun` ⇄ Rust). Transient bun oracles recreated in Archon, run,
captured to durable golden JSON under the crate, then DELETED (Archon left pristine — `git status` clean).
**Durable fixtures committed under the crate:**
- `tests/golden/cycle5_ts_oracle.json` + `tests/cycle5_differential.rs` (6 behavior groups)
- `tests/golden/cycle5_ts_oracle_adv.json` + `tests/cycle5_adversarial.rs` (backtracking/boundary edge cases)
- `tests/golden/cycle5_fuzz.json` (800 PRNG cases) + `tests/cycle5_fuzz.rs`
- `tests/golden/cycle5_ts_subproc.json` + `tests/cycle5_subproc.rs` (UTF-16 truncation)

Gate runs in CI: `cargo test -p har-dag-executor` (201 pass) + `cargo clippy --all-targets` (clean).

---

## Overall cycle-5 gate verdict: **PASS (re-verification 2026-06-13 — unit flips to `- [x]`)**

3 genuine behavioral divergences were found and **FIXED** (regex/encoding); a 4th **precedence
divergence** in `loadCommandPrompt` was escalated, the porter **aligned the port to the ACTUAL
source**, and that correction is now **differentially re-verified** below. All 15 WF-11 symbols are
`- [x]`.

> **NOTE (superseded):** the original cycle-5 verdict was **FAIL (unit held open)** on the
> `loadCommandPrompt` precedence divergence. That block is preserved below for the audit trail; the
> re-verification block immediately following is the operative verdict.

14 of 15 WF-11 symbols were `- [x]` at first pass; `loadCommandPrompt` is now also `- [x]`.

---

## Per-behavior verdicts

### 1. `classifyError` — **PASS**
FATAL>TRANSIENT priority, message lowercased before match, all 9 FATAL + 15 TRANSIENT members,
mixed-case, and the load-bearing mixed-pattern case all match bun.
- `"unauthorized: process exited with code 1"` → FATAL (both). `"credit balance timeout error"` → FATAL (both).
- `"UNAUTHORIZED"`/`"TIMEOUT"` → FATAL/TRANSIENT (both). `"http 429 too many requests"` → TRANSIENT (both).

### 2. `substituteWorkflowVariables` — **FAIL → FIXED → PASS**
- All 9 `$VAR`, shell-safe skip set, empty-`$BASE_BRANCH`-referenced error (exact message), `$DOCS_DIR`
  default `"docs/"`, global replacement: all match bun.
- **DIVERGENCE (FIXED): `$CONTEXT` boundary.** The porter replaced JS zero-width `(?![A-Za-z0-9_])`
  with a capture-group `([^A-Za-z0-9_]|$)` that **consumes** the boundary char.
  - Divergent input: `"$CONTEXT$CONTEXT"`, issueContext=`"C"`.
    - **TS:** `"CC"` (lookahead is zero-width; the shared `$` is not consumed, so both vars match).
    - **Rust (before):** `"C$CONTEXT"` (first match consumed the second var's leading `$`).
  - All other boundary chars (`.`, space, `,`, `-`, newline, tab) were already correct — only
    **directly-adjacent** context vars diverged.
  - **Fix:** `substitute_context_vars()` — name-only regex + manual zero-width boundary assertion;
    the boundary char is never consumed. Verified PASS over 800-case fuzz (adjacent vars, every fill char).

### 3. `detectCompletionSignal` / `stripCompletionTags` — **FAIL → FIXED → PASS**
- XML matching-tag (same + different case), non-matching tags, plain end/own-line, `<promise>` strip,
  false-positive `"not SIGNAL yet"`, trimming, attrs, regex-special signals: all match bun.
- **DIVERGENCE (FIXED): JS `i`-flag backreference backtracking.** The porter reimplemented `</\1>` with
  two independent tag captures + `eq_ignore_ascii_case`, which does NOT backtrack the open-tag name.
  - Divergent input: `detectCompletionSignal("<ab>SIG</a>", "SIG")`.
    - **TS:** `true` (`\1` backtracks to `a`, the `b` absorbed by `[^>]*`, matches `</a>`).
    - **Rust (before):** `false` (greedily captured open=`ab`, close=`a`, unequal, no shorter retry).
  - **Fix:** capture the full open-tag inner and apply `close_is_backref_of_open()` — a match exists iff
    `close` (case-insensitive) is a **prefix** of the open-tag inner (the exact set of values `\1` can take).
    `<a>SIG</ab>` correctly stays `false`. Applied to both detect and strip. Fuzz-verified (400 tag-soup cases).

### 4. `formatSubprocessFailure` — **FAIL → FIXED → PASS**
- `Command failed:` prefix strip, first-line removal, stderr preference, exit-code/killed suffix,
  no-diagnostic / unknown-error fallbacks: all match bun.
- **DIVERGENCE (FIXED): 2000-char truncation counted BYTES, not UTF-16 code units.** JS `String.length`
  / `slice(-2000)` count UTF-16 code units; the port used `diagnostic.len()` (bytes) + byte-slicing.
  - Divergent input: `stderr = "é".repeat(2500)` (2-byte char), label `"s"`.
    - **TS:** keeps last **2000 `é`** (UTF-16 len 2500 > 2000) → user msg 2023 chars.
    - **Rust (before):** byte-len 5000, kept last 2000 **bytes** = **1000 `é`** → user msg 1023 chars.
  - Emoji (4-byte, 2 UTF-16 units) diverged 4×. Boundary case `"A"*1999 + "😀" + "B"*10` also diff-tested.
  - **Fix:** `utf16_tail()` — slices the last N **UTF-16 code units**, applied to both the diagnostic and
    the `stderrTail` log field. ASCII paths unchanged. Verified PASS on é/emoji/surrogate-boundary inputs.

### 5. `detectCreditExhaustion` — **PASS**
Each session-limit + credit pattern, reset-time regex `[^\n·.!]+` stop-char extraction (incl. middle-dot
`·` U+00B7, `.`, `!` boundaries and case-insensitive `RESETS`), and exact returned strings match bun.

### 6. `isInlineScript` — **PASS**
Every char in the class `[;(){}&|<>$\`"' ]` (14 chars) → inline; newline → inline; plain name / `my.script`
/ tab / empty → not inline. All match bun.

### 7. `loadCommandPrompt` — **QUALIFIED / UNPROVEN (escalated)**
The 5 `LoadCommandResult` failure reasons (InvalidName, NotFound, EmptyFile, PermissionDenied, ReadError)
and the fake-FS control flow are covered by the port's unit tests and match the source's per-scope logic.
**HOWEVER**, the **search-path precedence is an unflagged divergence from source** and is NOT an
owner-approved `- [≠]`:
- **Source** (`archon-paths.ts:183` `getCommandFolderSearchPaths`): order is
  `[".archon/commands", ".archon/commands/defaults"]` then `configuredFolder` **appended LAST**
  (lowest precedence, only if not already present), then home, then bundled.
- **Port** (`command_folder_search_paths`, executor_shared.rs:~816): order is
  `configuredFolder` **FIRST** (highest precedence) → `.archon/commands/` → `.claude/commands/` → home → bundled.
- **Two concrete divergences:**
  1. `configuredFolder` precedence is **inverted** (source: lowest; port: highest). The port's own test
     `configured_folder_takes_precedence_over_archon_commands` asserts the OPPOSITE of source behavior.
  2. The port **invented `.claude/commands/`** and **dropped the source's `.archon/commands/defaults`** scope.
- This is plausibly an intended adaptation (harness-agent-rs adopting `.claude/` conventions), but ADR-0001
  requires a recorded `- [≠]` row + owner approval. None exists (the ledger row at parity-ledger.md:241 is the
  porter's claim; `getCommandFolderSearchPaths` itself is still an unported `- [ ]` at parity-ledger.md:724).
- **Verdict:** `load_command_prompt` stays `- [~]` (unproven). **Owner decision required:** either (a) align
  the port to the source precedence (`.archon/commands` > `.archon/commands/defaults` > configuredFolder), or
  (b) record an owner-approved `- [≠]` for the `.claude/`-conventions adaptation.

### 8. `safeSendMessage` — **PASS** (source-semantics verified; no pure-bun oracle — dep-touching)
Never-throws contract, FATAL rethrow, consecutive-UNKNOWN threshold (=3), and the **consecutive-reset**
subtlety all match source. Added two tests proving the load-bearing semantics:
- `safe_send_transient_resets_unknown_tracker` — UNKNOWN→TRANSIENT→UNKNOWN does NOT abort (TRANSIENT
  resets counter; source:627-629).
- `safe_send_fatal_resets_tracker_before_rethrow` — FATAL (non-UNKNOWN) resets the counter then rethrows.

---

## Symbols flipped to `- [x]` (14/15)
`ErrorType`, `FATAL_PATTERNS`, `TRANSIENT_PATTERNS`, `matchesPattern`, `classifyError`,
`formatSubprocessFailure`, `substituteWorkflowVariables`, `buildPromptWithContext`,
`detectCompletionSignal`, `stripCompletionTags`, `isInlineScript`, `detectCreditExhaustion`,
`safeSendMessage`, `SendMessageContext`.

## Symbol left `- [~]` (0/15 — RESOLVED)
~~`loadCommandPrompt`~~ — precedence aligned to ACTUAL source + differentially re-verified
(2026-06-13). Now `- [x]`.

## Rollup
Per the rollup rule (a unit `PASS` requires **every** symbol `- [x]`/`- [≠]`), all 15/15 WF-11 symbols
are `- [x]`, so **UNIT WF-11 is PASS**. The orchestrator may flip the WF-11 ledger row to `- [x]` and
commit-as-verified.

## Code changes made this cycle (in `executor_shared.rs`)
- `substitute_context_vars()` (new) + rewired detection/replacement — zero-width `$CONTEXT` boundary fix.
- `xml_wrapped_signal_match` / `strip_xml_wrapped_signal` + `close_is_backref_of_open()` (new) —
  JS `\1` backreference backtracking fix.
- `utf16_tail()` (new) + both truncation sites — byte→UTF-16 truncation fix.
- 2 new `safe_send_message` tests for the consecutive-UNKNOWN reset semantics.
- **(re-verification cycle) `command_folder_search_paths` rewritten to source order** + tests realigned.

---

## RE-VERIFICATION (2026-06-13) — `loadCommandPrompt` precedence correction

**Trigger:** the porter aligned `loadCommandPrompt`'s search-path precedence to the ACTUAL source
(`archon-paths.ts:183-196` `getCommandFolderSearchPaths` + `executor-shared.ts:259-267` path assembly),
removing the invented `.claude/commands/`, adding `.archon/commands/defaults`, and moving
`configuredFolder` from first → last.

**Method:** DIFFERENTIAL — live bun oracle of `getCommandFolderSearchPaths` over 6 inputs (incl. both
dedup-equals cases + empty string) captured in Archon (transient oracle deleted, Archon `git status`
clean), then the Rust `command_folder_search_paths` run over the identical inputs and diffed. Plus a
source re-read of `executor-shared.ts:259-307` for the home/app-defaults assembly + the 5 failure paths.

### 1. 5-level order EXACTLY matches source — **PASS**
Source (`archon-paths.ts:184` + `executor-shared.ts:264-267`, `:309-353`):
`(1) .archon/commands → (2) .archon/commands/defaults → (3) configuredFolder (appended LAST) →
(4) ~/.archon/commands (getHomeCommandsPath) → (5) bundled/app-defaults`.
Port (`executor_shared.rs:902-918` + `:957-962` + `:1025-1081`): byte-identical order.
- `configuredFolder` is consulted **LAST** among repo paths (port line 914 `paths.push`, after the two
  defaults) — confirmed by `archon_commands_takes_precedence_over_configured_folder` and
  `archon_commands_defaults_beats_configured_folder` (both PASS).
- `.archon/commands/defaults` is **index 1** (port line 905) — confirmed by
  `archon_commands_defaults_is_searched_after_archon_commands` (PASS).
- **No `.claude/commands/`** anywhere (grep over the symbol confirms it only appears in a "must NOT
  include it" doc-comment at line 901). The invented scope is gone.

### 2. Dedup guard matches source — **PASS**
Source `archon-paths.ts:187-192`: append `configuredFolder` only if truthy AND
`!= '.archon/commands'` AND `!= '.archon/commands/defaults'`. Port `executor_shared.rs:909-915`:
`if Some(folder) && !folder.is_empty() && folder != ".archon/commands" && folder != ".archon/commands/defaults"`.
Live differential — all 6 cases identical to bun:
| configuredFolder | bun `getCommandFolderSearchPaths` | Rust `command_folder_search_paths` |
|---|---|---|
| (none) | `[.archon/commands, .archon/commands/defaults]` | identical |
| `custom-cmds` | `[…, custom-cmds]` (appended last) | identical |
| `.archon/commands` | `[.archon/commands, .archon/commands/defaults]` (deduped) | identical |
| `.archon/commands/defaults` | `[.archon/commands, .archon/commands/defaults]` (deduped) | identical |
| `""` | `[.archon/commands, .archon/commands/defaults]` (falsy → skip) | identical |
| `a/b` | `[…, a/b]` | identical |
New durable regression test `configured_folder_dedup_matches_source` locks all five rows in.

### 3. Multi-dir winner resolves to source-correct dir — **PASS**
First-match-wins over the ordered dir list (`executor_shared.rs:965-974`, source `:269-272`):
- command in BOTH `.archon/commands` and configuredFolder → `.archon/commands` wins
  (`archon_commands_takes_precedence_over_configured_folder`, asserts `"archon version"`).
- command in BOTH `.archon/commands/defaults` and configuredFolder → defaults wins
  (`archon_commands_defaults_beats_configured_folder`, asserts `"defaults version"`).
- command ONLY in configuredFolder → still found (`configured_folder_is_searched_when_command_not_in_archon_dirs`).
- command ONLY in home → home fallback (`home_commands_used_as_fallback`).
The earlier port test `configured_folder_takes_precedence_over_archon_commands` (which asserted the
OPPOSITE of source) is **removed** — no anti-source assertion remains.

### 4. 5 failure reasons + command-name validation — **PASS (no regression)**
- `InvalidName` (`is_valid_command_name`, port `:882-890` ≡ source `command-validation.ts:5-15`:
  rejects `/`, `\`, `..`, empty, leading `.`) — `invalid_command_name_returns_invalid_name`, `valid_command_names`.
- `NotFound` — `not_found_returns_not_found`.
- `EmptyFile` (whitespace-only `content.trim().is_empty()`) — `empty_file_returns_empty_file_error`.
- `PermissionDenied` (EACCES) — `permission_denied_returns_permission_denied`.
- `ReadError` (ENOENT-between-walk-and-read / other IO) — covered by the `Ok(None)` + `Io` arms (port `:991-1021`).
All 10 command tests PASS; full crate suite **204 passed**, `cargo clippy --all-targets` clean.

### Re-verification verdict
`loadCommandPrompt` → **PASS**. The corrected precedence is byte-for-byte source-faithful (no
intentional `- [≠]` needed — it now MATCHES source, so it is a plain `- [x]`). Flip
`loadCommandPrompt` to `- [x]`. **All 15 WF-11 symbols `- [x]` → UNIT WF-11 PASSES the no-downgrade
gate → flip the unit ledger row to `- [x]`.**
