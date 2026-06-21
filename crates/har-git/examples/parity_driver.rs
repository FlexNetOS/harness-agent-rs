//! TRANSIENT cycle-8 parity differential driver.
//! Invoked: cargo run -q --example parity_driver -- <op> <jsonArgs>
//! Prints a single JSON line mirroring the TS oracle's shape.
//! Not a durable artifact — fixtures are committed separately.

use har_git as g;
use serde_json::{json, Value};

fn out_ok(v: Value) {
    println!("{}", json!({ "kind": "ok", "value": v }));
}
fn out_throw(msg: String) {
    // Rust has no err.code analog for git-process errors; report null like JS would
    // for git CLI errors (those carry message text, not an errno).
    println!(
        "{}",
        json!({ "kind": "throw", "message": msg, "code": Value::Null })
    );
}

fn r(s: &str) -> g::RepoPath {
    g::to_repo_path(s).expect("repo path")
}
fn b(s: &str) -> g::BranchName {
    g::to_branch_name(s).expect("branch")
}
fn w(s: &str) -> g::WorktreePath {
    g::to_worktree_path(s).expect("wt path")
}

fn git_result_to_json<T, F: Fn(&T) -> Value>(res: g::GitResult<T>, f: F) -> Value {
    match res {
        g::GitResult::Ok(v) => json!({ "ok": true, "value": f(&v) }),
        g::GitResult::Err(e) => {
            let err = match e {
                g::GitErrorCode::NotARepo { path } => json!({ "code": "not_a_repo", "path": path }),
                g::GitErrorCode::PermissionDenied { path } => {
                    json!({ "code": "permission_denied", "path": path })
                }
                g::GitErrorCode::BranchNotFound { branch } => {
                    json!({ "code": "branch_not_found", "branch": branch })
                }
                g::GitErrorCode::NoSpace { path } => json!({ "code": "no_space", "path": path }),
                g::GitErrorCode::Unknown { message } => {
                    json!({ "code": "unknown", "message": message })
                }
            };
            json!({ "ok": false, "error": err })
        }
    }
}

#[tokio::main]
async fn main() {
    let op = std::env::args().nth(1).unwrap_or_default();
    let raw = std::env::args().nth(2).unwrap_or_else(|| "{}".into());
    let a: Value = serde_json::from_str(&raw).expect("json args");
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();

    match op.as_str() {
        // ── branch ──
        "getDefaultBranch" => match g::get_default_branch(&r(&s("repo"))).await {
            Ok(v) => out_ok(json!(v.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        "checkout" => match g::checkout(&r(&s("repo")), &b(&s("branch"))).await {
            Ok(()) => out_ok(json!("void")),
            Err(e) => out_throw(e.to_string()),
        },
        "hasUncommittedChanges" => out_ok(json!(g::has_uncommitted_changes(&s("path")).await)),
        "commitAllChanges" => match g::commit_all_changes(&s("path"), &s("message")).await {
            Ok(v) => out_ok(json!(v)),
            Err(e) => out_throw(e.to_string()),
        },
        "isBranchMerged" => {
            match g::is_branch_merged(&r(&s("repo")), &b(&s("branch")), &b(&s("main"))).await {
                Ok(v) => out_ok(json!(v)),
                Err(e) => out_throw(e.to_string()),
            }
        }
        "isPatchEquivalent" => {
            match g::is_patch_equivalent(&r(&s("repo")), &b(&s("branch")), &b(&s("base"))).await {
                Ok(v) => out_ok(json!(v)),
                Err(e) => out_throw(e.to_string()),
            }
        }
        "isAncestorOf" => match g::is_ancestor_of(&s("path"), &s("ref")).await {
            Ok(v) => out_ok(json!(v)),
            Err(e) => out_throw(e.to_string()),
        },
        "getLastCommitDate" => match g::get_last_commit_date(&s("path")).await {
            Ok(None) => out_ok(Value::Null),
            // Emit ISO-8601 UTC like JS `.toISOString()` for cross-impl diff.
            Ok(Some(dt)) => out_ok(json!(
                dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            )),
            Err(e) => out_throw(e.to_string()),
        },
        // ── repo ──
        "findRepoRoot" => match g::find_repo_root(&s("path")).await {
            Ok(None) => out_ok(Value::Null),
            Ok(Some(rp)) => out_ok(json!(rp.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        "getRemoteUrl" => match g::get_remote_url(&r(&s("repo"))).await {
            Ok(None) => out_ok(Value::Null),
            Ok(Some(u)) => out_ok(json!(u)),
            Err(e) => out_throw(e.to_string()),
        },
        "syncWorkspace" => {
            let base = a.get("base").and_then(|v| v.as_str()).map(b);
            let reset = a
                .get("resetAfterFetch")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            match g::sync_workspace(&r(&s("repo")), base.as_ref(), reset).await {
                Ok(res) => out_ok(json!({
                    "branch": res.branch.as_str(),
                    "synced": res.synced,
                    "previousHead": res.previous_head,
                    "newHead": res.new_head,
                    "updated": res.updated,
                })),
                Err(e) => out_throw(e.to_string()),
            }
        }
        "cloneRepository" => {
            let token = a.get("token").and_then(|v| v.as_str());
            let res = g::clone_repository(&s("url"), &r(&s("target")), token).await;
            out_ok(git_result_to_json(res, |_| json!(null)));
        }
        "syncRepository" => {
            let res = g::sync_repository(&r(&s("repo")), &b(&s("branch"))).await;
            out_ok(git_result_to_json(res, |_| json!(null)));
        }
        // ── worktree ──
        "listWorktrees" => match g::list_worktrees(&r(&s("repo"))).await {
            Ok(list) => out_ok(json!(list
                .iter()
                .map(|wi| json!({ "path": wi.path.as_str(), "branch": wi.branch.as_str() }))
                .collect::<Vec<_>>())),
            Err(e) => out_throw(e.to_string()),
        },
        "worktreeExists" => match g::worktree_exists(&w(&s("path"))).await {
            Ok(v) => out_ok(json!(v)),
            Err(e) => out_throw(e.to_string()),
        },
        "findWorktreeByBranch" => {
            match g::find_worktree_by_branch(&r(&s("repo")), &b(&s("branch"))).await {
                Ok(None) => out_ok(Value::Null),
                Ok(Some(p)) => out_ok(json!(p.as_str())),
                Err(e) => out_throw(e.to_string()),
            }
        }
        "isWorktreePath" => match g::is_worktree_path(&s("path")).await {
            Ok(v) => out_ok(json!(v)),
            Err(e) => out_throw(e.to_string()),
        },
        "removeWorktree" => match g::remove_worktree(&r(&s("repo")), &w(&s("path"))).await {
            Ok(()) => out_ok(json!("void")),
            Err(e) => out_throw(e.to_string()),
        },
        "getCanonicalRepoPath" => match g::get_canonical_repo_path(&s("path")).await {
            Ok(rp) => out_ok(json!(rp.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        "verifyWorktreeOwnership" => {
            match g::verify_worktree_ownership(&w(&s("path")), &r(&s("repo"))).await {
                Ok(()) => out_ok(json!("void")),
                Err(e) => out_throw(e.to_string()),
            }
        }
        "extractOwnerRepo" => {
            // Source throws on <2 segments; Rust panics. Catch the panic to
            // emit a throw record for differential comparison.
            let repo = s("repo");
            let res = std::panic::catch_unwind(|| g::extract_owner_repo(&r(&repo)));
            match res {
                Ok((owner, rp)) => out_ok(json!({ "owner": owner, "repo": rp })),
                Err(_) => out_throw(format!(
                    "Cannot extract owner/repo from path \"{}\": expected at least 2 path segments",
                    repo
                )),
            }
        }
        // ── types ──
        "toRepoPath" => match g::to_repo_path(s("s")) {
            Ok(v) => out_ok(json!(v.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        "toBranchName" => match g::to_branch_name(s("s")) {
            Ok(v) => out_ok(json!(v.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        "toWorktreePath" => match g::to_worktree_path(s("s")) {
            Ok(v) => out_ok(json!(v.as_str())),
            Err(e) => out_throw(e.to_string()),
        },
        other => println!(
            "{}",
            json!({ "kind": "error", "message": format!("unknown op {other}") })
        ),
    }
}
