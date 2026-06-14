/// Isolation error types.
///
/// Ports `packages/isolation/src/errors.ts`.
///
/// `IsolationBlockedError` signals that ALL message handling should stop — the
/// user has already been notified by the time this is thrown.
use thiserror::Error;

/// Reason codes for blocked isolation — currently only `creation_failed`
/// (`IsolationBlockReason` in the source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationBlockReason {
    CreationFailed,
}

/// Thrown when isolation is required but cannot be provided.
/// Ports `IsolationBlockedError` from `errors.ts:9-17`.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct IsolationBlockedError {
    pub message: String,
    pub reason: IsolationBlockReason,
}

impl IsolationBlockedError {
    pub fn new(message: impl Into<String>, reason: IsolationBlockReason) -> Self {
        Self {
            message: message.into(),
            reason,
        }
    }
}

/// Per-pattern classification table.
/// Ports `ERROR_PATTERNS` from `errors.ts:27-111`.
struct ErrorPattern {
    pattern: &'static str,
    message: &'static str,
    known: bool,
}

const ERROR_PATTERNS: &[ErrorPattern] = &[
    ErrorPattern {
        pattern: "permission denied",
        message: "**Error:** Permission denied while creating workspace. Check file system permissions.",
        known: true,
    },
    ErrorPattern {
        pattern: "eacces",
        message: "**Error:** Permission denied while creating workspace. Check file system permissions.",
        known: true,
    },
    ErrorPattern {
        pattern: "timeout",
        message: "**Error:** Timed out creating workspace. Git repository may be slow or unavailable.",
        known: true,
    },
    ErrorPattern {
        pattern: "no space left",
        message: "**Error:** No disk space available for new workspace.",
        known: true,
    },
    ErrorPattern {
        pattern: "enospc",
        message: "**Error:** No disk space available for new workspace.",
        known: true,
    },
    ErrorPattern {
        pattern: "not a git repository",
        message: "**Error:** Target path is not a valid git repository.",
        known: true,
    },
    ErrorPattern {
        // Deliberately NOT `known` — user-input / registration bug, not infra.
        pattern: "cannot extract owner/repo",
        message: "**Error:** Repository path is too short to extract owner and repo name. \
Re-register the codebase with a full path (e.g. `/home/user/owner/repo`).",
        known: false,
    },
    ErrorPattern {
        pattern: "branch not found",
        message: "**Error:** Branch not found. The requested branch may have been deleted or not yet pushed.",
        known: true,
    },
    ErrorPattern {
        pattern: "no base branch configured",
        message: "**Error:** No base branch configured. Set `worktree.baseBranch` in `.archon/config.yaml` \
or use the `--from` flag to select a branch (e.g., `--from dev`).",
        known: true,
    },
    ErrorPattern {
        pattern: "belongs to a different clone",
        message: "**Error:** A worktree at the target path was created by a different local clone. \
Remove it from that clone, or register this codebase from the same local path.",
        known: true,
    },
    ErrorPattern {
        pattern: "cannot verify worktree ownership",
        message: "**Error:** Cannot verify ownership of an existing worktree at the target path. \
Check file system permissions and remove any unrelated git directories at that path.",
        known: true,
    },
    ErrorPattern {
        pattern: "cannot adopt",
        message: "**Error:** Refused to adopt an existing directory at the worktree path. \
Remove it or choose a different branch/codebase registration.",
        known: true,
    },
    ErrorPattern {
        pattern: "submodule initialization failed",
        message: "**Error:** Submodule initialization failed. Check credentials and network access to \
submodule remotes, or set `worktree.initSubmodules: false` in `.archon/config.yaml` \
to opt out if submodules are not needed for your workflows.",
        known: true,
    },
];

/// Classify an isolation creation error into a user-friendly message.
///
/// Ports `classifyIsolationError(err)` from `errors.ts:116-127`.
/// Concatenates `err.message` and any `.stderr` field (via the combined
/// lower-cased string) to match the TypeScript pattern table.
pub fn classify_isolation_error(message: &str, stderr: Option<&str>) -> String {
    let combined = format!("{} {}", message, stderr.unwrap_or("")).to_lowercase();

    for ep in ERROR_PATTERNS {
        if combined.contains(ep.pattern) {
            return ep.message.to_string();
        }
    }

    format!("**Error:** Could not create isolated workspace ({}).", message)
}

/// Returns `true` if the error is a known infrastructure failure (should
/// produce a "blocked" message, not a crash).
///
/// Ports `isKnownIsolationError(err)` from `errors.ts:136-141`.
pub fn is_known_isolation_error(message: &str, stderr: Option<&str>) -> bool {
    let combined = format!("{} {}", message, stderr.unwrap_or("")).to_lowercase();
    ERROR_PATTERNS
        .iter()
        .any(|ep| ep.known && combined.contains(ep.pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_permission_denied() {
        let msg = classify_isolation_error("permission denied: /foo", None);
        assert!(msg.contains("Permission denied"), "got: {msg}");
    }

    #[test]
    fn classify_eacces() {
        let msg = classify_isolation_error("EACCES", None);
        assert!(msg.contains("Permission denied"), "got: {msg}");
    }

    #[test]
    fn classify_timeout() {
        let msg = classify_isolation_error("timeout waiting for git", None);
        assert!(msg.contains("Timed out"), "got: {msg}");
    }

    #[test]
    fn classify_no_space_left() {
        let msg = classify_isolation_error("no space left on device", None);
        assert!(msg.contains("No disk space"), "got: {msg}");
    }

    #[test]
    fn classify_enospc() {
        let msg = classify_isolation_error("enospc error", None);
        assert!(msg.contains("No disk space"), "got: {msg}");
    }

    #[test]
    fn classify_not_a_git_repository() {
        let msg = classify_isolation_error("not a git repository", None);
        assert!(msg.contains("not a valid git repository"), "got: {msg}");
    }

    #[test]
    fn classify_cannot_extract_owner_repo() {
        let msg = classify_isolation_error("cannot extract owner/repo from /x", None);
        assert!(msg.contains("Repository path is too short"), "got: {msg}");
    }

    #[test]
    fn classify_branch_not_found() {
        let msg = classify_isolation_error("branch not found: feature", None);
        assert!(msg.contains("Branch not found"), "got: {msg}");
    }

    #[test]
    fn classify_belongs_to_different_clone() {
        let msg = classify_isolation_error("belongs to a different clone", None);
        assert!(msg.contains("different local clone"), "got: {msg}");
    }

    #[test]
    fn classify_submodule_failed() {
        let msg = classify_isolation_error("submodule initialization failed", None);
        assert!(msg.contains("Submodule initialization failed"), "got: {msg}");
    }

    #[test]
    fn classify_unknown_falls_back() {
        let msg = classify_isolation_error("some unexpected error xyz", None);
        assert!(
            msg.starts_with("**Error:** Could not create isolated workspace"),
            "got: {msg}"
        );
        assert!(msg.contains("some unexpected error xyz"), "got: {msg}");
    }

    #[test]
    fn classify_uses_stderr_field() {
        // pattern in stderr, not in message
        let msg = classify_isolation_error("git failed", Some("permission denied: /repo"));
        assert!(msg.contains("Permission denied"), "got: {msg}");
    }

    #[test]
    fn is_known_permission_denied_true() {
        assert!(is_known_isolation_error("permission denied", None));
    }

    #[test]
    fn is_known_cannot_extract_false() {
        // "cannot extract owner/repo" is `known: false`
        assert!(!is_known_isolation_error("cannot extract owner/repo", None));
    }

    #[test]
    fn is_known_unknown_error_false() {
        assert!(!is_known_isolation_error("some mysterious error", None));
    }

    #[test]
    fn isolation_blocked_error_fields() {
        let err = IsolationBlockedError::new("workspace limit reached", IsolationBlockReason::CreationFailed);
        assert_eq!(err.to_string(), "workspace limit reached");
        assert_eq!(err.reason, IsolationBlockReason::CreationFailed);
    }
}
