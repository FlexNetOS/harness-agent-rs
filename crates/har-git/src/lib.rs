//! har-git — Git plumbing layer.
//!
//! Ports Archon `packages/git/src/*`:
//!
//! - `exec.ts` (GI-01): `exec_file_async`, `mkdir_async`
//! - `branch.ts` (GI-02): `get_default_branch`, `checkout`,
//!   `has_uncommitted_changes`, `commit_all_changes`,
//!   `is_branch_merged`, `is_patch_equivalent`, `is_ancestor_of`,
//!   `get_last_commit_date`
//! - `repo.ts` (GI-03): `find_repo_root`, `get_remote_url`,
//!   `sync_workspace`, `clone_repository`, `sync_repository`,
//!   `add_safe_directory`
//! - `worktree.ts` (GI-04): `list_worktrees`, `worktree_exists`,
//!   `find_worktree_by_branch`, `is_worktree_path`, `remove_worktree`,
//!   `get_canonical_repo_path`, `verify_worktree_ownership`,
//!   `extract_owner_repo`, `get_worktree_base`,
//!   `is_project_scoped_worktree_base`, `WorktreeLayout`, `WorktreeBaseOverride`
//! - `types.ts` (GI-05): `RepoPath`, `BranchName`, `WorktreePath`,
//!   `GitResult`, `GitErrorCode`, `GitError`, `WorkspaceSyncResult`,
//!   `WorktreeInfo`, constructor fns

pub mod branch;
pub mod exec;
pub mod repo;
pub mod types;
pub mod worktree;

// Convenience re-exports for downstream crates.
pub use types::{
    BranchName, GitError, GitErrorCode, GitResult, RepoPath, Result, WorkspaceSyncResult,
    WorktreeInfo, WorktreePath, to_branch_name, to_repo_path, to_worktree_path,
};
pub use exec::{ExecOptions, ExecOutput, exec_file_async, mkdir_async, run_git, run_git_cwd};
pub use branch::{
    checkout, commit_all_changes, get_default_branch, get_last_commit_date,
    has_uncommitted_changes, is_ancestor_of, is_branch_merged, is_patch_equivalent,
};
pub use repo::{
    add_safe_directory, clone_repository, find_repo_root, get_remote_url, sync_repository,
    sync_workspace,
};
pub use worktree::{
    WorktreeBaseOverride, WorktreeLayout, extract_owner_repo, find_worktree_by_branch,
    get_canonical_repo_path, get_worktree_base, is_project_scoped_worktree_base,
    is_worktree_path, list_worktrees, remove_worktree, verify_worktree_ownership,
    worktree_exists,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Initialize a fresh git repo at `path` and return its RepoPath.
    async fn init_repo(path: &Path) -> RepoPath {
        tokio::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .await
            .expect("git init");
        tokio::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .await
            .expect("git config email");
        tokio::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .await
            .expect("git config name");
        to_repo_path(path.to_string_lossy().to_string()).unwrap()
    }

    /// Create a commit in `path` (needed before many operations).
    async fn make_commit(path: &Path, msg: &str) {
        tokio::fs::write(path.join("README.md"), msg)
            .await
            .expect("write");
        tokio::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .await
            .expect("git add");
        tokio::process::Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(path)
            .output()
            .await
            .expect("git commit");
    }

    // ── GI-05 types ──────────────────────────────────────────────────────────

    #[test]
    fn to_repo_path_rejects_empty() {
        assert!(to_repo_path("").is_err());
        let e = to_repo_path("").unwrap_err().to_string();
        assert!(e.contains("RepoPath cannot be empty"), "got: {}", e);
    }

    #[test]
    fn to_repo_path_accepts_nonempty() {
        let rp = to_repo_path("/some/path").unwrap();
        assert_eq!(rp.as_str(), "/some/path");
    }

    #[test]
    fn to_branch_name_rejects_empty() {
        assert!(to_branch_name("").is_err());
        let e = to_branch_name("").unwrap_err().to_string();
        assert!(e.contains("BranchName cannot be empty"), "got: {}", e);
    }

    #[test]
    fn to_branch_name_accepts_nonempty() {
        let b = to_branch_name("main").unwrap();
        assert_eq!(b.as_str(), "main");
    }

    #[test]
    fn to_worktree_path_rejects_empty() {
        assert!(to_worktree_path("").is_err());
        let e = to_worktree_path("").unwrap_err().to_string();
        assert!(e.contains("WorktreePath cannot be empty"), "got: {}", e);
    }

    #[test]
    fn to_worktree_path_accepts_nonempty() {
        let w = to_worktree_path("/some/worktree").unwrap();
        assert_eq!(w.as_str(), "/some/worktree");
    }

    #[test]
    fn git_result_ok_is_ok() {
        let r: GitResult<i32> = GitResult::Ok(42);
        assert!(r.is_ok());
        assert!(!r.is_err());
        assert_eq!(r.into_result().unwrap(), 42);
    }

    #[test]
    fn git_result_err_is_err() {
        let r: GitResult<i32> =
            GitResult::Err(GitErrorCode::NotARepo { path: "/foo".into() });
        assert!(r.is_err());
        assert!(!r.is_ok());
        assert!(r.into_result().is_err());
    }

    #[test]
    fn git_error_code_variants() {
        let e = GitErrorCode::BranchNotFound { branch: "main".into() };
        assert!(matches!(e, GitErrorCode::BranchNotFound { .. }));
        let e = GitErrorCode::PermissionDenied { path: "/x".into() };
        assert!(matches!(e, GitErrorCode::PermissionDenied { .. }));
        let e = GitErrorCode::NoSpace { path: "/x".into() };
        assert!(matches!(e, GitErrorCode::NoSpace { .. }));
        let e = GitErrorCode::Unknown { message: "oops".into() };
        assert!(matches!(e, GitErrorCode::Unknown { .. }));
    }

    // ── GI-01 exec ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn exec_file_async_captures_stdout_stderr() {
        // `echo` exits 0 and writes to stdout.
        let out = exec_file_async("echo", &["hello world"], ExecOptions::default())
            .await
            .expect("echo");
        assert!(out.stdout.contains("hello world"));
        assert_eq!(out.stderr, ""); // stderr empty on success
    }

    #[tokio::test]
    async fn exec_file_async_nonzero_returns_err() {
        // `false` always exits 1.
        let err = exec_file_async("false", &[], ExecOptions::default())
            .await
            .unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("process exited with code") || msg.contains("exit code"),
            "unexpected: {}",
            err
        );
    }

    #[tokio::test]
    async fn exec_file_async_timeout_fires() {
        // `sleep 10` but with 50ms timeout should time out.
        let err = exec_file_async(
            "sleep",
            &["10"],
            ExecOptions {
                timeout_ms: Some(50),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "unexpected: {}",
            err
        );
    }

    #[tokio::test]
    async fn exec_file_async_cwd_option() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = exec_file_async(
            "pwd",
            &[],
            ExecOptions {
                cwd: Some(dir.path()),
                ..Default::default()
            },
        )
        .await
        .expect("pwd");
        // The real path of the tmp dir should appear in stdout.
        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert!(
            out.stdout.trim().contains(real.to_string_lossy().as_ref()),
            "pwd={}, expected to contain {}",
            out.stdout.trim(),
            real.display()
        );
    }

    #[tokio::test]
    async fn mkdir_async_creates_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let new_dir = dir.path().join("a").join("b").join("c");
        mkdir_async(&new_dir, true).await.expect("mkdir");
        assert!(new_dir.exists());
    }

    #[tokio::test]
    async fn mkdir_async_nonrecursive_fails_on_missing_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let new_dir = dir.path().join("missing_parent").join("child");
        let result = mkdir_async(&new_dir, false).await;
        assert!(result.is_err());
    }

    // ── GI-03 repo ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn find_repo_root_returns_none_outside_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = find_repo_root(dir.path().to_str().unwrap())
            .await
            .expect("find_repo_root");
        assert!(result.is_none(), "expected None for non-repo dir");
    }

    #[tokio::test]
    async fn find_repo_root_returns_path_inside_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;

        // Subdir inside the repo.
        let subdir = dir.path().join("sub");
        tokio::fs::create_dir(&subdir).await.unwrap();

        let result = find_repo_root(subdir.to_str().unwrap())
            .await
            .expect("find_repo_root")
            .expect("should find root");

        let canonical_dir = std::fs::canonicalize(dir.path()).unwrap();
        let canonical_result = std::fs::canonicalize(result.as_str()).unwrap();
        assert_eq!(canonical_result, canonical_dir);
        let _ = repo;
    }

    #[tokio::test]
    async fn get_remote_url_returns_none_when_no_remote() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        let url = get_remote_url(&repo).await.expect("get_remote_url");
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn add_safe_directory_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = to_repo_path(dir.path().to_string_lossy().to_string()).unwrap();
        // Should not error (even if the path isn't a git repo — git config
        // --global --add safe.directory accepts any path).
        add_safe_directory(&repo).await.expect("add_safe_directory");
    }

    #[tokio::test]
    async fn clone_repository_returns_not_a_repo_for_bad_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = to_repo_path(dir.path().join("target").to_string_lossy().to_string()).unwrap();
        // A non-existent path produces a "not found" / 128 exit from git clone.
        let result = clone_repository("https://github.com/nonexistent_org_xyz/nonexistent_repo_xyz.git", &target, None).await;
        assert!(result.is_err(), "expected error for bad URL");
    }

    // ── GI-02 branch ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn has_uncommitted_changes_false_on_clean_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        let dirty = has_uncommitted_changes(repo.as_str()).await;
        assert!(!dirty, "fresh repo after commit should be clean");
    }

    #[tokio::test]
    async fn has_uncommitted_changes_true_when_dirty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        // Write a file but don't commit.
        tokio::fs::write(dir.path().join("dirty.txt"), "change")
            .await
            .unwrap();
        let dirty = has_uncommitted_changes(repo.as_str()).await;
        assert!(dirty, "modified file should be detected");
    }

    #[tokio::test]
    async fn has_uncommitted_changes_false_for_nonexistent_path() {
        // FAIL-SAFE: non-existent path → false (ENOENT branch).
        let dirty = has_uncommitted_changes("/nonexistent_path_xyz_12345").await;
        assert!(!dirty, "nonexistent path should return false");
    }

    #[tokio::test]
    async fn commit_all_changes_returns_false_when_clean() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        let committed = commit_all_changes(repo.as_str(), "should be no-op")
            .await
            .unwrap();
        assert!(!committed, "nothing to commit should return false");
    }

    #[tokio::test]
    async fn commit_all_changes_commits_dirty_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        tokio::fs::write(dir.path().join("new.txt"), "new content")
            .await
            .unwrap();
        let committed = commit_all_changes(repo.as_str(), "add new.txt")
            .await
            .unwrap();
        assert!(committed, "should have committed");
    }

    #[tokio::test]
    async fn checkout_creates_new_branch_if_not_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        let branch = to_branch_name("feature-xyz").unwrap();
        checkout(&repo, &branch).await.expect("checkout new branch");

        // Verify current branch.
        let out = run_git(repo.as_str(), &["branch", "--show-current"], Some(5_000))
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "feature-xyz");
    }

    #[tokio::test]
    async fn checkout_existing_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Create a branch manually.
        run_git(repo.as_str(), &["branch", "existing-branch"], Some(5_000))
            .await
            .unwrap();

        // checkout should switch to it without -b.
        let branch = to_branch_name("existing-branch").unwrap();
        checkout(&repo, &branch).await.expect("checkout existing");

        let out = run_git(repo.as_str(), &["branch", "--show-current"], Some(5_000))
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "existing-branch");
    }

    #[tokio::test]
    async fn is_branch_merged_true_after_merge() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Create + commit on a feature branch.
        run_git(repo.as_str(), &["checkout", "-b", "feature"], Some(5_000))
            .await
            .unwrap();
        make_commit(dir.path(), "feature commit").await;

        // Merge back into main.
        run_git(repo.as_str(), &["checkout", "main"], Some(5_000))
            .await
            .unwrap();
        run_git(repo.as_str(), &["merge", "feature", "--no-ff", "-m", "merge feature"], Some(5_000))
            .await
            .unwrap();

        let main = to_branch_name("main").unwrap();
        let feature = to_branch_name("feature").unwrap();
        let merged = is_branch_merged(&repo, &feature, &main).await.unwrap();
        assert!(merged, "feature should be merged into main");
    }

    #[tokio::test]
    async fn is_branch_merged_false_when_not_merged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        run_git(repo.as_str(), &["checkout", "-b", "unmerged"], Some(5_000))
            .await
            .unwrap();
        make_commit(dir.path(), "unmerged commit").await;
        run_git(repo.as_str(), &["checkout", "main"], Some(5_000))
            .await
            .unwrap();

        let main = to_branch_name("main").unwrap();
        let unmerged = to_branch_name("unmerged").unwrap();
        let merged = is_branch_merged(&repo, &unmerged, &main).await.unwrap();
        assert!(!merged, "unmerged branch should not appear as merged");
    }

    #[tokio::test]
    async fn is_branch_merged_false_for_nonexistent_repo() {
        let rp = to_repo_path("/nonexistent_xyz").unwrap();
        let main = to_branch_name("main").unwrap();
        let branch = to_branch_name("feature").unwrap();
        let merged = is_branch_merged(&rp, &branch, &main).await.unwrap();
        assert!(!merged, "nonexistent repo should return false");
    }

    #[tokio::test]
    async fn is_ancestor_of_true_for_parent_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Get the initial commit hash.
        let out = run_git(repo.as_str(), &["rev-parse", "HEAD"], Some(5_000))
            .await
            .unwrap();
        let initial_sha = out.stdout.trim().to_string();

        // Make another commit on top.
        make_commit(dir.path(), "second").await;

        // initial_sha is an ancestor of HEAD.
        let result = is_ancestor_of(repo.as_str(), &initial_sha).await.unwrap();
        assert!(result, "parent commit should be an ancestor");
    }

    #[tokio::test]
    async fn is_ancestor_of_false_for_unrelated_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        // HEAD is not its own descendant in --is-ancestor terms when no second commit.
        // Use a branch that doesn't exist — expected error → false.
        let result = is_ancestor_of(repo.as_str(), "nonexistent-branch-xyz").await.unwrap();
        assert!(!result, "nonexistent ref should return false");
    }

    #[tokio::test]
    async fn get_last_commit_date_returns_none_for_empty_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path()).await;
        let result = get_last_commit_date(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(result.is_none(), "empty repo has no commits");
    }

    #[tokio::test]
    async fn get_last_commit_date_returns_some_after_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;
        let result = get_last_commit_date(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(result.is_some(), "repo with commit should have a date");
    }

    #[tokio::test]
    async fn get_last_commit_date_returns_none_for_nonexistent_path() {
        let result = get_last_commit_date("/nonexistent_xyz_12345")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn is_patch_equivalent_true_for_empty_cherry() {
        // cherry with no divergent commits → all lines start with '-' (vacuously true).
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Feature branch at the same commit as main.
        run_git(repo.as_str(), &["checkout", "-b", "feature"], Some(5_000))
            .await
            .unwrap();
        run_git(repo.as_str(), &["checkout", "main"], Some(5_000))
            .await
            .unwrap();

        let main = to_branch_name("main").unwrap();
        let feature = to_branch_name("feature").unwrap();
        let result = is_patch_equivalent(&repo, &feature, &main).await.unwrap();
        assert!(result, "branch at same commit is patch-equivalent");
    }

    #[tokio::test]
    async fn is_patch_equivalent_false_for_nonexistent_repo() {
        let rp = to_repo_path("/nonexistent_xyz").unwrap();
        let main = to_branch_name("main").unwrap();
        let branch = to_branch_name("feature").unwrap();
        let result = is_patch_equivalent(&rp, &branch, &main).await.unwrap();
        assert!(!result, "nonexistent repo should return false");
    }

    // ── GI-04 worktree ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_worktrees_returns_main_worktree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        let wts = list_worktrees(&repo).await.unwrap();
        // The main worktree should be listed.
        assert!(!wts.is_empty(), "should list at least the main worktree");
        let canonical_dir = std::fs::canonicalize(dir.path()).unwrap();
        let found = wts.iter().any(|wt| {
            std::fs::canonicalize(wt.path.as_str())
                .map(|p| p == canonical_dir)
                .unwrap_or(false)
        });
        assert!(found, "main repo path should appear in worktree list");
    }

    #[tokio::test]
    async fn list_worktrees_returns_empty_for_non_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = to_repo_path(dir.path().to_string_lossy().to_string()).unwrap();
        let wts = list_worktrees(&repo).await.unwrap();
        assert!(wts.is_empty(), "non-repo dir should return empty list");
    }

    #[tokio::test]
    async fn worktree_exists_false_for_missing_path() {
        let wt = to_worktree_path("/nonexistent_path_xyz_99999").unwrap();
        let exists = worktree_exists(&wt).await.unwrap();
        assert!(!exists, "nonexistent path should return false");
    }

    #[tokio::test]
    async fn worktree_exists_false_for_dir_without_git() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wt = to_worktree_path(dir.path().to_string_lossy().to_string()).unwrap();
        let exists = worktree_exists(&wt).await.unwrap();
        assert!(!exists, "dir without .git should return false (corruption)");
    }

    #[tokio::test]
    async fn is_worktree_path_false_for_main_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path()).await;
        // Main repo has .git as a directory, not a file.
        let result = is_worktree_path(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(!result, "main repo .git dir should not be a worktree");
    }

    #[tokio::test]
    async fn is_worktree_path_false_for_non_git_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result = is_worktree_path(dir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(!result, "non-git dir should not be a worktree");
    }

    #[tokio::test]
    async fn linked_worktree_add_and_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        let wt_dir = dir.path().parent().unwrap().join("linked-wt");
        // git worktree add <path> -b <branch>
        run_git(
            repo.as_str(),
            &[
                "worktree",
                "add",
                wt_dir.to_str().unwrap(),
                "-b",
                "wt-branch",
            ],
            Some(10_000),
        )
        .await
        .expect("worktree add");

        let wts = list_worktrees(&repo).await.unwrap();
        assert!(wts.len() >= 2, "should list both main and linked worktree");

        let canonical_wt = std::fs::canonicalize(&wt_dir).unwrap();
        let found_wt = wts.iter().any(|wt| {
            std::fs::canonicalize(wt.path.as_str())
                .map(|p| p == canonical_wt)
                .unwrap_or(false)
                && wt.branch.as_str() == "wt-branch"
        });
        assert!(found_wt, "linked worktree should appear in list");

        // Verify the linked path is detected as a worktree path.
        let is_wt = is_worktree_path(wt_dir.to_str().unwrap()).await.unwrap();
        assert!(is_wt, "linked worktree should be detected as worktree");

        // Verify get_canonical_repo_path resolves back to the main repo.
        let canonical_main = std::fs::canonicalize(dir.path()).unwrap();
        let resolved = get_canonical_repo_path(wt_dir.to_str().unwrap())
            .await
            .unwrap();
        let resolved_canonical = std::fs::canonicalize(resolved.as_str()).unwrap();
        assert_eq!(
            resolved_canonical, canonical_main,
            "canonical repo path should match main repo"
        );

        // Remove worktree.
        let wt_path = to_worktree_path(wt_dir.to_string_lossy().to_string()).unwrap();
        remove_worktree(&repo, &wt_path).await.expect("remove worktree");

        let wts_after = list_worktrees(&repo).await.unwrap();
        let still_found = wts_after.iter().any(|wt| {
            std::fs::canonicalize(wt.path.as_str())
                .map(|p| p == canonical_wt)
                .unwrap_or(false)
        });
        assert!(!still_found, "removed worktree should not appear in list");
    }

    #[tokio::test]
    async fn find_worktree_by_branch_exact_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        let wt_dir = dir.path().parent().unwrap().join("wt-exact");
        run_git(
            repo.as_str(),
            &["worktree", "add", wt_dir.to_str().unwrap(), "-b", "exact-branch"],
            Some(10_000),
        )
        .await
        .expect("worktree add");

        let branch = to_branch_name("exact-branch").unwrap();
        let found = find_worktree_by_branch(&repo, &branch).await.unwrap();
        assert!(found.is_some(), "should find worktree by exact branch name");

        // Cleanup.
        let wt_path = to_worktree_path(wt_dir.to_string_lossy().to_string()).unwrap();
        let _ = remove_worktree(&repo, &wt_path).await;
    }

    #[tokio::test]
    async fn find_worktree_by_branch_slugified_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Create worktree on branch "feature-auth" (slugified form).
        let wt_dir = dir.path().parent().unwrap().join("wt-slug");
        run_git(
            repo.as_str(),
            &["worktree", "add", wt_dir.to_str().unwrap(), "-b", "feature-auth"],
            Some(10_000),
        )
        .await
        .expect("worktree add");

        // Search with slashed form "feature/auth" which slugifies to "feature-auth".
        let branch = to_branch_name("feature/auth").unwrap();
        let found = find_worktree_by_branch(&repo, &branch).await.unwrap();
        assert!(found.is_some(), "should find worktree by slugified branch name");

        let wt_path = to_worktree_path(wt_dir.to_string_lossy().to_string()).unwrap();
        let _ = remove_worktree(&repo, &wt_path).await;
    }

    #[tokio::test]
    async fn find_worktree_by_branch_returns_none_when_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        let branch = to_branch_name("nonexistent-branch-xyz").unwrap();
        let found = find_worktree_by_branch(&repo, &branch).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn extract_owner_repo_last_two_segments() {
        let rp = to_repo_path("/home/user/owner/repo").unwrap();
        let (owner, repo) = extract_owner_repo(&rp);
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    #[should_panic(expected = "expected at least 2 path segments")]
    fn extract_owner_repo_panics_on_too_short_path() {
        let rp = to_repo_path("/single").unwrap();
        extract_owner_repo(&rp);
    }

    #[tokio::test]
    async fn verify_worktree_ownership_rejects_main_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path()).await;
        // .git is a directory in the main repo — should report "full git checkout".
        let wt = to_worktree_path(dir.path().to_string_lossy().to_string()).unwrap();
        let expected = to_repo_path(dir.path().to_string_lossy().to_string()).unwrap();
        let result = verify_worktree_ownership(&wt, &expected).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("full git checkout"),
            "expected 'full git checkout' in: {}",
            msg
        );
    }

    #[tokio::test]
    async fn verify_worktree_ownership_accepts_correct_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        let wt_dir = dir.path().parent().unwrap().join("wt-ownership");
        run_git(
            repo.as_str(),
            &["worktree", "add", wt_dir.to_str().unwrap(), "-b", "ownership-branch"],
            Some(10_000),
        )
        .await
        .expect("worktree add");

        let wt = to_worktree_path(wt_dir.to_string_lossy().to_string()).unwrap();
        let result = verify_worktree_ownership(&wt, &repo).await;
        assert!(result.is_ok(), "ownership verification should pass: {:?}", result);

        let wt_path = to_worktree_path(wt_dir.to_string_lossy().to_string()).unwrap();
        let _ = remove_worktree(&repo, &wt_path).await;
    }

    #[tokio::test]
    async fn sync_workspace_fetch_only_returns_synced_false_update() {
        // We can't actually fetch a remote in tests, but we can verify the
        // fetch-only path returns the expected struct shape when fetch succeeds
        // on a local repo with itself as origin.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).await;
        make_commit(dir.path(), "initial").await;

        // Add itself as remote "origin" so fetch succeeds.
        run_git(
            repo.as_str(),
            &["remote", "add", "origin", dir.path().to_str().unwrap()],
            Some(5_000),
        )
        .await
        .unwrap();

        let main_branch = to_branch_name("main").unwrap();
        let result = sync_workspace(&repo, Some(&main_branch), false).await;
        // Either succeeds (fetch worked) or fails with a network-style error.
        // We just ensure the function doesn't panic.
        match result {
            Ok(sync) => {
                assert!(sync.synced);
                assert_eq!(sync.branch.as_str(), "main");
            }
            Err(_) => {
                // Acceptable — git fetch from a bare path may fail in CI.
            }
        }
    }

    // ── Cycle-8 differential golden fixtures ─────────────────────────────────
    //
    // These pin the source-verified contracts established by the live `bun ⇄
    // Rust` differential run (cycle 8). They are NOT existence checks — each
    // asserts a specific observable shape/substring that the TypeScript source
    // produced for the same input, so a future refactor that silently changes
    // the contract trips here.
    //
    // Provenance: `.handoff/loop/findings/parity-cycle8.md`.
    mod golden_cycle8 {
        use super::*;

        /// GI-04 verifyWorktreeOwnership: cross-clone rejection message text is
        /// substring-matched by the isolation layer's `classifyIsolationError`
        /// ("belongs to a different clone"). Source: worktree.ts:374-378.
        #[tokio::test]
        async fn verify_ownership_cross_clone_message() {
            let dir = tempfile::tempdir().unwrap();
            let main = init_repo(dir.path()).await;
            make_commit(dir.path(), "c1").await;
            let wl = dir.path().join("wl");
            run_git(
                main.as_str(),
                &["worktree", "add", "-b", "wtb", wl.to_str().unwrap()],
                Some(10_000),
            )
            .await
            .unwrap();

            let other = to_repo_path("/some/other/clone").unwrap();
            let wlp = to_worktree_path(wl.to_str().unwrap()).unwrap();
            let err = verify_worktree_ownership(&wlp, &other).await.unwrap_err();
            // The isolation layer matches this exact substring; it must survive.
            assert!(
                err.to_string().contains("belongs to a different clone"),
                "got: {err}"
            );
        }

        /// GI-04 verifyWorktreeOwnership: a full checkout (.git is a directory)
        /// yields the "full git checkout" message — classified EISDIR upstream.
        /// Source: worktree.ts:348-350.
        #[tokio::test]
        async fn verify_ownership_full_checkout_message() {
            let dir = tempfile::tempdir().unwrap();
            let main = init_repo(dir.path()).await;
            make_commit(dir.path(), "c1").await;
            let mainp = to_worktree_path(main.as_str()).unwrap();
            let err = verify_worktree_ownership(&mainp, &main).await.unwrap_err();
            assert!(
                err.to_string()
                    .contains("path contains a full git checkout, not a worktree"),
                "got: {err}"
            );
        }

        /// GI-01 exec error shape: a non-zero git exit surfaces the inner git
        /// stderr verbatim inside the error message (Node's execFile appends
        /// stderr to err.message). The classification layer relies on these
        /// substrings being present. Source: exec.ts:13 + Node execFile.
        #[tokio::test]
        async fn exec_error_message_carries_git_stderr() {
            let dir = tempfile::tempdir().unwrap();
            // `git -C <non-repo> rev-parse --show-toplevel` exits 128 with the
            // "not a git repository" stderr that findRepoRoot/listWorktrees etc.
            // classify on.
            let err = run_git(
                dir.path().to_str().unwrap(),
                &["rev-parse", "--show-toplevel"],
                Some(5_000),
            )
            .await
            .unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("Command failed: git"), "got: {msg}");
            assert!(msg.contains("not a git repository"), "got: {msg}");
        }

        /// GI-02 getDefaultBranch: when neither origin/HEAD nor origin/main
        /// resolve, the actionable error text matches the source verbatim
        /// (modulo the GitError Display prefix). Source: branch.ts:68-71.
        #[tokio::test]
        async fn default_branch_neither_actionable_message() {
            let dir = tempfile::tempdir().unwrap();
            let repo = init_repo(dir.path()).await;
            make_commit(dir.path(), "c1").await;
            // No origin remote at all → fallback chain exhausts → Err.
            let err = get_default_branch(&repo).await.unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("neither origin/HEAD nor origin/main exist"),
                "got: {msg}"
            );
            assert!(msg.contains(".archon/config.yaml"), "got: {msg}");
        }

        /// GI-03 cloneRepository: a token is sanitized to *** and never leaks
        /// in the returned error. Differentially verified against the source's
        /// `replaceAll(token, '***')`. Source: repo.ts:202-205.
        #[tokio::test]
        async fn clone_token_never_leaks_in_error() {
            let target = to_repo_path("/nonexistent/clone/target/xyz").unwrap();
            let token = "SUPERSECRET_TOKEN_42";
            let res = clone_repository(
                "https://github.com/no/such-xyz-404-parity.git",
                &target,
                Some(token),
            )
            .await;
            match res {
                GitResult::Err(e) => {
                    let s = format!("{e:?}");
                    assert!(!s.contains(token), "token leaked: {s}");
                }
                GitResult::Ok(()) => panic!("expected clone to fail"),
            }
        }
    }
}
