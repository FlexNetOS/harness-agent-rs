use chrono::{DateTime, Utc};
/// Isolation provider abstraction types.
///
/// Ports `packages/isolation/src/types.ts`.
///
/// ## Discriminant strategy for `IsolationRequest`
///
/// The TypeScript source uses STRUCTURAL discrimination (no explicit `type`/`kind`
/// tag field): each variant extends a shared `IsolationRequestBase` and adds
/// `workflowType: '<literal>'`. All five variants share the same base fields but
/// are distinguished purely by `workflowType`. We mirror this with
/// `#[serde(tag = "workflowType")]` on the `IsolationRequest` enum, giving
/// JSON shapes like `{"workflowType":"issue","codebaseId":"...","identifier":"..."}`.
///
/// Wire field names are camelCase to match the TypeScript source exactly.
use serde::{Deserialize, Serialize};

// ─── Simple string-union enums ─────────────────────────────────────────────

/// `'worktree' | 'container' | 'vm' | 'remote'`
/// Source: `types.ts:13`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IsolationProviderType {
    Worktree,
    Container,
    Vm,
    Remote,
}

/// `'issue' | 'pr' | 'review' | 'thread' | 'task'`
/// Source: `types.ts:15`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IsolationWorkflowType {
    Issue,
    Pr,
    Review,
    Thread,
    Task,
}

/// `'active' | 'destroyed'`
/// Source: `types.ts:17`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentStatus {
    Active,
    Destroyed,
}

// ─── Git identity ─────────────────────────────────────────────────────────

/// Optional git author identity to stamp on the new worktree.
/// Source: `types.ts:54` (`{ email: string; name?: string }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitIdentity {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// ─── IsolationRequest discriminated union ─────────────────────────────────

/// Shared base fields present in every `IsolationRequest` variant.
/// Source: `IsolationRequestBase` at `types.ts:21-55`.
///
/// Wire names are camelCase (matching TS source).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationRequestBase {
    /// Database ID for the codebase.
    #[serde(rename = "codebaseId")]
    pub codebase_id: String,

    /// Codebase name in "owner/repo" format.
    #[serde(rename = "codebaseName", skip_serializing_if = "Option::is_none")]
    pub codebase_name: Option<String>,

    /// Absolute, resolved filesystem path to the main repository checkout.
    #[serde(rename = "canonicalRepoPath")]
    pub canonical_repo_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "gitIdentity", skip_serializing_if = "Option::is_none")]
    pub git_identity: Option<GitIdentity>,
}

/// `IsolationRequest` discriminated union — tagged on `workflowType`.
///
/// Source: `types.ts:57-97` (`IssueIsolationRequest | PRIsolationRequest |
/// ReviewIsolationRequest | ThreadIsolationRequest | TaskIsolationRequest`).
///
/// Serde strategy: `#[serde(tag = "workflowType")]` mirrors the TS structural
/// union discriminated by the `workflowType` literal field on each interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "workflowType", rename_all = "lowercase")]
pub enum IsolationRequest {
    /// `IssueIsolationRequest`: `workflowType: 'issue'`, plus `identifier`.
    Issue {
        #[serde(flatten)]
        base: IsolationRequestBase,
        identifier: String,
    },

    /// `PRIsolationRequest`: `workflowType: 'pr'`, plus `identifier`,
    /// `prBranch`, `prSha?`, `isForkPR`.
    /// Source: `types.ts:62-71`.
    #[serde(rename = "pr")]
    Pr {
        #[serde(flatten)]
        base: IsolationRequestBase,
        identifier: String,
        #[serde(rename = "prBranch")]
        pr_branch: String,
        #[serde(rename = "prSha", skip_serializing_if = "Option::is_none")]
        pr_sha: Option<String>,
        #[serde(rename = "isForkPR")]
        is_fork_pr: bool,
    },

    /// `ReviewIsolationRequest`: `workflowType: 'review'`, plus `identifier`.
    Review {
        #[serde(flatten)]
        base: IsolationRequestBase,
        identifier: String,
    },

    /// `ThreadIsolationRequest`: `workflowType: 'thread'`, plus `identifier`.
    Thread {
        #[serde(flatten)]
        base: IsolationRequestBase,
        identifier: String,
    },

    /// `TaskIsolationRequest`: `workflowType: 'task'`, plus `identifier`,
    /// `fromBranch?`. Source: `types.ts:84-90`.
    Task {
        #[serde(flatten)]
        base: IsolationRequestBase,
        identifier: String,
        #[serde(rename = "fromBranch", skip_serializing_if = "Option::is_none")]
        from_branch: Option<String>,
    },
}

impl IsolationRequest {
    /// Get the shared base fields regardless of variant.
    pub fn base(&self) -> &IsolationRequestBase {
        match self {
            IsolationRequest::Issue { base, .. } => base,
            IsolationRequest::Pr { base, .. } => base,
            IsolationRequest::Review { base, .. } => base,
            IsolationRequest::Thread { base, .. } => base,
            IsolationRequest::Task { base, .. } => base,
        }
    }

    /// Get the `workflowType` as the enum.
    pub fn workflow_type(&self) -> IsolationWorkflowType {
        match self {
            IsolationRequest::Issue { .. } => IsolationWorkflowType::Issue,
            IsolationRequest::Pr { .. } => IsolationWorkflowType::Pr,
            IsolationRequest::Review { .. } => IsolationWorkflowType::Review,
            IsolationRequest::Thread { .. } => IsolationWorkflowType::Thread,
            IsolationRequest::Task { .. } => IsolationWorkflowType::Task,
        }
    }

    /// Get the `identifier` field (present on all variants).
    pub fn identifier(&self) -> &str {
        match self {
            IsolationRequest::Issue { identifier, .. } => identifier,
            IsolationRequest::Pr { identifier, .. } => identifier,
            IsolationRequest::Review { identifier, .. } => identifier,
            IsolationRequest::Thread { identifier, .. } => identifier,
            IsolationRequest::Task { identifier, .. } => identifier,
        }
    }
}

/// Type guard: `isPRIsolationRequest`.
/// Source: `types.ts:200-202`.
pub fn is_pr_isolation_request(request: &IsolationRequest) -> bool {
    matches!(request, IsolationRequest::Pr { .. })
}

// ─── Worktree metadata ────────────────────────────────────────────────────

/// Source of adoption. Source: `types.ts:103`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdoptedFrom {
    Path,
    Branch,
}

/// Metadata for an adopted worktree. Source: `types.ts:101-105`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptedWorktreeMetadata {
    pub adopted: bool, // always true
    #[serde(rename = "adoptedFrom", skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<AdoptedFrom>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<IsolationRequest>,
}

/// Metadata for a created (non-adopted) worktree. Source: `types.ts:107-110`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedWorktreeMetadata {
    pub adopted: bool, // always false
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<IsolationRequest>,
}

/// Union of adopted / created worktree metadata.
/// Source: `types.ts:112`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorktreeMetadata {
    Adopted(AdoptedWorktreeMetadata),
    Created(CreatedWorktreeMetadata),
}

// ─── Isolated environment ─────────────────────────────────────────────────

/// `WorktreeEnvironment` — the only concrete `IsolatedEnvironment` type.
/// Source: `types.ts:128-133`.
///
/// `createdAt` is a JS `Date` in source → `chrono::DateTime<Utc>` here
/// (same `- [≠]` as WF-06: JSON has no Date type, behavior preserved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeEnvironment {
    pub id: String,
    #[serde(rename = "workingPath")]
    pub working_path: String,
    pub status: EnvironmentStatus,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    /// Always `"worktree"`. Source: `types.ts:129`.
    pub provider: String,
    #[serde(rename = "branchName")]
    pub branch_name: String,
    pub metadata: WorktreeMetadata,
}

// ─── Destroy types ────────────────────────────────────────────────────────

/// Options for `destroy()`.
/// Source: `types.ts:138-148`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DestroyOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
    /// `WorktreeDestroyOptions` extension: branch name to delete.
    #[serde(rename = "branchName", skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    /// Canonical repo path (required for branch cleanup if worktree path gone).
    #[serde(rename = "canonicalRepoPath", skip_serializing_if = "Option::is_none")]
    pub canonical_repo_path: Option<String>,
    /// Delete the remote branch (best-effort).
    #[serde(rename = "deleteRemoteBranch", skip_serializing_if = "Option::is_none")]
    pub delete_remote_branch: Option<bool>,
}

/// Result of a best-effort `destroy()` call.
/// Source: `types.ts:154-162`.
///
/// `branch_deleted: Option<bool>` — `null` means "no branch specified".
/// `remote_branch_deleted: Option<bool>` — `null` means "not requested".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestroyResult {
    #[serde(rename = "worktreeRemoved")]
    pub worktree_removed: bool,
    /// `null` = no branch specified.
    #[serde(rename = "branchDeleted")]
    pub branch_deleted: Option<bool>,
    /// `null` = not requested.
    #[serde(rename = "remoteBranchDeleted")]
    pub remote_branch_deleted: Option<bool>,
    #[serde(rename = "directoryClean")]
    pub directory_clean: bool,
    pub warnings: Vec<String>,
}

// ─── IIsolationProvider trait ─────────────────────────────────────────────

/// Provider interface for isolation strategies.
///
/// Ports `IIsolationProvider` from `types.ts:177-196`.
///
/// `adopt` is optional in TS (marked `adopt?`); here it has a default impl
/// returning `Ok(None)` to preserve optional semantics without `Option<fn>`.
#[async_trait::async_trait]
pub trait IsolationProvider: Send + Sync {
    fn provider_type(&self) -> IsolationProviderType;

    async fn create(&self, request: IsolationRequest) -> crate::Result<WorktreeEnvironment>;

    /// Best-effort cleanup. Returns `DestroyResult` with partial-failure details.
    async fn destroy(
        &self,
        env_id: &str,
        options: Option<DestroyOptions>,
    ) -> crate::Result<DestroyResult>;

    /// Returns `None` if not found; errors only on unexpected I/O failures.
    async fn get(&self, env_id: &str) -> crate::Result<Option<WorktreeEnvironment>>;

    /// For worktrees, `codebase_id` is the canonical repo path.
    async fn list(&self, codebase_id: &str) -> crate::Result<Vec<WorktreeEnvironment>>;

    /// Take ownership of externally-created environments (optional).
    /// Default: `Ok(None)`.
    async fn adopt(&self, _path: &str) -> crate::Result<Option<WorktreeEnvironment>> {
        Ok(None)
    }

    async fn health_check(&self, env_id: &str) -> crate::Result<bool>;
}

// ─── Isolation hints & block reason ───────────────────────────────────────

/// Hints passed into the resolver for richer resolution decisions.
/// Source: `types.ts:206-229`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IsolationHints {
    #[serde(rename = "workflowType", skip_serializing_if = "Option::is_none")]
    pub workflow_type: Option<IsolationWorkflowType>,
    #[serde(rename = "workflowId", skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,

    // PR-specific
    #[serde(rename = "prBranch", skip_serializing_if = "Option::is_none")]
    pub pr_branch: Option<String>,
    #[serde(rename = "prSha", skip_serializing_if = "Option::is_none")]
    pub pr_sha: Option<String>,
    #[serde(rename = "isForkPR", skip_serializing_if = "Option::is_none")]
    pub is_fork_pr: Option<bool>,
    #[serde(rename = "prFetchFailed", skip_serializing_if = "Option::is_none")]
    pub pr_fetch_failed: Option<bool>,

    // Task-specific
    #[serde(rename = "fromBranch", skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,

    #[serde(rename = "baseBranch", skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,

    // Cross-reference hints
    #[serde(rename = "linkedIssues", skip_serializing_if = "Option::is_none")]
    pub linked_issues: Option<Vec<u32>>,
    #[serde(rename = "linkedPRs", skip_serializing_if = "Option::is_none")]
    pub linked_prs: Option<Vec<u32>>,

    // Adoption hints
    #[serde(rename = "suggestedBranch", skip_serializing_if = "Option::is_none")]
    pub suggested_branch: Option<String>,
}

// ─── Database row type ────────────────────────────────────────────────────

/// Mirrors the database row shape for an isolation environment.
/// Source: `types.ts:235-249`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationEnvironmentRow {
    pub id: String,
    pub codebase_id: String,
    pub workflow_type: IsolationWorkflowType,
    pub workflow_id: String,
    pub provider: IsolationProviderType,
    pub working_path: String,
    pub branch_name: String,
    pub status: EnvironmentStatus,
    pub created_at: DateTime<Utc>,
    pub created_by_platform: Option<String>,
    pub created_by_user_id: Option<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

// ─── Config injection ─────────────────────────────────────────────────────

/// Per-repo worktree configuration.
/// Source: `types.ts:253-275`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorktreeCreateConfig {
    #[serde(rename = "baseBranch", skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(rename = "copyFiles", skip_serializing_if = "Option::is_none")]
    pub copy_files: Option<Vec<String>>,
    #[serde(rename = "initSubmodules", skip_serializing_if = "Option::is_none")]
    pub init_submodules: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Function signature for async repo config loader.
/// Source: `types.ts:277`.
///
/// Returns `None` when no config is found for the repo path.
pub type RepoConfigLoader = std::sync::Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<WorktreeCreateConfig>> + Send>,
        > + Send
        + Sync,
>;

// ─── Worktree status breakdown ────────────────────────────────────────────

/// Detailed worktree status for a codebase.
/// Source: `types.ts:285-293`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeStatusBreakdown {
    pub total: u32,
    pub merged: u32,
    pub stale: u32,
    pub active: u32,
    #[serde(rename = "mergedEnvs")]
    pub merged_envs: Vec<EnvSummary>,
    #[serde(rename = "staleEnvs")]
    pub stale_envs: Vec<StaleEnvSummary>,
    #[serde(rename = "activeEnvs")]
    pub active_envs: Vec<EnvSummary>,
}

/// Minimal env identifier for status breakdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvSummary {
    pub id: String,
    #[serde(rename = "branchName")]
    pub branch_name: String,
}

/// Stale env with age info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaleEnvSummary {
    pub id: String,
    #[serde(rename = "branchName")]
    pub branch_name: String,
    #[serde(rename = "daysInactive")]
    pub days_inactive: u32,
}

// ─── Store params ─────────────────────────────────────────────────────────

/// Parameters for creating a new isolation environment record.
/// Source: `types.ts:297-309`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEnvironmentParams {
    pub codebase_id: String,
    pub workflow_type: IsolationWorkflowType,
    pub workflow_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<IsolationProviderType>,
    pub working_path: String,
    pub branch_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

// ─── Resolver types ───────────────────────────────────────────────────────

/// Request to the isolation resolver.
/// Source: `types.ts:312-329`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRequest {
    #[serde(rename = "existingEnvId")]
    pub existing_env_id: Option<String>,
    pub codebase: Option<CodebaseSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<IsolationHints>,
    #[serde(rename = "platformType")]
    pub platform_type: String,
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(rename = "gitIdentity", skip_serializing_if = "Option::is_none")]
    pub git_identity: Option<GitIdentity>,
}

/// Minimal codebase info for resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseSummary {
    pub id: String,
    #[serde(rename = "defaultCwd")]
    pub default_cwd: String,
    pub name: String,
}

/// How the resolution was achieved.
/// Source: `types.ts:331-336`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolutionMethod {
    Existing,
    WorkflowReuse,
    LinkedIssueReuse {
        #[serde(rename = "issueNumber")]
        issue_number: u32,
    },
    BranchAdoption {
        branch: String,
    },
    Created {
        #[serde(rename = "autoCleanedCount", skip_serializing_if = "Option::is_none")]
        auto_cleaned_count: Option<u32>,
    },
}

/// Resolved variant payload (boxed to reduce enum size).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPayload {
    pub env: IsolationEnvironmentRow,
    pub cwd: String,
    pub method: ResolutionMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

/// Result of an isolation resolution attempt.
/// Source: `types.ts:338-348`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IsolationResolution {
    Resolved(Box<ResolvedPayload>),
    StaleCleaned {
        #[serde(rename = "previousEnvId")]
        previous_env_id: String,
    },
    None {
        cwd: String,
    },
    Blocked {
        reason: String, // IsolationBlockReason as string
        #[serde(rename = "userMessage")]
        user_message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── IsolationProviderType ───────────────────────────────────────────────

    #[test]
    fn isolation_provider_type_wire_names() {
        assert_eq!(
            serde_json::to_string(&IsolationProviderType::Worktree).unwrap(),
            r#""worktree""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationProviderType::Container).unwrap(),
            r#""container""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationProviderType::Vm).unwrap(),
            r#""vm""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationProviderType::Remote).unwrap(),
            r#""remote""#
        );
    }

    #[test]
    fn isolation_provider_type_deserializes_all() {
        let v: IsolationProviderType = serde_json::from_str(r#""worktree""#).unwrap();
        assert_eq!(v, IsolationProviderType::Worktree);
        let v: IsolationProviderType = serde_json::from_str(r#""container""#).unwrap();
        assert_eq!(v, IsolationProviderType::Container);
        let v: IsolationProviderType = serde_json::from_str(r#""vm""#).unwrap();
        assert_eq!(v, IsolationProviderType::Vm);
        let v: IsolationProviderType = serde_json::from_str(r#""remote""#).unwrap();
        assert_eq!(v, IsolationProviderType::Remote);
    }

    // ─── IsolationWorkflowType ───────────────────────────────────────────────

    #[test]
    fn isolation_workflow_type_wire_names() {
        assert_eq!(
            serde_json::to_string(&IsolationWorkflowType::Issue).unwrap(),
            r#""issue""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationWorkflowType::Pr).unwrap(),
            r#""pr""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationWorkflowType::Review).unwrap(),
            r#""review""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationWorkflowType::Thread).unwrap(),
            r#""thread""#
        );
        assert_eq!(
            serde_json::to_string(&IsolationWorkflowType::Task).unwrap(),
            r#""task""#
        );
    }

    // ─── EnvironmentStatus ───────────────────────────────────────────────────

    #[test]
    fn environment_status_wire_names() {
        assert_eq!(
            serde_json::to_string(&EnvironmentStatus::Active).unwrap(),
            r#""active""#
        );
        assert_eq!(
            serde_json::to_string(&EnvironmentStatus::Destroyed).unwrap(),
            r#""destroyed""#
        );
    }

    // ─── IsolationRequest discriminated union ────────────────────────────────

    #[test]
    fn issue_request_round_trip() {
        let json = json!({
            "workflowType": "issue",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "42"
        });
        let req: IsolationRequest = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(req, IsolationRequest::Issue { .. }));
        assert_eq!(req.identifier(), "42");
        assert_eq!(req.workflow_type(), IsolationWorkflowType::Issue);
        let back = serde_json::to_value(&req).unwrap();
        assert_eq!(back["workflowType"], "issue");
        assert_eq!(back["identifier"], "42");
    }

    #[test]
    fn pr_request_round_trip() {
        let json = json!({
            "workflowType": "pr",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "99",
            "prBranch": "feat/new-feature",
            "prSha": "abc123",
            "isForkPR": true
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        match &req {
            IsolationRequest::Pr {
                pr_branch,
                is_fork_pr,
                pr_sha,
                ..
            } => {
                assert_eq!(pr_branch, "feat/new-feature");
                assert_eq!(is_fork_pr, &true);
                assert_eq!(pr_sha.as_deref(), Some("abc123"));
            }
            _ => panic!("expected PR variant"),
        }
        // type guard
        assert!(is_pr_isolation_request(&req));
    }

    #[test]
    fn pr_request_without_sha() {
        let json = json!({
            "workflowType": "pr",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "1",
            "prBranch": "main",
            "isForkPR": false
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        match &req {
            IsolationRequest::Pr { pr_sha, .. } => assert!(pr_sha.is_none()),
            _ => panic!("expected PR"),
        }
    }

    #[test]
    fn review_request_round_trip() {
        let json = json!({
            "workflowType": "review",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "77"
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        assert!(matches!(req, IsolationRequest::Review { .. }));
        assert!(!is_pr_isolation_request(&req));
    }

    #[test]
    fn thread_request_round_trip() {
        let json = json!({
            "workflowType": "thread",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "thread-55"
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        assert!(matches!(req, IsolationRequest::Thread { .. }));
    }

    #[test]
    fn task_request_with_from_branch() {
        let json = json!({
            "workflowType": "task",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "task-abc",
            "fromBranch": "develop"
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        match &req {
            IsolationRequest::Task { from_branch, .. } => {
                assert_eq!(from_branch.as_deref(), Some("develop"));
            }
            _ => panic!("expected Task"),
        }
    }

    #[test]
    fn task_request_without_from_branch() {
        let json = json!({
            "workflowType": "task",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "identifier": "task-xyz"
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        match &req {
            IsolationRequest::Task { from_branch, .. } => assert!(from_branch.is_none()),
            _ => panic!("expected Task"),
        }
    }

    #[test]
    fn isolation_request_base_optional_fields() {
        let json = json!({
            "workflowType": "issue",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/home/user/owner/repo",
            "codebaseName": "owner/repo",
            "description": "A workflow",
            "gitIdentity": { "email": "user@github.noreply.com", "name": "User" },
            "identifier": "5"
        });
        let req: IsolationRequest = serde_json::from_value(json).unwrap();
        let base = req.base();
        assert_eq!(base.codebase_name.as_deref(), Some("owner/repo"));
        assert_eq!(base.description.as_deref(), Some("A workflow"));
        assert!(base.git_identity.is_some());
    }

    #[test]
    fn unknown_workflow_type_rejects() {
        let json = json!({
            "workflowType": "unknown-type",
            "codebaseId": "cdb-001",
            "canonicalRepoPath": "/repo",
            "identifier": "1"
        });
        let result: Result<IsolationRequest, _> = serde_json::from_value(json);
        assert!(result.is_err(), "unknown workflowType should reject");
    }

    // ─── is_pr_isolation_request type guard ─────────────────────────────────

    #[test]
    fn type_guard_issue_false() {
        let req = IsolationRequest::Issue {
            base: IsolationRequestBase {
                codebase_id: "x".into(),
                codebase_name: None,
                canonical_repo_path: "/r".into(),
                description: None,
                git_identity: None,
            },
            identifier: "1".into(),
        };
        assert!(!is_pr_isolation_request(&req));
    }

    #[test]
    fn type_guard_pr_true() {
        let req = IsolationRequest::Pr {
            base: IsolationRequestBase {
                codebase_id: "x".into(),
                codebase_name: None,
                canonical_repo_path: "/r".into(),
                description: None,
                git_identity: None,
            },
            identifier: "1".into(),
            pr_branch: "feat".into(),
            pr_sha: None,
            is_fork_pr: false,
        };
        assert!(is_pr_isolation_request(&req));
    }

    // ─── DestroyResult ───────────────────────────────────────────────────────

    #[test]
    fn destroy_result_null_fields_are_none() {
        let json = json!({
            "worktreeRemoved": true,
            "branchDeleted": null,
            "remoteBranchDeleted": null,
            "directoryClean": true,
            "warnings": []
        });
        let dr: DestroyResult = serde_json::from_value(json).unwrap();
        assert!(dr.worktree_removed);
        assert!(dr.branch_deleted.is_none());
        assert!(dr.remote_branch_deleted.is_none());
        assert!(dr.directory_clean);
        assert!(dr.warnings.is_empty());
    }

    #[test]
    fn destroy_result_bool_fields() {
        let dr = DestroyResult {
            worktree_removed: true,
            branch_deleted: Some(true),
            remote_branch_deleted: Some(false),
            directory_clean: true,
            warnings: vec!["minor issue".to_string()],
        };
        let v = serde_json::to_value(&dr).unwrap();
        assert_eq!(v["worktreeRemoved"], true);
        assert_eq!(v["branchDeleted"], true);
        assert_eq!(v["remoteBranchDeleted"], false);
    }

    // ─── ResolutionMethod ────────────────────────────────────────────────────

    #[test]
    fn resolution_method_existing() {
        let m = ResolutionMethod::Existing;
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], "existing");
    }

    #[test]
    fn resolution_method_created_with_count() {
        let m = ResolutionMethod::Created {
            auto_cleaned_count: Some(3),
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], "created");
        assert_eq!(v["autoCleanedCount"], 3);
    }

    #[test]
    fn resolution_method_linked_issue() {
        let m = ResolutionMethod::LinkedIssueReuse { issue_number: 42 };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], "linked_issue_reuse");
        assert_eq!(v["issueNumber"], 42);
    }

    // ─── IsolationResolution ─────────────────────────────────────────────────

    #[test]
    fn isolation_resolution_none_variant() {
        let r = IsolationResolution::None {
            cwd: "/tmp/work".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "none");
        assert_eq!(v["cwd"], "/tmp/work");
    }

    #[test]
    fn isolation_resolution_blocked_variant() {
        let r = IsolationResolution::Blocked {
            reason: "creation_failed".into(),
            user_message: "**Error:** Permission denied".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "blocked");
        assert_eq!(v["reason"], "creation_failed");
        assert!(v["userMessage"]
            .as_str()
            .unwrap()
            .contains("Permission denied"));
    }

    #[test]
    fn isolation_resolution_stale_cleaned() {
        let r = IsolationResolution::StaleCleaned {
            previous_env_id: "env-123".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "stale_cleaned");
        assert_eq!(v["previousEnvId"], "env-123");
    }
}
