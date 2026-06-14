//! GI-01 — Git subprocess execution helper.
//!
//! Ports `packages/git/src/exec.ts`.
//!
//! The source uses Node.js `child_process.execFile` (not `exec`) — meaning
//! **no shell interpolation**: the command and arguments are passed as a direct
//! argv array. This property is preserved here via `tokio::process::Command`
//! with explicit args (never `.arg("sh -c ...")` or similar).
//!
//! `execFileAsync` returns `{ stdout, stderr }` as strings; non-zero exit
//! throws an error whose message contains `stderr` content (Node wraps the
//! error text from the child process in `err.message`). We replicate this by
//! returning `Err(GitError::ProcessError { message })` on failure, where
//! `message` is built from the process's stderr and exit status.
//!
//! `mkdirAsync` wraps `fs.mkdir` (also async). We port it as an async fn that
//! calls `tokio::fs::create_dir_all`.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

use crate::types::{GitError, Result};

/// Output from a subprocess invocation.
/// Mirrors the `{ stdout, stderr }` object returned by `execFileAsync`.
#[derive(Debug, Default)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Options for `exec_file_async` — mirrors the optional third arg in the
/// TypeScript source.
#[derive(Debug, Default)]
pub struct ExecOptions<'a> {
    /// Working directory for the subprocess (mirrors `cwd` option).
    pub cwd: Option<&'a Path>,
    /// Timeout in milliseconds (mirrors `timeout` option).
    pub timeout_ms: Option<u64>,
    /// Extra environment variables to inject (mirrors `env` option).
    /// When set, **adds** to the inherited env rather than replacing it.
    pub env: Option<Vec<(String, String)>>,
}

/// Wrapper around `tokio::process::Command` for test mockability.
///
/// Corresponds to `execFileAsync(cmd, args, options)` in `exec.ts:8-18`.
///
/// Key properties preserved from the source:
/// - No shell: args are passed directly to the OS exec call (no quoting /
///   injection hazard). The TS source uses `execFile`, not `exec`.
/// - `stdout ?? ''` and `stderr ?? ''`: never returns `null`; empty string on
///   missing output.
/// - Non-zero exit → `Err`. The error message is derived from the child's
///   stderr (Node's promisified `execFile` includes stderr in err.message).
/// - Timeout: if `timeout_ms` is set and the child exceeds it, the process is
///   killed and an error is returned (mirrors Node's `options.timeout`).
pub async fn exec_file_async(
    cmd: &str,
    args: &[&str],
    options: ExecOptions<'_>,
) -> Result<ExecOutput> {
    let mut command = Command::new(cmd);
    command.args(args);
    command.kill_on_drop(true);

    if let Some(cwd) = options.cwd {
        command.current_dir(cwd);
    }

    // Inject extra env vars on top of the inherited environment.
    if let Some(env_pairs) = options.env {
        for (k, v) in env_pairs {
            command.env(k, v);
        }
    }

    // Capture stdout + stderr separately.
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let child = command.spawn().map_err(|e| GitError::ProcessError {
        message: format!("Failed to spawn '{}': {}", cmd, e),
    })?;

    // Apply timeout if requested.
    let output = if let Some(ms) = options.timeout_ms {
        match tokio::time::timeout(Duration::from_millis(ms), child.wait_with_output()).await {
            Ok(result) => result.map_err(|e| GitError::ProcessError {
                message: format!("Process wait error: {}", e),
            })?,
            Err(_) => {
                return Err(GitError::ProcessError {
                    message: format!(
                        "Command '{}' timed out after {}ms",
                        cmd, ms
                    ),
                });
            }
        }
    } else {
        child.wait_with_output().await.map_err(|e| GitError::ProcessError {
            message: format!("Process wait error: {}", e),
        })?
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        // Mirror Node's error shape: `err.message` includes the command and
        // stderr text. We build a similar message here so call sites that
        // inspect `err.message` + `err.stderr` in TS have an equivalent.
        let code = output.status.code().unwrap_or(-1);
        let message = if stderr.trim().is_empty() {
            format!(
                "Command failed: {} {}\nProcess exited with code {}",
                cmd,
                args.join(" "),
                code
            )
        } else {
            format!(
                "Command failed: {} {}\n{}\nProcess exited with code {}",
                cmd,
                args.join(" "),
                stderr.trim(),
                code
            )
        };
        return Err(GitError::ProcessError { message });
    }

    Ok(ExecOutput { stdout, stderr })
}

/// Async directory creation (recursive by default).
/// Corresponds to `mkdirAsync(path, options?)` in `exec.ts:21-23`.
///
/// The source uses `fs.mkdir` from Node's `fs/promises`. We use
/// `tokio::fs::create_dir_all` for the recursive=true case (which the
/// isolation layer uses). When `recursive` is false we call `create_dir`.
pub async fn mkdir_async(path: &Path, recursive: bool) -> Result<()> {
    if recursive {
        tokio::fs::create_dir_all(path).await.map_err(GitError::Io)
    } else {
        tokio::fs::create_dir(path).await.map_err(GitError::Io)
    }
}

/// Convenience: run a git command with `-C <repo>` prefix.
/// Most git operations in the source use `execFileAsync('git', ['-C', repoPath, ...])`.
///
/// `timeout_ms`: `Some(ms)` sets a timeout; `None` means no timeout (mirrors
/// the source's omitting the `timeout` option entirely).
pub async fn run_git(
    repo_or_cwd: &str,
    sub_args: &[&str],
    timeout_ms: Option<u64>,
) -> Result<ExecOutput> {
    let mut args = vec!["-C", repo_or_cwd];
    args.extend_from_slice(sub_args);
    exec_file_async("git", &args, ExecOptions { timeout_ms, ..Default::default() }).await
}

/// Convenience: run a git command with a `cwd` option instead of `-C`.
/// Some operations in `repo.ts` use `{ cwd: repoPath }` instead of `-C`.
pub async fn run_git_cwd(
    cwd: &Path,
    sub_args: &[&str],
    timeout_ms: Option<u64>,
) -> Result<ExecOutput> {
    exec_file_async(
        "git",
        sub_args,
        ExecOptions {
            cwd: Some(cwd),
            timeout_ms,
            ..Default::default()
        },
    )
    .await
}
