//! Differential parity oracle for WF-18 script-discovery (sub-cycle 4b).
//! Mirrors `packages/workflows/src/_oracle_wf18.ts`. Runs the Rust
//! `discover_scripts` / `discover_scripts_for_cwd` / `get_default_scripts`
//! over the same on-disk fixtures and emits JSON for diffing.
//!
//! Usage: ARCHON_HOME=$ROOT/home cargo run -p har-dag-executor --example wf18_oracle -- $ROOT

use har_dag_executor::{discover_scripts, discover_scripts_for_cwd, get_default_scripts};
use std::path::Path;

fn norm(p: &str, root: &str) -> String {
    p.replace(root, "<ROOT>")
}

async fn try_discover(dir: &str, root: &str) -> serde_json::Value {
    match discover_scripts(Path::new(dir)).await {
        Ok(map) => {
            // raw HashMap iteration order (demonstrates order behavior)
            let raw: Vec<serde_json::Value> = map
                .iter()
                .map(|(k, v)| {
                    serde_json::json!({
                        "key": k, "name": v.name,
                        "path": norm(&v.path, root),
                        "runtime": match v.runtime { har_workflow_schema::ScriptRuntime::Bun => "bun", har_workflow_schema::ScriptRuntime::Uv => "uv" }
                    })
                })
                .collect();
            // sorted-by-key for content comparison
            let mut sorted = raw.clone();
            sorted.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
            serde_json::json!({ "ok": true, "entries_sorted": sorted, "raw_order": raw.iter().map(|e| e["key"].clone()).collect::<Vec<_>>() })
        }
        Err(e) => serde_json::json!({ "ok": false, "error": norm(&e.to_string(), root) }),
    }
}

async fn try_for_cwd(cwd: &str, root: &str) -> serde_json::Value {
    match discover_scripts_for_cwd(Path::new(cwd)).await {
        Ok(map) => {
            let raw: Vec<serde_json::Value> = map
                .iter()
                .map(|(k, v)| {
                    serde_json::json!({
                        "key": k, "name": v.name,
                        "path": norm(&v.path, root),
                        "runtime": match v.runtime { har_workflow_schema::ScriptRuntime::Bun => "bun", har_workflow_schema::ScriptRuntime::Uv => "uv" }
                    })
                })
                .collect();
            let mut sorted = raw.clone();
            sorted.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
            serde_json::json!({ "ok": true, "entries_sorted": sorted, "raw_order": raw.iter().map(|e| e["key"].clone()).collect::<Vec<_>>() })
        }
        Err(e) => serde_json::json!({ "ok": false, "error": norm(&e.to_string(), root) }),
    }
}

#[tokio::main]
async fn main() {
    let root = std::env::args().nth(1).expect("ROOT arg");
    let out = serde_json::json!({
        "for_cwd": try_for_cwd(&format!("{root}/repo"), &root).await,
        "repo_scope": try_discover(&format!("{root}/repo/.archon/scripts"), &root).await,
        "home_scope": try_discover(&format!("{root}/home/scripts"), &root).await,
        "dup": try_discover(&format!("{root}/dup"), &root).await,
        "empty": try_discover(&format!("{root}/empty"), &root).await,
        "nonexistent": try_discover(&format!("{root}/does_not_exist_xyz"), &root).await,
        "unreadable": try_discover(&format!("{root}/unreadable"), &root).await,
        "notadir": try_discover(&format!("{root}/notadir_file.ts"), &root).await,
        "default_scripts_size": get_default_scripts().len(),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
