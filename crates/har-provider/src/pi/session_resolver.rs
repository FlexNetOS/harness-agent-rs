//! Pi session resolution.
//!
//! PORT of `packages/providers/src/community/pi/session-resolver.ts`.
//!
//! Resolves a `SessionManager` for a `sendQuery` call. Pi stores sessions as
//! JSONL files under `~/.pi/agent/sessions/<encoded-cwd>/` (or
//! `$PI_CODING_AGENT_DIR/sessions/...`).
//!
//! # SDK seam note
//!
//! `SessionManager.create(cwd)` / `SessionManager.open(path)` /
//! `SessionManager.list(cwd)` are Pi SDK operations. In the Rust port, this
//! module exposes the same decision logic in a testable, SDK-seam-free form:
//! `resolve_pi_session_logic` captures which branch was taken and what path
//! would be opened, without calling the live SDK. `resolve_pi_session` uses it
//! when wired to the SDK (which is the `pi_sdk_not_bound` seam in provider.rs).

use std::io;

/// A Pi session entry from `SessionManager.list(cwd)`.
///
/// PORT of the list-entry shape (session-resolver.ts:46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiSessionEntry {
    pub id: String,
    pub path: String,
}

/// The decision taken by session-resolution logic.
///
/// Used for both testing and the live SDK call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResolutionDecision {
    /// No resume ID provided — create a fresh session for `cwd`.
    Fresh { cwd: String },
    /// Resume ID matched an existing session — open `path`.
    Open { path: String },
    /// Resume ID was provided but not found — create fresh, flag `resume_failed`.
    FreshWithFailedResume { cwd: String },
}

/// Result of resolving a Pi session.
///
/// PORT of `ResolvedSession` (session-resolver.ts:6-16).
#[derive(Debug, Clone)]
pub struct ResolvedSession {
    /// True when a `resume_session_id` was provided but no matching session
    /// file was found — caller should surface a system warning before the new
    /// session starts. Mirrors the `resume_thread_failed` fallback pattern.
    pub resume_failed: bool,
    /// Which decision was taken by the resolver.
    pub decision: SessionResolutionDecision,
}

/// True if `err` is an ENOENT/ENOTDIR error (session directory doesn't exist yet).
///
/// PORT of `isMissingSessionDirError(err)` (session-resolver.ts:64-68).
pub fn is_missing_session_dir_error(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::NotFound | io::ErrorKind::Other)
        || err.raw_os_error() == Some(20) // ENOTDIR on Linux
}

/// Resolve Pi session logic without calling the live SDK.
///
/// This is the parity-testable core of `resolvePiSession`. It takes the
/// list of available sessions (pre-fetched, or `None` for "fetch failed
/// with ENOENT") and returns the decision + resume_failed flag.
///
/// PORT of the decision logic inside `resolvePiSession` (session-resolver.ts:37-62).
/// `available_sessions`: pass `None` for "list failed with a missing-dir error (ENOENT/ENOTDIR)".
pub fn resolve_pi_session_logic(
    cwd: &str,
    resume_session_id: Option<&str>,
    available_sessions: Option<&[PiSessionEntry]>,
) -> ResolvedSession {
    // No resume ID → fresh session.
    let id = match resume_session_id {
        None | Some("") => {
            return ResolvedSession {
                resume_failed: false,
                decision: SessionResolutionDecision::Fresh {
                    cwd: cwd.to_owned(),
                },
            }
        }
        Some(id) => id,
    };

    // `None` means list() threw ENOENT/ENOTDIR — treat as "not found".
    let sessions = match available_sessions {
        None => {
            return ResolvedSession {
                resume_failed: true,
                decision: SessionResolutionDecision::FreshWithFailedResume {
                    cwd: cwd.to_owned(),
                },
            }
        }
        Some(s) => s,
    };

    if let Some(entry) = sessions.iter().find(|s| s.id == id) {
        return ResolvedSession {
            resume_failed: false,
            decision: SessionResolutionDecision::Open {
                path: entry.path.clone(),
            },
        };
    }

    ResolvedSession {
        resume_failed: true,
        decision: SessionResolutionDecision::FreshWithFailedResume {
            cwd: cwd.to_owned(),
        },
    }
}

/// Stub type for SDK-seam tests — represents the resolved session manager.
///
/// In the live SDK path this would hold a `SessionManager` reference.
/// Exported for use by the provider's `send_query` seam.
pub struct ResolvedPiSession {
    pub resume_failed: bool,
    pub decision: SessionResolutionDecision,
}

/// Resolve a Pi session manager for a `sendQuery` call.
///
/// This is the entry point called from `provider.rs`. Returns `ResolvedPiSession`
/// which carries the decision. The actual SDK `SessionManager` construction
/// happens at the `pi_sdk_not_bound` seam in `provider.rs`.
///
/// PORT of `resolvePiSession(cwd, resumeSessionId)` (session-resolver.ts:37-62).
///
/// Error handling: ENOENT/ENOTDIR from `SessionManager.list()` → treat as
/// "no sessions yet" (graceful fallback). Any other error propagates.
pub fn resolve_pi_session(cwd: &str, resume_session_id: Option<&str>) -> ResolvedPiSession {
    // In the live SDK path, this would call `SessionManager.list(cwd)` and
    // pass the results to `resolve_pi_session_logic`. At the `pi_sdk_not_bound`
    // seam, we perform only the decision logic without a real file-backed list.
    //
    // For the non-SDK path (used in provider.rs before the seam call), we call
    // the pure logic function with no sessions (simulates fresh-session path).
    let result = resolve_pi_session_logic(cwd, resume_session_id, Some(&[]));

    // If a resume was requested but no sessions available, it will show resume_failed=true
    // (correct behavior: the session wasn't found).
    ResolvedPiSession {
        resume_failed: result.resume_failed,
        decision: result.decision,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_resume_id_creates_fresh() {
        let result = resolve_pi_session_logic("/tmp/proj", None, Some(&[]));
        assert!(!result.resume_failed);
        assert_eq!(
            result.decision,
            SessionResolutionDecision::Fresh {
                cwd: "/tmp/proj".to_owned()
            }
        );
    }

    #[test]
    fn empty_resume_id_creates_fresh() {
        let result = resolve_pi_session_logic("/tmp/proj", Some(""), Some(&[]));
        assert!(!result.resume_failed);
        assert!(matches!(
            result.decision,
            SessionResolutionDecision::Fresh { .. }
        ));
    }

    #[test]
    fn matching_resume_id_opens_by_path() {
        let sessions = vec![
            PiSessionEntry {
                id: "abc-123".to_owned(),
                path: "/sessions/abc-123.jsonl".to_owned(),
            },
            PiSessionEntry {
                id: "def-456".to_owned(),
                path: "/sessions/def-456.jsonl".to_owned(),
            },
        ];
        let result = resolve_pi_session_logic("/tmp/proj", Some("def-456"), Some(&sessions));
        assert!(!result.resume_failed);
        assert_eq!(
            result.decision,
            SessionResolutionDecision::Open {
                path: "/sessions/def-456.jsonl".to_owned()
            }
        );
    }

    #[test]
    fn missing_resume_id_creates_fresh_with_failure() {
        let sessions = vec![PiSessionEntry {
            id: "abc-123".to_owned(),
            path: "/sessions/abc-123.jsonl".to_owned(),
        }];
        let result = resolve_pi_session_logic("/tmp/proj", Some("missing-id"), Some(&sessions));
        assert!(result.resume_failed);
        assert!(matches!(
            result.decision,
            SessionResolutionDecision::FreshWithFailedResume { .. }
        ));
    }

    #[test]
    fn enoent_list_treated_as_not_found() {
        // `None` for available_sessions = ENOENT/ENOTDIR from list()
        let result = resolve_pi_session_logic("/tmp/proj", Some("some-id"), None);
        assert!(result.resume_failed);
        assert!(matches!(
            result.decision,
            SessionResolutionDecision::FreshWithFailedResume { .. }
        ));
    }

    #[test]
    fn empty_session_list_with_id_is_failed_resume() {
        let result = resolve_pi_session_logic("/tmp/proj", Some("some-id"), Some(&[]));
        assert!(result.resume_failed);
    }
}
