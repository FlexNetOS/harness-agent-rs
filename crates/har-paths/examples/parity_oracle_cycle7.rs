//! TRANSIENT differential parity harness for cycle 7 (PA-01/PA-06/PA-07).
//! Mirrors `Archon/__parity_oracle_cycle7.ts`. One fixture per process (env isolation).
//! Run: `cargo run -q --example parity_oracle_cycle7 -- <fixture> [cwd]`
//! Emits one JSON line to stdout. DELETE after differential run if not kept as a fixture.

use std::path::{Path, PathBuf};

use har_paths::archon_paths::{
    expand_tilde, get_archon_config_path, get_archon_env_path, get_archon_home,
    get_archon_workspaces_path, get_archon_worktrees_path, get_command_folder_search_paths,
    get_home_commands_path, get_home_scripts_path, get_home_workflows_path,
    get_legacy_home_workflows_path, get_project_artifacts_path, get_project_logs_path,
    get_project_root, get_project_source_path, get_project_worktrees_path,
    get_repo_archon_env_path, get_run_artifacts_path, get_run_log_path, get_web_dist_dir,
    get_workflow_folder_search_paths, is_docker, parse_owner_repo, resolve_project_root_from_cwd,
    ArchonPathError,
};
use har_paths::env_loader::load_archon_env;
use har_paths::strip_cwd_env::strip_cwd_env;

// ─── Tiny JSON emitters (no serde dep) ─────────────────────────────────────────

fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `Ok(path)` → `{"ok":true,"value":"<path>"}`; `Err` → `{"ok":false,"error":"<msg>"}`.
fn jresult(r: Result<PathBuf, ArchonPathError>) -> String {
    match r {
        Ok(p) => format!("{{\"ok\":true,\"value\":{}}}", jstr(&p.to_string_lossy())),
        Err(e) => format!("{{\"ok\":false,\"error\":{}}}", jstr(&e.to_string())),
    }
}

fn jpath(p: PathBuf) -> String {
    jstr(&p.to_string_lossy())
}

/// `Option<(owner,repo)>` → JSON matching TS `{owner,repo}` object or `null`.
fn jowner(o: Option<(String, String)>) -> String {
    match o {
        Some((owner, repo)) => format!("{{\"owner\":{},\"repo\":{}}}", jstr(&owner), jstr(&repo)),
        None => "null".to_string(),
    }
}

fn jvec(v: Vec<String>) -> String {
    let items: Vec<String> = v.iter().map(|s| jstr(s)).collect();
    format!("[{}]", items.join(","))
}

/// `Result<Option<PathBuf>>` for resolve_project_root_from_cwd → path-string or null.
fn jopt_path(r: Result<Option<PathBuf>, ArchonPathError>) -> String {
    match r {
        Ok(Some(p)) => jstr(&p.to_string_lossy()),
        Ok(None) => "null".to_string(),
        Err(e) => format!("{{\"error\":{}}}", jstr(&e.to_string())),
    }
}

fn obj(pairs: &[(&str, String)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", jstr(k), v))
        .collect();
    format!("{{{}}}", body.join(","))
}

fn jbool(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

fn keys_of_interest() -> Vec<String> {
    std::env::var("PARITY_KEYS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Snapshot the keys of interest: `{"K":"v",...}`.
///
/// UNSET keys are OMITTED — mirrors TS `JSON.stringify` dropping `undefined`-valued
/// fields, so the differential comparison is apples-to-apples (absent == unset).
fn snapshot(keys: &[String]) -> String {
    let body: Vec<String> = keys
        .iter()
        .filter_map(|k| {
            std::env::var(k)
                .ok()
                .map(|v| format!("{}:{}", jstr(k), jstr(&v)))
        })
        .collect();
    format!("{{{}}}", body.join(","))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixture = args.get(1).map(|s| s.as_str()).unwrap_or("");

    let line = match fixture {
        "home-docker-workspace"
        | "home-docker-root-ws"
        | "home-docker-archon-docker"
        | "home-archon-home-set"
        | "home-archon-home-tilde"
        | "home-archon-home-undefined"
        | "home-default"
        | "home-root-no-ws"
        | "home-archon-docker-not-true" => obj(&[
            ("isDocker", jbool(is_docker())),
            ("home", jresult(get_archon_home())),
        ]),

        "expand-tilde" => obj(&[
            ("tilde_bare", jpath(expand_tilde("~"))),
            ("tilde_slash", jpath(expand_tilde("~/x"))),
            ("tilde_backslash", jpath(expand_tilde("~\\x"))),
            ("tilde_nosep", jpath(expand_tilde("~foo"))),
            ("non_tilde", jpath(expand_tilde("/abs/path"))),
            ("relative", jpath(expand_tilde("rel/path"))),
            ("tilde_only_slash", jpath(expand_tilde("~/"))),
        ]),

        "parse-owner-repo" => obj(&[
            ("valid", jowner(parse_owner_repo("owner/repo"))),
            ("noslash", jowner(parse_owner_repo("noslash"))),
            ("three", jowner(parse_owner_repo("a/b/c"))),
            ("empty_owner", jowner(parse_owner_repo("/repo"))),
            ("empty_repo", jowner(parse_owner_repo("owner/"))),
            ("dot_owner", jowner(parse_owner_repo("./repo"))),
            ("dotdot_owner", jowner(parse_owner_repo("../repo"))),
            ("dot_repo", jowner(parse_owner_repo("owner/."))),
            ("dotdot_repo", jowner(parse_owner_repo("owner/.."))),
            ("space", jowner(parse_owner_repo("own er/repo"))),
            ("dashes_dots", jowner(parse_owner_repo("my-org/my.repo_1"))),
            ("slash_at_start_only", jowner(parse_owner_repo("/"))),
            ("empty", jowner(parse_owner_repo(""))),
            ("unicode", jowner(parse_owner_repo("öwner/repo"))),
            ("tilde_char", jowner(parse_owner_repo("own~er/repo"))),
            ("plus", jowner(parse_owner_repo("a+b/repo"))),
            (
                "trailing_slash_multi",
                jowner(parse_owner_repo("owner/repo/")),
            ),
            ("dotfile_owner", jowner(parse_owner_repo(".hidden/repo"))),
        ]),

        "path-builders" => obj(&[
            ("workspaces", jresult(get_archon_workspaces_path())),
            ("worktrees", jresult(get_archon_worktrees_path())),
            ("config", jresult(get_archon_config_path())),
            ("home_workflows", jresult(get_home_workflows_path())),
            ("home_commands", jresult(get_home_commands_path())),
            ("home_scripts", jresult(get_home_scripts_path())),
            (
                "legacy_home_workflows",
                jresult(get_legacy_home_workflows_path()),
            ),
            ("archon_env", jresult(get_archon_env_path())),
            (
                "repo_archon_env",
                jpath(get_repo_archon_env_path(Path::new("/projects/myapp"))),
            ),
            ("project_root", jresult(get_project_root("owner", "repo"))),
            (
                "project_source",
                jresult(get_project_source_path("owner", "repo")),
            ),
            (
                "project_worktrees",
                jresult(get_project_worktrees_path("owner", "repo")),
            ),
            (
                "project_artifacts",
                jresult(get_project_artifacts_path("owner", "repo")),
            ),
            (
                "project_logs",
                jresult(get_project_logs_path("owner", "repo")),
            ),
            (
                "run_artifacts",
                jresult(get_run_artifacts_path("owner", "repo", "run-123")),
            ),
            (
                "run_log",
                jresult(get_run_log_path("owner", "repo", "run-123")),
            ),
            ("web_dist", jresult(get_web_dist_dir("v0.3.2"))),
            (
                "cmd_search_none",
                jvec(get_command_folder_search_paths(None)),
            ),
            (
                "cmd_search_dup1",
                jvec(get_command_folder_search_paths(Some(".archon/commands"))),
            ),
            (
                "cmd_search_dup2",
                jvec(get_command_folder_search_paths(Some(
                    ".archon/commands/defaults",
                ))),
            ),
            (
                "cmd_search_empty",
                jvec(get_command_folder_search_paths(Some(""))),
            ),
            (
                "cmd_search_custom",
                jvec(get_command_folder_search_paths(Some("custom-cmds"))),
            ),
            ("wf_search", jvec(get_workflow_folder_search_paths())),
        ]),

        "resolve-project-root" => {
            let home = std::env::var("ARCHON_HOME").unwrap_or_default();
            obj(&[
                (
                    "under_valid",
                    jopt_path(resolve_project_root_from_cwd(&PathBuf::from(format!(
                        "{}/workspaces/myorg/myrepo/source/sub",
                        home
                    )))),
                ),
                (
                    "under_exact",
                    jopt_path(resolve_project_root_from_cwd(&PathBuf::from(format!(
                        "{}/workspaces/myorg/myrepo",
                        home
                    )))),
                ),
                (
                    "one_segment",
                    jopt_path(resolve_project_root_from_cwd(&PathBuf::from(format!(
                        "{}/workspaces/onlyone",
                        home
                    )))),
                ),
                (
                    "not_under",
                    jopt_path(resolve_project_root_from_cwd(Path::new("/some/other/dir"))),
                ),
            ])
        }

        "env-loader" => {
            let cwd = args.get(2).cloned().unwrap_or_default();
            let keys = keys_of_interest();
            let before = snapshot(&keys);
            load_archon_env(Path::new(&cwd));
            let after = snapshot(&keys);
            obj(&[("before", before), ("after", after)])
        }

        "strip-cwd-env" => {
            let cwd = args.get(2).cloned().unwrap_or_default();
            let keys = keys_of_interest();
            let before = snapshot(&keys);
            strip_cwd_env(Path::new(&cwd));
            let after = snapshot(&keys);
            obj(&[("before", before), ("after", after)])
        }

        other => {
            eprintln!("unknown fixture: {}", other);
            std::process::exit(2);
        }
    };

    println!("{}", line);
}
