/// PR state lookup via the `gh` CLI.
///
/// Ports `packages/isolation/src/pr-state.ts`.
///
/// ## RESOLVED IS-05 shape (read from source 2026-06-14)
///
/// Source is a single file with:
///   - `PrState` string union: `'MERGED' | 'CLOSED' | 'OPEN' | 'NONE'`
///   - `getPrState(branch, repoPath, cache?) -> Promise<PrState>` — async fn
///
/// Behavior:
///   1. Check `cache?.get(branch)` → return early if present.
///   2. `git -C <repoPath> remote get-url origin` with 10 s timeout.
///      On error → debug log, cache NONE, return NONE.
///   3. If remote URL does not include "github.com" (case-insensitive) →
///      debug log, cache NONE, return NONE.
///   4. `gh pr list --head <branch> --state all --json state --limit 1`
///      with 15 s timeout, cwd = repoPath.
///      Parse JSON: `[{ state?: string }]`
///      If first element's `state` is MERGED/CLOSED/OPEN → result = that value.
///      On any error:
///        - If `code == ENOENT` or message contains "command not found" →
///          debug log "gh not installed"
///        - Else → warn log with err + branch + repoPath + ghStdout
///   5. Cache the result (even on error, result defaults to NONE).
///   6. Return result.
///
/// `gh` is a SOFT dependency — if unavailable or on a non-GitHub remote,
/// returns `PrState::None` so callers can fall back to git-only signals.
use har_git::exec::{exec_file_async, ExecOptions};
use tracing::{debug, warn};

/// PR lifecycle state as returned by the `gh` CLI.
/// Source: `pr-state.ts:19`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrState {
    Merged,
    Closed,
    Open,
    /// No PR found, `gh` unavailable, or non-GitHub remote.
    None,
}

/// Look up the PR state for a branch in the GitHub remote via the `gh` CLI.
///
/// Ports `getPrState(branch, repoPath, cache?)` from `pr-state.ts:30-91`.
///
/// - `branch`    — the branch whose head PR to look up
/// - `repo_path` — canonical repo path (git `-C` flag)
/// - `cache`     — optional mutable dedup cache for the current cleanup run
///
/// Returns `PrState::None` when no PR is found, `gh` is unavailable, or
/// the remote is not GitHub.
pub async fn get_pr_state(
    branch: &str,
    repo_path: &str,
    cache: Option<&mut std::collections::HashMap<String, PrState>>,
) -> PrState {
    // Check cache first.
    if let Some(ref c) = cache {
        if let Some(&cached) = c.get(branch) {
            return cached;
        }
    }

    // Step 1: get the remote URL (soft check for GitHub).
    let remote_url = match exec_file_async(
        "git",
        &["-C", repo_path, "remote", "get-url", "origin"],
        ExecOptions {
            timeout_ms: Some(10_000),
            ..Default::default()
        },
    )
    .await
    {
        Ok(out) => out.stdout.trim().to_string(),
        Err(e) => {
            debug!(
                err = %e,
                repo_path = %repo_path,
                branch = %branch,
                "isolation.pr_state_remote_lookup_failed"
            );
            if let Some(c) = cache {
                c.insert(branch.to_string(), PrState::None);
            }
            return PrState::None;
        }
    };

    // Step 2: only proceed for GitHub remotes.
    if !remote_url.to_lowercase().contains("github.com") {
        debug!(
            repo_path = %repo_path,
            branch = %branch,
            remote_url = %remote_url,
            "isolation.pr_state_github_only"
        );
        if let Some(c) = cache {
            c.insert(branch.to_string(), PrState::None);
        }
        return PrState::None;
    }

    // Step 3: call `gh pr list`.
    let mut result = PrState::None;
    let mut gh_stdout = String::new();

    match exec_file_async(
        "gh",
        &[
            "pr", "list", "--head", branch, "--state", "all", "--json", "state", "--limit", "1",
        ],
        ExecOptions {
            timeout_ms: Some(15_000),
            cwd: Some(std::path::Path::new(repo_path)),
            ..Default::default()
        },
    )
    .await
    {
        Ok(out) => {
            gh_stdout.clone_from(&out.stdout);
            // Parse `[{ state?: string }]`
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&gh_stdout) {
                if let Some(state_str) = parsed
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.get("state"))
                    .and_then(|s| s.as_str())
                {
                    result = match state_str {
                        "MERGED" => PrState::Merged,
                        "CLOSED" => PrState::Closed,
                        "OPEN" => PrState::Open,
                        _ => PrState::None,
                    };
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            // Detect "gh not installed" via ENOENT-equivalent or "command not found".
            let is_not_installed = msg.contains("No such file or directory")
                || msg.contains("ENOENT")
                || msg.contains("command not found")
                || msg.contains("os error 2"); // ENOENT on Linux

            if is_not_installed {
                debug!(
                    branch = %branch,
                    repo_path = %repo_path,
                    "isolation.pr_state_gh_not_installed"
                );
            } else {
                warn!(
                    err = %e,
                    branch = %branch,
                    repo_path = %repo_path,
                    gh_stdout = %gh_stdout,
                    "isolation.pr_state_lookup_failed"
                );
            }
            // result stays PrState::None
        }
    }

    if let Some(c) = cache {
        c.insert(branch.to_string(), result);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pr_state_none_default() {
        let s = PrState::None;
        assert_eq!(s, PrState::None);
    }

    #[test]
    fn pr_state_variants_distinct() {
        assert_ne!(PrState::Merged, PrState::Closed);
        assert_ne!(PrState::Closed, PrState::Open);
        assert_ne!(PrState::Open, PrState::None);
    }

    /// Cache hit: value in cache is returned without any I/O.
    #[tokio::test]
    async fn get_pr_state_returns_cached_value() {
        let mut cache: HashMap<String, PrState> = HashMap::new();
        cache.insert("cached-branch".to_string(), PrState::Merged);

        let result = get_pr_state("cached-branch", "/nonexistent", Some(&mut cache)).await;
        // Should return the cached value, NOT hit the network.
        assert_eq!(result, PrState::Merged);
    }

    /// Non-GitHub remote: should return None without calling gh.
    /// (We can't easily inject the remote URL without a real git repo, but we
    /// can verify that a nonexistent repo path → None without panic.)
    #[tokio::test]
    async fn get_pr_state_nonexistent_repo_returns_none() {
        let result = get_pr_state("some-branch", "/nonexistent_repo_xyz_99999", None).await;
        assert_eq!(result, PrState::None, "nonexistent repo should return None");
    }

    /// Cache is populated after a lookup (even on failure).
    #[tokio::test]
    async fn get_pr_state_populates_cache_on_failure() {
        let mut cache: HashMap<String, PrState> = HashMap::new();
        get_pr_state("branch-x", "/nonexistent_repo_xyz", Some(&mut cache)).await;
        // After the call, the branch should be in the cache (as None).
        assert!(cache.contains_key("branch-x"));
        assert_eq!(cache["branch-x"], PrState::None);
    }

    /// Second call to same branch hits cache even if the earlier lookup failed.
    #[tokio::test]
    async fn get_pr_state_second_call_hits_cache() {
        let mut cache: HashMap<String, PrState> = HashMap::new();
        // First call — nonexistent repo, should set cache to None.
        let r1 = get_pr_state("b", "/nonexistent", Some(&mut cache)).await;
        assert_eq!(r1, PrState::None);
        // Second call — should return from cache.
        let r2 = get_pr_state("b", "/nonexistent", Some(&mut cache)).await;
        assert_eq!(r2, PrState::None);
    }
}
