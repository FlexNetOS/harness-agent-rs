//! Cycle-10 differential parity fixtures for IS-02 (branch naming + getWorktreePath).
//!
//! These assert the EXACT byte-for-byte branch name produced by
//! `WorktreeProvider::generate_branch_name` for each workflow type, and the
//! `get_worktree_path` precedence, against the values captured from the live
//! bun source oracle (`packages/isolation/src/providers/worktree.ts`).
//!
//! Source oracle output is recorded inline in each assertion (TS == Rust).
//! Branch naming is the highest-risk surface: a wrong scheme = wrong worktree.

use har_isolation::providers::WorktreeProvider;
use har_isolation::types::{IsolationRequest, IsolationRequestBase, WorktreeCreateConfig};

const REPO: &str = "/home/drdave/Desktop/meta/Archon/packages/isolation/__tmp_repo_owner/myrepo";

fn provider() -> WorktreeProvider {
    WorktreeProvider::new(std::sync::Arc::new(|_| Box::pin(async { None })))
}

fn base(codebase_name: Option<&str>) -> IsolationRequestBase {
    IsolationRequestBase {
        codebase_id: "cdb-001".into(),
        codebase_name: codebase_name.map(|s| s.to_string()),
        canonical_repo_path: REPO.into(),
        description: None,
        git_identity: None,
    }
}

fn issue(id: &str) -> IsolationRequest {
    IsolationRequest::Issue {
        base: base(Some("owner/repo")),
        identifier: id.into(),
    }
}
fn review(id: &str) -> IsolationRequest {
    IsolationRequest::Review {
        base: base(Some("owner/repo")),
        identifier: id.into(),
    }
}
fn thread(id: &str) -> IsolationRequest {
    IsolationRequest::Thread {
        base: base(Some("owner/repo")),
        identifier: id.into(),
    }
}
fn task(id: &str) -> IsolationRequest {
    IsolationRequest::Task {
        base: base(Some("owner/repo")),
        identifier: id.into(),
        from_branch: None,
    }
}
fn pr(id: &str, pr_branch: &str, is_fork: bool) -> IsolationRequest {
    IsolationRequest::Pr {
        base: base(Some("owner/repo")),
        identifier: id.into(),
        pr_branch: pr_branch.into(),
        pr_sha: None,
        is_fork_pr: is_fork,
    }
}

// ─── Branch naming: byte-for-byte vs TS oracle ──────────────────────────────

#[test]
fn branch_naming_matches_ts_oracle() {
    let p = provider();
    // (input request, TS-oracle branch string)
    let cases: Vec<(IsolationRequest, &str)> = vec![
        (issue("42"), "archon/issue-42"),
        (issue(""), "archon/issue-"),
        (issue("café-#9"), "archon/issue-café-#9"), // issue does NOT slugify
        (review("77"), "archon/review-77"),
        (pr("5", "feature/My-Branch", false), "feature/My-Branch"), // same-repo: verbatim
        (pr("5", "ignored", true), "archon/pr-5-review"),           // fork: synthetic
        (thread("C123.456"), "archon/thread-ef6545d2"),
        (thread(""), "archon/thread-e3b0c442"),
        (thread("héllo-世界"), "archon/thread-3e4ff432"),
        (task("Add New Feature!!"), "archon/task-add-new-feature"),
        (task("---Foo Bar---"), "archon/task-foo-bar"),
        (
            task(&"a".repeat(80)),
            "archon/task-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        (task("Café Münchën 2024"), "archon/task-caf-m-nch-n-2024"),
        (task("!!!@@@###"), "archon/task-"),
        (task("foo___bar...baz"), "archon/task-foo-bar-baz"),
        (
            task(&("x".repeat(50) + "YZ")),
            "archon/task-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ),
    ];
    for (req, expected) in cases {
        let got = p.generate_branch_name(&req);
        assert_eq!(got, expected, "branch naming mismatch for {req:?}");
    }
}

// ─── getWorktreePath precedence vs TS oracle ────────────────────────────────

#[test]
fn worktree_path_precedence_matches_ts_oracle() {
    let p = provider();
    let req = issue("1");
    let branch = p.generate_branch_name(&req); // "archon/issue-1"

    // default (workspace-scoped) — uses ~/.archon. Assert the suffix that does not
    // depend on $HOME so the test is portable; the owner/repo + branch layout is
    // the load-bearing part.
    let default_path = p.get_worktree_path(&req, &branch, None).unwrap();
    assert!(
        default_path.ends_with("/.archon/workspaces/owner/repo/worktrees/archon/issue-1"),
        "default path: {default_path}"
    );

    // empty override → same as default (TS: empty-after-trim → undefined)
    let empty_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some("".into()),
    };
    let empty_path = p
        .get_worktree_path(&req, &branch, Some(&empty_cfg))
        .unwrap();
    assert_eq!(
        empty_path, default_path,
        "empty override should equal default"
    );

    // repo-local override → <repoRoot>/.worktrees/<branch>
    let rl_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some(".worktrees".into()),
    };
    let rl_path = p.get_worktree_path(&req, &branch, Some(&rl_cfg)).unwrap();
    assert_eq!(
        rl_path,
        format!("{REPO}/.worktrees/archon/issue-1"),
        "repo-local override path"
    );

    // nested repo-local override
    let nested_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some("a/b/c".into()),
    };
    let nested_path = p
        .get_worktree_path(&req, &branch, Some(&nested_cfg))
        .unwrap();
    assert_eq!(nested_path, format!("{REPO}/a/b/c/archon/issue-1"));

    // no codebaseName → owner/repo derived from last 2 path segments
    let req_noname = IsolationRequest::Issue {
        base: base(None),
        identifier: "1".into(),
    };
    let noname_path = p.get_worktree_path(&req_noname, &branch, None).unwrap();
    assert!(
        noname_path
            .ends_with("/.archon/workspaces/__tmp_repo_owner/myrepo/worktrees/archon/issue-1"),
        "no-codebasename path: {noname_path}"
    );
}

// ─── getWorktreePath error precedence vs TS oracle ──────────────────────────

#[test]
fn worktree_path_errors_match_ts_oracle() {
    let p = provider();
    let req = issue("1");
    let branch = p.generate_branch_name(&req);

    // absolute path → Err with exact message
    let abs_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some("/abs/path".into()),
    };
    let abs_err = p
        .get_worktree_path(&req, &branch, Some(&abs_cfg))
        .unwrap_err()
        .to_string();
    assert!(
        abs_err.contains("must be relative to the repo root (got absolute: /abs/path)"),
        "absolute err: {abs_err}"
    );

    // `..` segment → Err
    let dd_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some("../escape".into()),
    };
    let dd_err = p
        .get_worktree_path(&req, &branch, Some(&dd_cfg))
        .unwrap_err()
        .to_string();
    assert!(
        dd_err.contains("must stay within the repo (got: ../escape)"),
        "dotdot err: {dd_err}"
    );

    // nested `..` that escapes after normalization → Err
    let nd_cfg = WorktreeCreateConfig {
        base_branch: None,
        copy_files: None,
        init_submodules: None,
        path: Some("a/../../escape".into()),
    };
    let nd_err = p.get_worktree_path(&req, &branch, Some(&nd_cfg));
    assert!(nd_err.is_err(), "nested dotdot must error: {nd_err:?}");
}
