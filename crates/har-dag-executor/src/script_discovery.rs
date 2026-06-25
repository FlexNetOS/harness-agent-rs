//! Port of `packages/workflows/src/script-discovery.ts` (170 ln) — UNIT WF-18.
//!
//! Finds and loads script files from `.archon/scripts/`.  Scripts are keyed by
//! filename-without-extension.  Runtime is auto-detected from file extension:
//!   `.ts` / `.js` → bun,  `.py` → uv.
//!
//! Public surface (all TS exports ported):
//!   - `ScriptDefinition`          (struct)
//!   - `MAX_SCRIPT_DISCOVERY_DEPTH` (const = 1)
//!   - `get_runtime_for_extension`  (fn)
//!   - `scan_script_dir`            (async fn, recursive with depth cap)
//!   - `discover_scripts`           (async fn)
//!   - `discover_scripts_for_cwd`   (async fn, repo > home precedence)
//!   - `get_default_scripts`        (fn → empty map)
//!   - `ScriptDiscoveryError`       (error type)
//!
//! Internal: `normalize_sep` (forward-slash normalisation).
//!
//! Source: script-discovery.ts:1-170.

use har_paths::get_home_scripts_path;
use har_workflow_schema::ScriptRuntime;
use indexmap::IndexMap;
use std::path::Path;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Map of script name → definition, **insertion-ordered** to mirror the TS `Map`
/// (whose iteration order is readdir-population order). A `HashMap` would drop this
/// ordering guarantee and make `listScripts`-style consumers non-reproducible
/// (verifier D-ORDER). Source: `Map<string, ScriptDefinition>` (script-discovery.ts).
pub type ScriptMap = IndexMap<String, ScriptDefinition>;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum subfolder depth we descend into when scanning scripts.
///
/// `1` matches the workflows/commands convention: allow one level of
/// grouping (e.g. `.archon/scripts/triage/foo.ts`) but no nested folders.
/// Source: script-discovery.ts:57.
pub const MAX_SCRIPT_DISCOVERY_DEPTH: usize = 1;

// ─── Types ────────────────────────────────────────────────────────────────────

/// A discovered script with its metadata. Source: script-discovery.ts:27-31.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDefinition {
    /// Script name (filename without extension).
    pub name: String,
    /// Absolute path to the script file (forward-slash normalised).
    pub path: String,
    /// Runtime that should execute this script.
    pub runtime: ScriptRuntime,
}

/// Errors that can occur during script discovery.
///
/// Maps the TS `throw new Error(...)` calls in `scanScriptDir` /
/// `discoverScripts` / `discoverScriptsForCwd`.
#[derive(Debug, Error)]
pub enum ScriptDiscoveryError {
    /// Non-ENOENT error reading a directory. Source: script-discovery.ts:76-79.
    ///
    /// `message` and `code` are reconstructed to match Node's `ErrnoException`
    /// shape so the surfaced string is byte-identical to TS — `message` is
    /// `"<CODE>: <strerror>, scandir '<path>'"` and `code` is the **symbolic**
    /// errno (e.g. `EACCES`, NOT the numeric `13`). See [`node_readdir_error`].
    #[error("Directory read error: {message} ({code})")]
    DirReadError { message: String, code: String },

    /// Two script files share the same basename (different extensions).
    /// Source: script-discovery.ts:113-117.
    #[error(
        "Duplicate script name \"{name}\": found \"{existing}\" and \"{new}\". \
         Script names must be unique across extensions."
    )]
    DuplicateScriptName {
        name: String,
        existing: String,
        new: String,
    },

    /// `get_home_scripts_path()` returned an error (ARCHON_HOME unresolvable).
    #[error("Path resolution error: {0}")]
    PathError(String),
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Normalize path separators to forward slashes for cross-platform consistency.
/// Source: script-discovery.ts:12-14.
fn normalize_sep(p: &str) -> String {
    p.replace('\\', "/")
}

/// Reconstruct Node's `ErrnoException` `(message, code)` pair for a failed
/// directory read, so the surfaced error string matches the TS source byte-for-byte.
///
/// Node's `readdir` rejects with `err.message = "<CODE>: <strerror>, scandir '<path>'"`
/// and `err.code = "<CODE>"` (a **symbolic** errno from libuv, e.g. `EACCES`).
/// The TS wrap is `Directory read error: ${err.message} (${err.code ?? 'unknown'})`
/// (script-discovery.ts:78). Rust's `io::Error` Display gives a different body
/// (`"Permission denied (os error 13)"`) and `raw_os_error()` gives the **numeric**
/// code (`13`) — so we map errno → (symbolic code, libuv strerror text) here.
///
/// The strerror bodies match libuv's `uv_strerror` (what Node surfaces). The errno
/// numbers are the Linux values. Unmapped errnos fall back to the raw `io::Error`
/// Display + `"unknown"` code (mirrors TS `err.code ?? 'unknown'`).
fn node_readdir_error(err: &std::io::Error, dir_path: &Path) -> (String, String) {
    let mapped: Option<(&str, &str)> = match err.raw_os_error() {
        Some(1) => Some(("EPERM", "operation not permitted")),
        Some(2) => Some(("ENOENT", "no such file or directory")),
        Some(13) => Some(("EACCES", "permission denied")),
        Some(20) => Some(("ENOTDIR", "not a directory")),
        Some(23) => Some(("ENFILE", "file table overflow")),
        Some(24) => Some(("EMFILE", "too many open files")),
        Some(36) => Some(("ENAMETOOLONG", "name too long")),
        Some(40) => Some(("ELOOP", "too many symbolic links encountered")),
        _ => None,
    };
    match mapped {
        Some((code, desc)) => {
            // Node `readdir` syscall label is `scandir`. Path is the one passed to readdir.
            let message = format!("{}: {}, scandir '{}'", code, desc, dir_path.display());
            (message, code.to_string())
        }
        None => {
            // No symbolic mapping — preserve the OS message, mark code 'unknown'
            // (matches TS `err.code ?? 'unknown'` when the field is undefined).
            (err.to_string(), "unknown".to_string())
        }
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Derive the runtime from a file extension (dot-prefixed, e.g. `".ts"`).
/// Returns `None` for unknown extensions. Source: script-discovery.ts:44-46.
pub fn get_runtime_for_extension(ext: &str) -> Option<ScriptRuntime> {
    match ext {
        ".ts" | ".js" => Some(ScriptRuntime::Bun),
        ".py" => Some(ScriptRuntime::Uv),
        _ => None,
    }
}

/// Scan a directory for script files, descending at most `MAX_SCRIPT_DISCOVERY_DEPTH`
/// folders deep.  Skips files with unknown extensions.  Throws on duplicate script names.
///
/// The function mutates the caller-supplied `scripts` map (matches TS: same `Map`
/// reference is threaded through all recursive calls).
///
/// Source: script-discovery.ts:63-122.
pub async fn scan_script_dir(
    dir_path: &Path,
    scripts: &mut ScriptMap,
    depth: usize,
) -> Result<(), ScriptDiscoveryError> {
    // readdir equivalent. Source: 69-79.
    let mut rd = match tokio::fs::read_dir(dir_path).await {
        Ok(rd) => rd,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                debug!(dir_path = %dir_path.display(), "script_directory_not_found");
                return Ok(());
            }
            warn!(dir_path = %dir_path.display(), err = %err, "script_directory_read_error");
            // Reconstruct Node's (symbolic code, scandir message) so the surfaced
            // string matches TS byte-for-byte (verifier D-ERR). Source: 76-79.
            let (message, code) = node_readdir_error(&err, dir_path);
            return Err(ScriptDiscoveryError::DirReadError { message, code });
        }
    };

    // Collect entries into a Vec so we can iterate them (mirrors TS `readdir` returning
    // a single String[] that is then iterated with `for...of`).
    let mut entries = Vec::new();
    loop {
        match rd.next_entry().await {
            Ok(Some(e)) => entries.push(e),
            Ok(None) => break,
            Err(err) => {
                // OS-level error reading a single entry — log and skip (robust).
                warn!(dir_path = %dir_path.display(), err = %err, "script_directory_entry_read_error");
            }
        }
    }

    for entry in entries {
        let entry_path = entry.path();

        // stat equivalent. Source: 84-90.
        let entry_stat = match tokio::fs::metadata(&entry_path).await {
            Ok(m) => m,
            Err(err) => {
                warn!(entry_path = %entry_path.display(), err = %err, "script_file_stat_error");
                continue; // TS: `continue`
            }
        };

        if entry_stat.is_dir() {
            // 1-depth cap. Source: script-discovery.ts:96.
            if depth >= MAX_SCRIPT_DISCOVERY_DEPTH {
                continue;
            }
            // Recursive call. Box::pin required for async recursion.
            Box::pin(scan_script_dir(&entry_path, scripts, depth + 1)).await?;
            continue;
        }

        // Extract extension (dot-prefixed). Source: 101.
        let os_name = entry.file_name();
        let file_name = match os_name.to_str() {
            Some(s) => s,
            None => continue,
        };
        let ext = match Path::new(file_name).extension().and_then(|s| s.to_str()) {
            Some(e) => format!(".{}", e),
            None => {
                debug!(entry_path = %entry_path.display(), ext = "", "script_unknown_extension_skipped");
                continue;
            }
        };

        let runtime = match get_runtime_for_extension(&ext) {
            Some(r) => r,
            None => {
                debug!(
                    entry_path = %entry_path.display(),
                    ext,
                    "script_unknown_extension_skipped"
                );
                continue;
            }
        };

        // name = basename(entry, ext). Source: 109.
        let name = match Path::new(file_name).file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Duplicate check. Source: 111-117.
        if let Some(existing) = scripts.get(&name) {
            return Err(ScriptDiscoveryError::DuplicateScriptName {
                name,
                existing: existing.path.clone(),
                new: normalize_sep(&entry_path.to_string_lossy()),
            });
        }

        let normalized_path = normalize_sep(&entry_path.to_string_lossy());
        debug!(name, ?runtime, entry_path = %entry_path.display(), "script_loaded");
        scripts.insert(
            name.clone(),
            ScriptDefinition {
                name,
                path: normalized_path,
                runtime,
            },
        );
    }

    Ok(())
}

/// Discover scripts from a directory (expected to be `.archon/scripts/` or equivalent).
///
/// Returns a [`ScriptMap`] (insertion-ordered) of script name → `ScriptDefinition`.
/// Returns an empty map if the directory does not exist.
/// Throws if duplicate script names are found across different extensions within the directory.
///
/// Insertion order = readdir traversal order (with inline depth-1 recursion), mirroring
/// the TS `Map`'s population order so `listScripts`-style consumers are reproducible
/// (verifier D-ORDER). Source: script-discovery.ts:130-135.
pub async fn discover_scripts(dir: &Path) -> Result<ScriptMap, ScriptDiscoveryError> {
    let mut scripts = ScriptMap::new();
    scan_script_dir(dir, &mut scripts, 0).await?;
    info!(count = scripts.len(), dir = %dir.display(), "scripts_discovery_completed");
    Ok(scripts)
}

/// Discover scripts across all scopes for a given repo `cwd`.
///
/// Resolution order (repo wins on same-name collision — matches the
/// workflows/commands precedence):
///   1. `~/.archon/scripts/`          — home-scoped
///   2. `<cwd>/.archon/scripts/`      — repo-scoped (wins on collision)
///
/// Within a single scope, duplicate basenames across extensions still throw
/// (matches `discover_scripts` behavior).  Across scopes, the repo-level entry
/// silently overrides the home-level one.
///
/// Returns a [`ScriptMap`] (insertion-ordered): home entries first (in home readdir
/// order), then repo entries appended. A repo entry whose name already exists in home
/// updates the value **in place** — `IndexMap::insert` on an existing key keeps the
/// original index, exactly as TS `Map.set` does. Source: script-discovery.ts:149-162.
pub async fn discover_scripts_for_cwd(cwd: &Path) -> Result<ScriptMap, ScriptDiscoveryError> {
    let home_scripts_path =
        get_home_scripts_path().map_err(|e| ScriptDiscoveryError::PathError(e.to_string()))?;

    let home_scripts = discover_scripts(&home_scripts_path).await?;
    let repo_scripts = discover_scripts(&cwd.join(".archon").join("scripts")).await?;

    // Start with home, overlay repo (repo wins). Source: 154-159.
    let mut merged = home_scripts;
    for (name, def) in repo_scripts {
        if merged.contains_key(&name) {
            debug!(name, "script.repo_overrides_home");
        }
        // IndexMap::insert keeps the existing index when the key is already present
        // (value-update-in-place) — matches TS `Map.set` ordering semantics.
        merged.insert(name, def);
    }
    Ok(merged)
}

/// Returns bundled default scripts (empty — no bundled scripts for now).
/// Follows the bundled-defaults.ts pattern for future extensibility.
/// Source: script-discovery.ts:168-170.
pub fn get_default_scripts() -> ScriptMap {
    ScriptMap::new()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Helper: write a file at `dir/name` with `content`.
    fn write_file(dir: &Path, name: &str, content: &[u8]) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(content).expect("write file");
    }

    /// Helper: write empty file.
    fn touch(dir: &Path, name: &str) {
        write_file(dir, name, b"");
    }

    // ── get_runtime_for_extension ─────────────────────────────────────────────

    #[test]
    fn runtime_ts_and_js_are_bun() {
        assert_eq!(get_runtime_for_extension(".ts"), Some(ScriptRuntime::Bun));
        assert_eq!(get_runtime_for_extension(".js"), Some(ScriptRuntime::Bun));
    }

    #[test]
    fn runtime_py_is_uv() {
        assert_eq!(get_runtime_for_extension(".py"), Some(ScriptRuntime::Uv));
    }

    #[test]
    fn runtime_unknown_returns_none() {
        assert_eq!(get_runtime_for_extension(".sh"), None);
        assert_eq!(get_runtime_for_extension(".rb"), None);
        assert_eq!(get_runtime_for_extension(""), None);
    }

    // ── normalize_sep ─────────────────────────────────────────────────────────

    #[test]
    fn normalize_sep_replaces_backslashes() {
        assert_eq!(normalize_sep("foo\\bar\\baz"), "foo/bar/baz");
        assert_eq!(normalize_sep("already/forward"), "already/forward");
    }

    // ── scan_script_dir / discover_scripts ────────────────────────────────────

    #[tokio::test]
    async fn discover_scripts_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let result = discover_scripts(tmp.path()).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn discover_scripts_nonexistent_dir() {
        // Should return empty map, not error.
        let result = discover_scripts(Path::new("/nonexistent/path/__archon_test__"))
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn discover_scripts_basic() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "foo.ts");
        touch(tmp.path(), "bar.py");
        touch(tmp.path(), "ignored.sh");

        let scripts = discover_scripts(tmp.path()).await.unwrap();
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts["foo"].runtime, ScriptRuntime::Bun);
        assert_eq!(scripts["bar"].runtime, ScriptRuntime::Uv);
        // .sh is ignored
        assert!(!scripts.contains_key("ignored"));
    }

    #[tokio::test]
    async fn discover_scripts_one_level_deep() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("triage");
        std::fs::create_dir(&sub).unwrap();
        touch(&sub, "triage_script.ts");
        // Also a script at root
        touch(tmp.path(), "root_script.py");

        let scripts = discover_scripts(tmp.path()).await.unwrap();
        assert_eq!(scripts.len(), 2);
        assert!(scripts.contains_key("triage_script"));
        assert!(scripts.contains_key("root_script"));
    }

    #[tokio::test]
    async fn discover_scripts_depth_cap_at_one() {
        let tmp = TempDir::new().unwrap();
        let sub1 = tmp.path().join("level1");
        std::fs::create_dir(&sub1).unwrap();
        let sub2 = sub1.join("level2");
        std::fs::create_dir(&sub2).unwrap();
        // This script is at depth 2 — should NOT be discovered.
        touch(&sub2, "deep_script.ts");
        // This script is at depth 1 — SHOULD be discovered.
        touch(&sub1, "shallow_script.ts");

        let scripts = discover_scripts(tmp.path()).await.unwrap();
        assert!(
            scripts.contains_key("shallow_script"),
            "depth-1 script must be found"
        );
        assert!(
            !scripts.contains_key("deep_script"),
            "depth-2 script must NOT be found (MAX_SCRIPT_DISCOVERY_DEPTH=1)"
        );
    }

    #[tokio::test]
    async fn discover_scripts_duplicate_name_same_scope_errors() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "foo.ts");
        touch(tmp.path(), "foo.py"); // same name, different extension → error

        let result = discover_scripts(tmp.path()).await;
        assert!(
            matches!(
                result,
                Err(ScriptDiscoveryError::DuplicateScriptName { .. })
            ),
            "Expected DuplicateScriptName, got {:?}",
            result
        );
    }

    // ── discover_scripts_for_cwd ──────────────────────────────────────────────

    #[tokio::test]
    async fn discover_scripts_for_cwd_repo_overrides_home() {
        let home_tmp = TempDir::new().unwrap();
        let repo_tmp = TempDir::new().unwrap();

        // Simulate home scripts dir = home_tmp/
        touch(home_tmp.path(), "shared.ts"); // bun version in home

        // Simulate repo scripts dir = repo_tmp/.archon/scripts/
        let archon_scripts = repo_tmp.path().join(".archon").join("scripts");
        std::fs::create_dir_all(&archon_scripts).unwrap();
        touch(&archon_scripts, "shared.py"); // uv version in repo (overrides home)
        touch(&archon_scripts, "repo_only.ts");

        // Discover repo scope
        let repo_scripts = discover_scripts(&archon_scripts).await.unwrap();
        // Discover home scope
        let home_scripts = discover_scripts(home_tmp.path()).await.unwrap();

        // Merge (repo wins) — mirrors discover_scripts_for_cwd logic
        let mut merged = home_scripts;
        for (name, def) in repo_scripts {
            merged.insert(name, def);
        }

        // "shared" should be the repo's uv version
        assert_eq!(
            merged["shared"].runtime,
            ScriptRuntime::Uv,
            "repo must override home"
        );
        assert!(merged.contains_key("repo_only"));
    }

    // ── get_default_scripts ───────────────────────────────────────────────────

    #[test]
    fn get_default_scripts_is_empty() {
        assert!(get_default_scripts().is_empty());
    }

    // ── D-ERR: Node-style EACCES directory-read error string ──────────────────

    /// Locks the verifier D-ERR fix: the `DirReadError` string must match Node's
    /// `ErrnoException` shape — symbolic code `EACCES` (not numeric `13`) and a
    /// `scandir '<path>'` message body. TS surfaces:
    /// `Directory read error: EACCES: permission denied, scandir '<path>' (EACCES)`.
    #[test]
    fn node_readdir_error_maps_eacces_symbolically() {
        let err = std::io::Error::from_raw_os_error(13); // EACCES on Linux
        let path = Path::new("/repo/.archon/scripts");
        let (message, code) = node_readdir_error(&err, path);

        assert_eq!(code, "EACCES", "code must be symbolic, not numeric 13");
        assert_eq!(
            message, "EACCES: permission denied, scandir '/repo/.archon/scripts'",
            "message must match Node's scandir errno shape"
        );

        // Full surfaced string (what execute_script_node wraps into node_failed).
        let dre = ScriptDiscoveryError::DirReadError { message, code };
        assert_eq!(
            dre.to_string(),
            "Directory read error: EACCES: permission denied, scandir '/repo/.archon/scripts' (EACCES)",
        );
    }

    /// ENOTDIR / EMFILE / ELOOP also map symbolically (spot-check the table).
    #[test]
    fn node_readdir_error_maps_other_errnos_symbolically() {
        let p = Path::new("/x");
        assert_eq!(
            node_readdir_error(&std::io::Error::from_raw_os_error(20), p).1,
            "ENOTDIR"
        );
        assert_eq!(
            node_readdir_error(&std::io::Error::from_raw_os_error(24), p).1,
            "EMFILE"
        );
        assert_eq!(
            node_readdir_error(&std::io::Error::from_raw_os_error(40), p).1,
            "ELOOP"
        );
    }

    /// Real unreadable directory (mode 000) surfaces the EACCES-shaped error.
    /// Skipped when the runtime bypasses permission bits (e.g. running as root),
    /// detected by probing the locked dir with `std::fs::read_dir` first.
    #[cfg(unix)]
    #[tokio::test]
    async fn discover_scripts_unreadable_dir_eacces_shape() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let locked = tmp.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Probe: if a plain read succeeds despite mode 000, the runtime bypasses
        // permission checks (root) — skip the shape assertion.
        let bypasses_perms = std::fs::read_dir(&locked).is_ok();

        let result = discover_scripts(&locked).await;

        // Restore perms so TempDir cleanup succeeds.
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));

        if bypasses_perms {
            assert!(result.is_ok(), "perm-bypassing runtime reads mode-000 dirs");
            return;
        }

        match result {
            Err(ScriptDiscoveryError::DirReadError { code, message }) => {
                assert_eq!(code, "EACCES");
                let expected_tail = format!("scandir '{}'", locked.display());
                assert_eq!(
                    message,
                    format!("EACCES: permission denied, {expected_tail}"),
                    "message must be Node scandir-shaped"
                );
            }
            other => panic!("expected DirReadError EACCES, got {other:?}"),
        }
    }

    // ── D-ORDER: discover_scripts preserves readdir insertion order ───────────

    /// Locks the verifier D-ORDER fix: `discover_scripts` returns a [`ScriptMap`]
    /// (IndexMap) whose iteration order equals the OS `readdir` traversal order —
    /// the same order the TS `Map` is populated in (both read OS-native order, no
    /// sorting). A `HashMap` would randomize this. Verified by comparing key order
    /// against an independent `std::fs::read_dir` walk over the same directory, and
    /// by confirming the order is reproducible across two discovery calls.
    #[tokio::test]
    async fn discover_scripts_preserves_readdir_insertion_order() {
        let tmp = TempDir::new().unwrap();
        for n in &[
            "m3.py",
            "m2.js",
            "m1.ts",
            "pyroot.py",
            "quux.js",
            "foo.ts",
            "shared.ts",
        ] {
            touch(tmp.path(), n);
        }
        touch(tmp.path(), "ignored.sh"); // unknown ext — must be absent

        // Expected = std::fs::read_dir order, filtered to known extensions.
        // Both std and tokio read_dir use the same unsorted OS readdir, so this is
        // the exact order discover_scripts (IndexMap insertion) must reproduce.
        let mut expected = Vec::new();
        for entry in std::fs::read_dir(tmp.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.metadata().unwrap().is_dir() {
                continue;
            }
            let fname = entry.file_name();
            let fname = fname.to_str().unwrap().to_string();
            let ext = Path::new(&fname)
                .extension()
                .and_then(|s| s.to_str())
                .map(|e| format!(".{e}"));
            if let Some(ext) = ext {
                if get_runtime_for_extension(&ext).is_some() {
                    expected.push(
                        Path::new(&fname)
                            .file_stem()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string(),
                    );
                }
            }
        }

        let scripts = discover_scripts(tmp.path()).await.unwrap();
        let actual: Vec<String> = scripts.keys().cloned().collect();
        assert_eq!(
            actual, expected,
            "discover_scripts key order must equal readdir order (IndexMap insertion)"
        );
        assert!(!actual.contains(&"ignored".to_string()));

        // Reproducibility: a second discovery yields identical order.
        let scripts2 = discover_scripts(tmp.path()).await.unwrap();
        let actual2: Vec<String> = scripts2.keys().cloned().collect();
        assert_eq!(actual, actual2, "order must be reproducible across calls");
    }
}
