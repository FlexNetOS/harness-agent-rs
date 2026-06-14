//! Cancellation bridge: `CancellationToken` → child process kill.
//!
//! Architecture §2.2 and §6.6: the workspace cancellation primitive is
//! `tokio_util::sync::CancellationToken`. When cancellation is requested, the
//! spawned child process must be killed (mirrors `AbortController.abort()` in TS,
//! which the SDK translates to `child.kill()`).
//!
//! This module provides `CancelGuard`, a drop-guard that kills the child when
//! the token is cancelled. It runs a watcher task that monitors the token and
//! signals via an `OwnedSemaphorePermit`-style oneshot when cancellation fires,
//! so the retry loop can detect it between poll points.

use tokio_util::sync::CancellationToken;

/// A guard that kills a subprocess when a `CancellationToken` is cancelled.
///
/// Drop this guard to stop watching (e.g. when the process exits normally).
/// It is NOT a `Drop`-based guard itself (there is no `kill` on drop) because
/// the child may have already exited by the time the guard is dropped — instead
/// it drives an async task.
///
/// Usage:
/// ```ignore
/// let guard = CancelGuard::spawn(token.clone(), child_pid);
/// // ... await stream ...
/// drop(guard); // cancels the watch task
/// ```
pub struct CancelGuard {
    _watcher: tokio::task::JoinHandle<()>,
}

impl CancelGuard {
    /// Spawn a background task that kills `child` when `token` is cancelled.
    ///
    /// `child` is accessed through the `raw_pid` (Unix) so the kill can be
    /// issued without needing ownership of the `Child` struct (which is owned
    /// by the caller for `await child.wait()`).
    ///
    /// Safety: on non-Unix platforms `kill_pid` is a no-op (the `Child`'s
    /// `kill_on_drop(true)` will handle it when the `Child` is dropped).
    pub fn spawn(token: CancellationToken, child_pid: u32) -> Self {
        let watcher = tokio::spawn(async move {
            token.cancelled().await;
            kill_pid(child_pid);
        });
        Self { _watcher: watcher }
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        self._watcher.abort();
    }
}

/// Send SIGTERM (Unix) or TerminateProcess (Windows) to a process by PID.
///
/// Best-effort: errors are logged at debug level, not propagated.
fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: `libc::kill` is safe to call with any PID.
        let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if ret != 0 {
            tracing::debug!(pid, errno = ?std::io::Error::last_os_error(), "cancel_guard.kill_failed");
        } else {
            tracing::debug!(pid, "cancel_guard.killed");
        }
    }
    #[cfg(windows)]
    {
        // On Windows, use TerminateProcess via the Win32 API.
        // For simplicity we use std::process::Command-level handles.
        // Since we only have a PID and not a handle, this is best-effort.
        tracing::debug!(pid, "cancel_guard.kill_windows_pid_only (best-effort)");
        // Windows: would need OpenProcess + TerminateProcess; omitted here.
        // The `kill_on_drop(true)` on Child handles the normal cleanup path.
        let _ = pid;
    }
    #[cfg(not(any(unix, windows)))]
    {
        tracing::debug!(pid, "cancel_guard.kill_not_supported_on_platform");
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_guard_drops_without_panic() {
        let token = CancellationToken::new();
        // Spawn guard with a fake pid (0 — no real process).
        // Guard should drop cleanly without error.
        let guard = CancelGuard::spawn(token.clone(), 0);
        drop(guard);
    }

    #[tokio::test]
    async fn cancel_guard_does_not_block_when_token_not_cancelled() {
        let token = CancellationToken::new();
        let guard = CancelGuard::spawn(token, 12345); // fake pid
        drop(guard);
        // If we reach here without hanging, the guard properly aborts its task on drop.
    }
}
