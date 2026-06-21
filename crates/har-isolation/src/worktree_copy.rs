/// Worktree file copy utility.
///
/// Ports `packages/isolation/src/worktree-copy.ts`.
///
/// Copies git-ignored files from the canonical repo to a new worktree based on
/// configuration entries. Each entry is a relative path (file or directory) that
/// is copied from the source root to an identical relative path in the target root.
///
/// ## Exact semantics ported from source (worktree-copy.ts)
///
/// `parse_copy_file_entry(entry)`:
/// - Trims whitespace from entry string (`.trim()`, `types.ts:37`)
/// - Empty after trim → error ("Copy entry cannot be empty")
/// - Returns `{source: trimmed, destination: trimmed}` (source == destination)
///
/// `is_path_within_root(root, file_path)`:
/// - Normalizes `join(root, file_path)` and `root` separately
/// - Gets relative path from root to joined; if it starts with ".." or is absolute → false
/// - Cross-drive (Windows) paths appear as absolute → false (same rule)
///
/// `copy_worktree_file(source_root, dest_root, entry)`:
/// - Path traversal check on both source and dest entries → false + error log
/// - `stat()` the source path → if ENOENT → false + debug log (silently skipped)
/// - If directory → `cp -r` (recursive); if file → single `copyFile`
/// - Ensures destination parent directory exists first
/// - Other errors → false + error log (never throws from JS perspective)
///
/// `copy_worktree_files(canonical, worktree, copy_files)`:
/// - Iterates entries sequentially (for loop, not parallel)
/// - Parse error (empty entry) → error log, continue
/// - Returns only the successfully copied entries
use std::path::{Path, PathBuf};
use tracing::{debug, error};

/// A parsed copy entry (source == destination, both are the trimmed relative path).
/// Source: `CopyFileEntry` at `worktree-copy.ts:19-22`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyFileEntry {
    pub source: String,
    pub destination: String,
}

/// Parse a single copy-files config entry.
///
/// Trims whitespace; rejects empty strings.
/// Source: `parseCopyFileEntry` at `worktree-copy.ts:32-40`.
pub fn parse_copy_file_entry(entry: &str) -> Result<CopyFileEntry, String> {
    let trimmed = entry.trim().to_string();
    if trimmed.is_empty() {
        return Err("Copy entry cannot be empty".to_string());
    }
    Ok(CopyFileEntry {
        source: trimmed.clone(),
        destination: trimmed,
    })
}

/// Join `file_path` onto `root` with **Node `path.join` semantics**.
///
/// CRITICAL: Rust's `Path::join("/x", "/abs")` *replaces* with the absolute arg
/// (`/abs`), but Node's `path.join('/x', '/abs')` *concatenates* (`/x/abs`) — it
/// never lets the second argument override the first. The whole path-traversal
/// guard depends on this: in the source an absolute `copyFiles` entry like
/// `/etc/hosts` is appended under the repo root (`<root>/etc/hosts`) rather than
/// escaping to the real `/etc/hosts`. We must mirror that exactly, otherwise the
/// guard rejects entries the source accepts (and a naive `Path::join` would even
/// read the real absolute path). Source: `worktree-copy.ts:52,104-105`.
fn node_join(root: &Path, file_path: &str) -> PathBuf {
    // Strip any leading separators from `file_path` so it is always treated as
    // relative to `root` (Node concatenates; the leading `/` becomes a no-op
    // separator). This matches `path.join(root, '/etc/x')` === `root/etc/x`.
    let rel = file_path.trim_start_matches(['/', '\\']);
    root.join(rel)
}

/// Check whether a relative `file_path` stays within `root` (path traversal guard).
///
/// Ports `isPathWithinRoot` from `worktree-copy.ts:50-65`.
///
/// 1. `join(root, file_path)` (Node semantics) then normalize both
/// 2. Get relative path from root → joined
/// 3. Starts with ".." or is absolute → false
pub fn is_path_within_root(root: &Path, file_path: &str) -> bool {
    let full = normalize_path(&node_join(root, file_path));
    let normalized_root = normalize_path(root);

    // Compute relative path from root to full.
    full.strip_prefix(&normalized_root).is_ok()
}

/// Normalize a path by resolving all `.` and `..` components without hitting the
/// filesystem (mirrors Node `path.normalize`). Since we can't call `canonicalize`
/// (path may not exist yet), we do a manual segment walk.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Only pop a Normal segment — never pop RootDir or Prefix.
                match out.last() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    _ => {
                        // If there's a root or nothing, just push the `..` (or drop it).
                        // For rooted paths the `..` is effectively a no-op past the root.
                        // For relative paths without a root, we push it so relative paths
                        // that escape their prefix are represented correctly.
                        if matches!(
                            out.last(),
                            Some(Component::RootDir) | Some(Component::Prefix(_))
                        ) {
                            // Can't go above root — drop the `..`
                        } else {
                            out.push(component);
                        }
                    }
                }
            }
            Component::CurDir => {} // skip `.`
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// Copy a single file or directory from `source_root/entry.source` to
/// `dest_root/entry.destination`.
///
/// Returns `true` on success, `false` for:
/// - Path traversal detected (security guard)
/// - Source ENOENT (expected, silently skipped)
/// - Other errors (logged but not thrown)
///
/// Source: `copyWorktreeFile` at `worktree-copy.ts:78-147`.
pub async fn copy_worktree_file(
    source_root: &Path,
    dest_root: &Path,
    entry: &CopyFileEntry,
) -> bool {
    // Security: path traversal guard on both source and destination.
    if !is_path_within_root(source_root, &entry.source) {
        error!(
            source = %entry.source,
            source_root = %source_root.display(),
            reason = "Source path escapes repository root",
            "path_traversal_blocked"
        );
        return false;
    }

    if !is_path_within_root(dest_root, &entry.destination) {
        error!(
            destination = %entry.destination,
            dest_root = %dest_root.display(),
            reason = "Destination path escapes worktree root",
            "path_traversal_blocked"
        );
        return false;
    }

    // Node `path.join` semantics (see `node_join`): an absolute entry is
    // appended under the root, never used to escape it. Mirrors source
    // `join(sourceRoot, entry.source)` / `join(destRoot, entry.destination)`.
    let source_path = node_join(source_root, &entry.source);
    let dest_path = node_join(dest_root, &entry.destination);

    // Stat the source to decide file vs directory, and to detect ENOENT.
    match tokio::fs::metadata(&source_path).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(source = %entry.source, "file_skipped_not_found");
            false
        }
        Err(e) => {
            error!(
                source = %entry.source,
                destination = %entry.destination,
                source_path = %source_path.display(),
                dest_path = %dest_path.display(),
                err = %e,
                code = "UNKNOWN",
                "copy_failed"
            );
            false
        }
        Ok(meta) => {
            // Ensure destination parent directory exists.
            if let Some(parent) = dest_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    error!(
                        dest_path = %dest_path.display(),
                        err = %e,
                        "copy_failed"
                    );
                    return false;
                }
            }

            let result = if meta.is_dir() {
                // Copy directory recursively (mirrors `cp(src, dst, { recursive: true })`).
                copy_dir_recursive(&source_path, &dest_path).await
            } else {
                // Copy single file (mirrors `copyFile(src, dst)`).
                tokio::fs::copy(&source_path, &dest_path).await.map(|_| ())
            };

            match result {
                Ok(()) => {
                    debug!(
                        source = %entry.source,
                        destination = %entry.destination,
                        "file_copied"
                    );
                    true
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    debug!(source = %entry.source, "file_skipped_not_found");
                    false
                }
                Err(e) => {
                    error!(
                        source = %entry.source,
                        destination = %entry.destination,
                        source_path = %source_path.display(),
                        dest_path = %dest_path.display(),
                        err = %e,
                        code = "UNKNOWN",
                        "copy_failed"
                    );
                    false
                }
            }
        }
    }
}

/// Recursively copy a directory tree from `src` to `dst`.
/// Mirrors Node's `cp(src, dst, { recursive: true })`.
///
/// Uses `Box::pin` to avoid the infinite-future-size issue with recursive async fns.
fn copy_dir_recursive<'a>(
    src: &'a Path,
    dst: &'a Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(dst).await?;
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let file_name = entry.file_name();
            let dst_path = dst.join(&file_name);
            let ft = entry.file_type().await?;
            if ft.is_dir() {
                copy_dir_recursive(&src_path, &dst_path).await?;
            } else {
                tokio::fs::copy(&src_path, &dst_path).await?;
            }
        }
        Ok(())
    })
}

/// Copy all configured files from the canonical repo to the worktree.
///
/// Sequential iteration (mirrors the JS `for` loop, not `Promise.all`).
/// Parse errors (empty entries) are logged and skipped.
/// Returns the list of successfully copied entries.
///
/// Source: `copyWorktreeFiles` at `worktree-copy.ts:157-179`.
pub async fn copy_worktree_files(
    canonical_repo_path: &Path,
    worktree_path: &Path,
    copy_files: &[String],
) -> Vec<CopyFileEntry> {
    let mut copied = Vec::new();

    for file_config in copy_files {
        match parse_copy_file_entry(file_config) {
            Err(e) => {
                // Invalid config entry (e.g., empty string) — log and continue.
                error!(entry = %file_config, err = %e, "invalid_config_entry");
            }
            Ok(entry) => {
                let success = copy_worktree_file(canonical_repo_path, worktree_path, &entry).await;
                if success {
                    copied.push(entry);
                }
            }
        }
    }

    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    // ─── parse_copy_file_entry ───────────────────────────────────────────────

    #[test]
    fn parse_empty_entry_fails() {
        let result = parse_copy_file_entry("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Copy entry cannot be empty");
    }

    #[test]
    fn parse_whitespace_only_fails() {
        let result = parse_copy_file_entry("   ");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Copy entry cannot be empty");
    }

    #[test]
    fn parse_plain_entry_succeeds() {
        let entry = parse_copy_file_entry(".env").unwrap();
        assert_eq!(entry.source, ".env");
        assert_eq!(entry.destination, ".env");
    }

    #[test]
    fn parse_trims_whitespace() {
        let entry = parse_copy_file_entry("  data/fixtures/  ").unwrap();
        assert_eq!(entry.source, "data/fixtures/");
        assert_eq!(entry.destination, "data/fixtures/");
    }

    #[test]
    fn parse_source_equals_destination() {
        let entry = parse_copy_file_entry("secrets/.env.production").unwrap();
        assert_eq!(entry.source, entry.destination);
    }

    // ─── is_path_within_root ────────────────────────────────────────────────

    #[test]
    fn path_within_root_plain_file() {
        let root = Path::new("/repo");
        assert!(is_path_within_root(root, ".env"));
    }

    #[test]
    fn path_within_root_nested() {
        let root = Path::new("/repo");
        assert!(is_path_within_root(root, "data/fixtures/seed.sql"));
    }

    #[test]
    fn path_within_root_dotdot_escapes() {
        let root = Path::new("/repo");
        assert!(!is_path_within_root(root, "../outside/file"));
    }

    #[test]
    fn path_within_root_absolute_is_appended_under_root() {
        // CORRECTED (cycle-9 parity): an absolute entry does NOT escape — Node
        // `path.join` appends it under root, so it stays within. The earlier
        // assertion (`!within`) was a porter assumption that diverged from the
        // TS source; differential testing against bun proved within == true.
        let root = Path::new("/repo");
        assert!(is_path_within_root(root, "/etc/passwd"));
    }

    #[test]
    fn path_within_root_absolute_entry_is_appended_not_replaced() {
        // PARITY (cycle 9): Node `path.join('/repo', '/etc/passwd')` === '/repo/etc/passwd',
        // so isPathWithinRoot('/repo', '/etc/passwd') === TRUE in the TS source
        // (the absolute arg is appended under root, NOT used to escape). A naive
        // Rust `Path::join` replaces with the absolute arg and would return false.
        // We must match the source: within-root = true.
        let root = Path::new("/repo");
        assert!(
            is_path_within_root(root, "/etc/passwd"),
            "absolute entry must be appended under root (Node join semantics), matching TS source"
        );
    }

    #[tokio::test]
    async fn copy_absolute_entry_reads_under_root_not_real_path() {
        // PARITY (cycle 9): an absolute entry `/etc/hosts` with `<src>/etc/hosts`
        // present is copied from UNDER the root (matching TS), and never touches
        // the real `/etc/hosts`.
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        tokio::fs::create_dir_all(src_dir.path().join("etc"))
            .await
            .unwrap();
        tokio::fs::write(src_dir.path().join("etc/hosts"), "INNER")
            .await
            .unwrap();

        let copied =
            copy_worktree_files(src_dir.path(), dst_dir.path(), &["/etc/hosts".to_string()]).await;

        assert_eq!(
            copied.len(),
            1,
            "absolute entry resolving under root should be copied"
        );
        assert_eq!(copied[0].source, "/etc/hosts");
        let content = tokio::fs::read_to_string(dst_dir.path().join("etc/hosts"))
            .await
            .unwrap();
        assert_eq!(
            content, "INNER",
            "must copy the under-root file, not the real /etc/hosts"
        );
    }

    #[test]
    fn path_within_root_dotdot_then_back() {
        // "/repo" + "../../other/.env" → joins to "/repo/../../other/.env"
        // normalized: root "/" → pop "repo" → pop past root (dropped) → "other/.env"
        // result: "/other/.env" which does NOT start with "/repo", escapes.
        let root = Path::new("/repo");
        assert!(!is_path_within_root(root, "../../other/.env"));
    }

    // ─── copy_worktree_file ──────────────────────────────────────────────────

    #[tokio::test]
    async fn copy_single_file_succeeds() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Create source file.
        tokio::fs::write(src_dir.path().join(".env"), "SECRET=123")
            .await
            .unwrap();

        let entry = CopyFileEntry {
            source: ".env".to_string(),
            destination: ".env".to_string(),
        };

        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(ok, "expected copy to succeed");

        let content = tokio::fs::read_to_string(dst_dir.path().join(".env"))
            .await
            .unwrap();
        assert_eq!(content, "SECRET=123");
    }

    #[tokio::test]
    async fn copy_missing_source_returns_false() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let entry = CopyFileEntry {
            source: "nonexistent.env".to_string(),
            destination: "nonexistent.env".to_string(),
        };

        // ENOENT → silently false
        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(!ok, "missing source should return false");
    }

    #[tokio::test]
    async fn copy_directory_recursive() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Create a nested directory structure.
        tokio::fs::create_dir_all(src_dir.path().join("data/fixtures"))
            .await
            .unwrap();
        tokio::fs::write(
            src_dir.path().join("data/fixtures/seed.sql"),
            "INSERT INTO foo VALUES (1);",
        )
        .await
        .unwrap();
        tokio::fs::write(src_dir.path().join("data/config.json"), r#"{"key":"val"}"#)
            .await
            .unwrap();

        let entry = CopyFileEntry {
            source: "data".to_string(),
            destination: "data".to_string(),
        };

        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(ok, "directory copy should succeed");

        // Verify recursive copy.
        let seed = tokio::fs::read_to_string(dst_dir.path().join("data/fixtures/seed.sql"))
            .await
            .unwrap();
        assert!(seed.contains("INSERT INTO foo"));

        let cfg = tokio::fs::read_to_string(dst_dir.path().join("data/config.json"))
            .await
            .unwrap();
        assert!(cfg.contains("key"));
    }

    #[tokio::test]
    async fn path_traversal_source_blocked() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let entry = CopyFileEntry {
            source: "../../../etc/passwd".to_string(),
            destination: "passwd".to_string(),
        };

        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(!ok, "path traversal should be blocked");
    }

    #[tokio::test]
    async fn path_traversal_dest_blocked() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Create a valid source file
        tokio::fs::write(src_dir.path().join("legit.txt"), "ok")
            .await
            .unwrap();

        let entry = CopyFileEntry {
            source: "legit.txt".to_string(),
            destination: "../../outside/legit.txt".to_string(),
        };

        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(!ok, "path traversal in dest should be blocked");
    }

    #[tokio::test]
    async fn copy_creates_parent_dirs() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // Source has nested file, destination parent doesn't exist yet.
        tokio::fs::create_dir_all(src_dir.path().join("a/b/c"))
            .await
            .unwrap();
        tokio::fs::write(src_dir.path().join("a/b/c/deep.txt"), "deep content")
            .await
            .unwrap();

        let entry = CopyFileEntry {
            source: "a/b/c/deep.txt".to_string(),
            destination: "a/b/c/deep.txt".to_string(),
        };

        let ok = copy_worktree_file(src_dir.path(), dst_dir.path(), &entry).await;
        assert!(ok, "should create parent dirs and copy file");

        let content = tokio::fs::read_to_string(dst_dir.path().join("a/b/c/deep.txt"))
            .await
            .unwrap();
        assert_eq!(content, "deep content");
    }

    // ─── copy_worktree_files ─────────────────────────────────────────────────

    #[tokio::test]
    async fn copy_multiple_files() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        tokio::fs::write(src_dir.path().join(".env"), "A=1")
            .await
            .unwrap();
        tokio::fs::write(src_dir.path().join("config.yaml"), "x: y")
            .await
            .unwrap();

        let result = copy_worktree_files(
            src_dir.path(),
            dst_dir.path(),
            &[".env".to_string(), "config.yaml".to_string()],
        )
        .await;

        assert_eq!(result.len(), 2, "both files should be copied");
    }

    #[tokio::test]
    async fn copy_files_skips_missing_source() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        tokio::fs::write(src_dir.path().join("exists.txt"), "hello")
            .await
            .unwrap();

        let result = copy_worktree_files(
            src_dir.path(),
            dst_dir.path(),
            &["exists.txt".to_string(), "missing.txt".to_string()],
        )
        .await;

        assert_eq!(
            result.len(),
            1,
            "only the existing file should be in result"
        );
        assert_eq!(result[0].source, "exists.txt");
    }

    #[tokio::test]
    async fn copy_files_skips_empty_entry() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        tokio::fs::write(src_dir.path().join(".env"), "SECRET=x")
            .await
            .unwrap();

        let result = copy_worktree_files(
            src_dir.path(),
            dst_dir.path(),
            &[
                ".env".to_string(),
                "".to_string(),    // empty entry — should be skipped, not panic
                "   ".to_string(), // whitespace only
            ],
        )
        .await;

        assert_eq!(result.len(), 1, "only .env should be counted as copied");
    }

    #[tokio::test]
    async fn copy_files_returns_empty_when_all_missing() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let result = copy_worktree_files(
            src_dir.path(),
            dst_dir.path(),
            &["none.txt".to_string(), "also_none.txt".to_string()],
        )
        .await;

        assert!(result.is_empty(), "all-missing should give empty result");
    }
}
