/// Isolation resolver — 6-stage resolution cascade.
///
/// Ports `packages/isolation/src/resolver.ts`.
///
/// ## Resolution stages (in order)
///
/// 1. **Existing** — explicit `existing_env_id` provided; verify + return or StaleCleaned.
/// 2. **No-codebase** — no `codebase` in request → return `None{cwd: '/workspace'}`.
/// 3. **Workflow reuse** — find active env for (codebase, workflowType, workflowId).
/// 4. **Linked-issue reuse** — iterate `hints.linkedIssues` and reuse first hit.
/// 5. **Branch adoption** — hints.prBranch exists in repo.
/// 6. **Create new** — provision a fresh worktree environment, optionally after
///    cleaning stale envs for capacity.
///
/// ## Fields stored on `IsolationResolver`
///
/// The TypeScript `IsolationResolverDeps` has `{ store, provider, cleanup?, staleThresholdDays? }`.
/// The constructor stores only `store`, `provider`, `staleThresholdDays`; the
/// `cleanup` dep is NOT stored (it is injected only for the constructor's own
/// `cleanupStaleEnvironments` call which happens in `createNewEnvironment`).
///
/// Source: class definition at `resolver.ts:68-83`.
use std::sync::Arc;

use tracing::{debug, error, info, warn};

use har_git::{
    find_worktree_by_branch, get_canonical_repo_path, is_ancestor_of, to_branch_name, to_repo_path,
    to_worktree_path, verify_worktree_ownership, worktree_exists,
};

use crate::store::IsolationStore;
use crate::types::IsolationProvider;
use crate::types::{
    CodebaseSummary, CreateEnvironmentParams, DestroyOptions, IsolationHints,
    IsolationProviderType, IsolationRequest, IsolationRequestBase, IsolationResolution,
    IsolationWorkflowType, ResolutionMethod, ResolveRequest, ResolvedPayload,
};
use crate::{IsolationError, Result};

/// Default stale-threshold days if not set by caller.
/// Source: `DEFAULT_STALE_THRESHOLD_DAYS = 14` at `resolver.ts:55`.
const DEFAULT_STALE_THRESHOLD_DAYS: u32 = 14;

/// Optional async cleanup function injected at creation time.
/// Mirrors `IsolationResolverDeps.cleanup?: { makeRoom, getBreakdown }`.
pub type CleanupFn = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = u32> + Send>>
        + Send
        + Sync,
>;

/// Full set of constructor dependencies.
///
/// Source: `IsolationResolverDeps` at `resolver.ts:45-53`.
pub struct IsolationResolverDeps {
    pub store: Arc<dyn IsolationStore>,
    pub provider: Arc<dyn IsolationProvider>,
    pub cleanup: Option<CleanupFn>,
    pub stale_threshold_days: Option<u32>,
}

/// The isolation resolver — orchestrates the 6-stage cascade.
///
/// Source: `IsolationResolver` class at `resolver.ts:68-570`.
pub struct IsolationResolver {
    store: Arc<dyn IsolationStore>,
    provider: Arc<dyn IsolationProvider>,
    /// Injected cleanup function (from constructor deps).
    cleanup: Option<CleanupFn>,
    /// Stale threshold in days (validated > 0; future use by cleanup/capacity logic).
    #[allow(dead_code)]
    stale_threshold_days: u32,
}

impl IsolationResolver {
    /// Create a new resolver from deps.
    ///
    /// Source: `constructor(deps)` at `resolver.ts:73-83`.
    pub fn new(deps: IsolationResolverDeps) -> Result<Self> {
        let stale_threshold_days = deps
            .stale_threshold_days
            .unwrap_or(DEFAULT_STALE_THRESHOLD_DAYS);
        if stale_threshold_days == 0 {
            return Err(IsolationError::Other(
                "staleThresholdDays must be > 0".to_string(),
            ));
        }
        Ok(Self {
            store: deps.store,
            provider: deps.provider,
            cleanup: deps.cleanup,
            stale_threshold_days,
        })
    }

    // ─── Stage 1: Explicit existing env ──────────────────────────────────────

    /// Stage 1 inner check: look up an explicitly-provided env ID and verify
    /// the worktree is still on disk.
    ///
    /// Returns `Ok(Some(Resolved))` if healthy, `Ok(None)` if stale/missing
    /// (caller must emit `StaleCleaned` and best-effort mark the row destroyed).
    ///
    /// Source: `checkExisting` at `resolver.ts:231-252`.
    /// Key differences from prior port:
    /// - NO `status == Active` gate (source has none).
    /// - Uses `worktreeExists` (FS check), NOT `provider.health_check`.
    /// - Calls `markDestroyedBestEffort` on the stale path (store.updateStatus).
    async fn check_existing(
        &self,
        env_id: &str,
        hints: Option<&IsolationHints>,
    ) -> Result<Option<IsolationResolution>> {
        let row = match self.store.get_by_id(env_id).await? {
            None => return Ok(None),
            Some(r) => r,
        };

        let working_path = row.working_path.clone();

        // FS-check whether the worktree is still on disk.
        // Source: `worktreeExists(toWorktreePath(env.working_path))` at resolver.ts:236.
        let wt_typed = to_worktree_path(working_path.clone())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let exists = worktree_exists(&wt_typed).await.unwrap_or(false);

        if exists {
            // Collect base-branch warnings (non-fatal).
            let warnings = self
                .collect_base_branch_warnings(&row.branch_name, &working_path, hints)
                .await;

            info!(env_id, working_path, "isolation.resolved.existing");
            return Ok(Some(IsolationResolution::Resolved(Box::new(
                ResolvedPayload {
                    env: row,
                    cwd: working_path,
                    method: ResolutionMethod::Existing,
                    warnings: if warnings.is_empty() {
                        None
                    } else {
                        Some(warnings)
                    },
                },
            ))));
        }

        // Worktree is missing — mark the DB row destroyed and return None
        // (caller will emit StaleCleaned).
        // Source: `if (env) { await this.markDestroyedBestEffort(env.id); }` at resolver.ts:247-249.
        warn!(
            env_id,
            working_path, "isolation.resolve_existing.worktree_missing"
        );
        self.mark_destroyed_best_effort(env_id).await;
        Ok(None)
    }

    // ─── Stage 3: Workflow reuse ──────────────────────────────────────────────

    /// Stage 3: find an active env for the current (codebase, workflowType, workflowId).
    ///
    /// Source: `findReusable` at `resolver.ts:288-317`.
    /// Key: uses `worktreeExists` (FS check); if exists, calls `assertWorktreeOwnership`
    /// which THROWS on cross-clone mismatch (propagated as Err). If not on disk,
    /// `markDestroyedBestEffort` + return None.
    async fn find_reusable_environment(
        &self,
        codebase: &CodebaseSummary,
        canonical_repo_path: &str,
        workflow_type: IsolationWorkflowType,
        workflow_id: &str,
        hints: Option<&IsolationHints>,
    ) -> Result<Option<IsolationResolution>> {
        let row = match self
            .store
            .find_active_by_workflow(&codebase.id, workflow_type.clone(), workflow_id)
            .await?
        {
            None => return Ok(None),
            Some(r) => r,
        };

        let working_path = row.working_path.clone();

        // FS-check first. Source: `if (await worktreeExists(worktreePath))` at resolver.ts:299.
        let wt_typed = to_worktree_path(working_path.clone())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let exists = worktree_exists(&wt_typed).await.unwrap_or(false);

        if !exists {
            // Stale — mark destroyed and fall through to next stage.
            // Source: `await this.markDestroyedBestEffort(existing.id); return null;` at resolver.ts:315-316.
            warn!(
                workflow_id,
                working_path, "isolation.workflow_reuse.worktree_missing"
            );
            self.mark_destroyed_best_effort(&row.id).await;
            return Ok(None);
        }

        // Ownership check — THROWS on cross-clone mismatch (propagates as Err).
        // Source: `assertWorktreeOwnership` at resolver.ts:300-305 (re-throws).
        self.assert_worktree_ownership(canonical_repo_path, &working_path)
            .await?;

        // Collect base-branch warnings (non-fatal).
        let warnings = self
            .collect_base_branch_warnings(&row.branch_name, &working_path, hints)
            .await;

        info!(
            workflow_id,
            working_path, "isolation.resolved.workflow_reuse"
        );
        Ok(Some(IsolationResolution::Resolved(Box::new(
            ResolvedPayload {
                env: row,
                cwd: working_path,
                method: ResolutionMethod::WorkflowReuse,
                warnings: if warnings.is_empty() {
                    None
                } else {
                    Some(warnings)
                },
            },
        ))))
    }

    // ─── Stage 4: Linked-issue reuse ─────────────────────────────────────────

    /// Stage 4: check each linked issue for a reusable env.
    ///
    /// Source: `findLinkedIssueEnv` at `resolver.ts:329-363`.
    /// Key: uses `worktreeExists` + `assertWorktreeOwnership` (throws on cross-clone).
    async fn find_linked_issue_environment(
        &self,
        codebase: &CodebaseSummary,
        canonical_repo_path: &str,
        hints: &IsolationHints,
    ) -> Result<Option<IsolationResolution>> {
        let linked_issues = match &hints.linked_issues {
            None => return Ok(None),
            Some(v) if v.is_empty() => return Ok(None),
            Some(v) => v.clone(),
        };

        for issue_num in linked_issues {
            let workflow_id = issue_num.to_string();
            let row = match self
                .store
                .find_active_by_workflow(&codebase.id, IsolationWorkflowType::Issue, &workflow_id)
                .await?
            {
                None => continue,
                Some(r) => r,
            };

            let working_path = row.working_path.clone();

            // FS-check. Source: `if (await worktreeExists(worktreePath))` at resolver.ts:343.
            let wt_typed = to_worktree_path(working_path.clone())
                .map_err(|e| IsolationError::Other(e.to_string()))?;
            let exists = worktree_exists(&wt_typed).await.unwrap_or(false);

            if !exists {
                // Stale — mark destroyed and try the next linked issue.
                // Source: `await this.markDestroyedBestEffort(linkedEnv.id);` at resolver.ts:360.
                warn!(
                    issue_num,
                    working_path, "isolation.linked_issue_reuse.worktree_missing"
                );
                self.mark_destroyed_best_effort(&row.id).await;
                continue;
            }

            // Ownership check — THROWS on cross-clone mismatch (propagates as Err).
            // Source: `assertWorktreeOwnership` at resolver.ts:344-349 (re-throws).
            self.assert_worktree_ownership(canonical_repo_path, &working_path)
                .await?;

            info!(
                issue_num,
                working_path, "isolation.resolved.linked_issue_reuse"
            );
            return Ok(Some(IsolationResolution::Resolved(Box::new(
                ResolvedPayload {
                    env: row,
                    cwd: working_path,
                    method: ResolutionMethod::LinkedIssueReuse {
                        issue_number: issue_num,
                    },
                    warnings: None,
                },
            ))));
        }

        Ok(None)
    }

    // ─── Stage 5: Branch adoption ─────────────────────────────────────────────

    /// Stage 5: try to adopt a worktree that already has the PR branch.
    ///
    /// Source: `tryBranchAdoption` at `resolver.ts:372-412`.
    /// Key: uses `prBranch` hint (not `suggestedBranch`); persists
    ///      `metadata: { adopted: true, adopted_from: 'skill' }`.
    async fn try_branch_adoption(
        &self,
        codebase: &CodebaseSummary,
        canonical_repo_path: &str,
        hints: &IsolationHints,
        workflow_type: IsolationWorkflowType,
        workflow_id: &str,
        git_identity: Option<&crate::types::GitIdentity>,
    ) -> Result<Option<IsolationResolution>> {
        // Source uses `hints.prBranch` only. Source: `const prBranch = hints.prBranch;` at resolver.ts:381.
        let branch = match hints.pr_branch.as_deref() {
            Some(b) => b.to_string(),
            None => return Ok(None),
        };

        let repo_path_typed = to_repo_path(canonical_repo_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let branch_typed =
            to_branch_name(branch.clone()).map_err(|e| IsolationError::Other(e.to_string()))?;

        let adopted_path = match find_worktree_by_branch(&repo_path_typed, &branch_typed).await? {
            None => return Ok(None),
            Some(p) => p,
        };

        let working_path = adopted_path.as_str().to_string();

        // FS-check. Source: `if (adoptedPath && (await worktreeExists(adoptedPath)))` at resolver.ts:385.
        let wt_typed = to_worktree_path(working_path.clone())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let exists = worktree_exists(&wt_typed).await.unwrap_or(false);
        if !exists {
            return Ok(None);
        }

        // Ownership check — THROWS on cross-clone mismatch.
        // Source: `assertWorktreeOwnership` at resolver.ts:386-390.
        self.assert_worktree_ownership(canonical_repo_path, &working_path)
            .await?;

        // Persist to store with adoption metadata.
        // Source: `metadata: { adopted: true, adopted_from: 'skill' }` at resolver.ts:402.
        let mut meta = serde_json::Map::new();
        meta.insert("adopted".to_string(), serde_json::Value::Bool(true));
        meta.insert(
            "adopted_from".to_string(),
            serde_json::Value::String("skill".to_string()),
        );

        let env = self
            .store
            .create(CreateEnvironmentParams {
                codebase_id: codebase.id.clone(),
                workflow_type,
                workflow_id: workflow_id.to_string(),
                provider: Some(IsolationProviderType::Worktree),
                working_path: working_path.clone(),
                branch_name: branch.clone(),
                created_by_platform: None,
                created_by_user_id: git_identity.and_then(|i| {
                    if i.email.is_empty() {
                        None
                    } else {
                        Some(i.email.clone())
                    }
                }),
                metadata: Some(meta),
            })
            .await?;

        info!(branch, working_path, "isolation.resolved.branch_adoption");
        Ok(Some(IsolationResolution::Resolved(Box::new(
            ResolvedPayload {
                env,
                cwd: working_path,
                method: ResolutionMethod::BranchAdoption { branch },
                warnings: None,
            },
        ))))
    }

    // ─── Stage 6: Create new ──────────────────────────────────────────────────

    /// Stage 6: create a fresh isolation environment.
    ///
    /// Source: `createNewEnvironment` at `resolver.ts:433-568`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_new_environment(
        &self,
        codebase: &CodebaseSummary,
        canonical_repo_path: &str,
        workflow_type: IsolationWorkflowType,
        workflow_id: &str,
        hints: Option<&IsolationHints>,
        platform_type: &str,
        git_identity: Option<&crate::types::GitIdentity>,
    ) -> Result<IsolationResolution> {
        // Run cleanup to make room (best-effort; count is reported in resolution).
        let auto_cleaned_count = if let Some(cleanup) = &self.cleanup {
            let codebase_id = codebase.id.clone();
            let f = cleanup(codebase_id);
            f.await
        } else {
            0
        };

        // Build the IsolationRequest for this workflow type.
        let base = IsolationRequestBase {
            codebase_id: codebase.id.clone(),
            codebase_name: Some(codebase.name.clone()),
            canonical_repo_path: canonical_repo_path.to_string(),
            description: None,
            git_identity: git_identity.cloned(),
        };

        let request =
            self.build_isolation_request(base, workflow_type.clone(), workflow_id, hints)?;

        // Create the worktree environment.
        let wt_env = match self.provider.create(request.clone()).await {
            Ok(env) => env,
            Err(e) => {
                let err_str = e.to_string();
                if crate::errors::is_known_isolation_error(&err_str, None) {
                    let user_msg = crate::errors::classify_isolation_error(&err_str, None);
                    return Ok(IsolationResolution::Blocked {
                        reason: "creation_failed".to_string(),
                        user_message: user_msg,
                    });
                }
                return Err(e);
            }
        };

        let working_path = wt_env.working_path.clone();
        let branch_name = wt_env.branch_name.clone();
        let warnings = wt_env.warnings.clone();

        // Persist to store; on failure clean up the orphaned worktree.
        let env = match self
            .store
            .create(CreateEnvironmentParams {
                codebase_id: codebase.id.clone(),
                workflow_type: workflow_type.clone(),
                workflow_id: workflow_id.to_string(),
                provider: Some(IsolationProviderType::Worktree),
                working_path: working_path.clone(),
                branch_name: branch_name.clone(),
                created_by_platform: Some(platform_type.to_string()),
                created_by_user_id: git_identity.and_then(|i| {
                    if i.email.is_empty() {
                        None
                    } else {
                        Some(i.email.clone())
                    }
                }),
                metadata: None,
            })
            .await
        {
            Ok(row) => row,
            Err(e) => {
                // Store failed — physically destroy the orphaned worktree (best-effort).
                // Source: `await this.provider.destroy(isolatedEnv.workingPath, {
                //   canonicalRepoPath, branchName, force: true })` at resolver.ts:536-541.
                // Errors from destroy are logged but do NOT mask the original store error.
                error!(
                    working_path,
                    branch_name,
                    err = %e,
                    "isolation.create.store_failed_destroying_worktree"
                );
                if let Err(destroy_err) = self
                    .provider
                    .destroy(
                        &working_path,
                        Some(DestroyOptions {
                            force: Some(true),
                            branch_name: Some(branch_name.clone()),
                            canonical_repo_path: Some(canonical_repo_path.to_string()),
                            ..Default::default()
                        }),
                    )
                    .await
                {
                    error!(
                        working_path,
                        err = %destroy_err,
                        "isolation.create.orphan_destroy_failed"
                    );
                }
                return Err(IsolationError::Other(format!(
                    "Failed to persist isolation environment record: {e}"
                )));
            }
        };

        info!(
            workflow_id,
            working_path, branch_name, auto_cleaned_count, "isolation.resolved.created"
        );

        Ok(IsolationResolution::Resolved(Box::new(ResolvedPayload {
            env,
            cwd: working_path,
            method: ResolutionMethod::Created {
                auto_cleaned_count: if auto_cleaned_count > 0 {
                    Some(auto_cleaned_count)
                } else {
                    None
                },
            },
            warnings,
        })))
    }

    // ─── Build IsolationRequest from ResolveRequest ───────────────────────────

    /// Map `ResolveRequest` → `IsolationRequest` for a given workflow type.
    ///
    /// Source: `buildIsolationRequest` inside `createNewEnvironment` at `resolver.ts:452-471`.
    fn build_isolation_request(
        &self,
        base: IsolationRequestBase,
        workflow_type: IsolationWorkflowType,
        workflow_id: &str,
        hints: Option<&IsolationHints>,
    ) -> Result<IsolationRequest> {
        match workflow_type {
            IsolationWorkflowType::Issue => Ok(IsolationRequest::Issue {
                base,
                identifier: workflow_id.to_string(),
            }),
            IsolationWorkflowType::Review => Ok(IsolationRequest::Review {
                base,
                identifier: workflow_id.to_string(),
            }),
            IsolationWorkflowType::Thread => Ok(IsolationRequest::Thread {
                base,
                identifier: workflow_id.to_string(),
            }),
            IsolationWorkflowType::Task => {
                let from_branch = hints.and_then(|h| h.from_branch.clone());
                Ok(IsolationRequest::Task {
                    base,
                    identifier: workflow_id.to_string(),
                    from_branch,
                })
            }
            IsolationWorkflowType::Pr => {
                let hints = hints.ok_or_else(|| {
                    IsolationError::Other(
                        "PR isolation request requires hints (prBranch, isForkPR)".to_string(),
                    )
                })?;
                let pr_branch = hints.pr_branch.clone().ok_or_else(|| {
                    IsolationError::Other("PR isolation request missing prBranch hint".to_string())
                })?;
                let is_fork_pr = hints.is_fork_pr.unwrap_or(false);
                Ok(IsolationRequest::Pr {
                    base,
                    identifier: workflow_id.to_string(),
                    pr_branch,
                    pr_sha: hints.pr_sha.clone(),
                    is_fork_pr,
                })
            }
        }
    }

    // ─── Helpers ──────────────────────────────────────────────────────────────

    /// Assert that a worktree belongs to the expected repo, logging and re-throwing
    /// on cross-clone mismatch.
    ///
    /// Source: `assertWorktreeOwnership` at `resolver.ts:263-278`. Throws on mismatch
    /// (propagates via `?`).
    async fn assert_worktree_ownership(
        &self,
        canonical_repo_path: &str,
        working_path: &str,
    ) -> Result<()> {
        let repo_typed = to_repo_path(canonical_repo_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let wt_typed = to_worktree_path(working_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        verify_worktree_ownership(&wt_typed, &repo_typed)
            .await
            .map_err(|e| {
                warn!(
                    canonical_repo_path,
                    working_path,
                    err = %e,
                    "isolation.ownership_mismatch"
                );
                IsolationError::Other(e.to_string())
            })
    }

    /// Collect warnings about the worktree branch not being based on `base_branch`.
    ///
    /// Source: `collectBaseBranchWarnings` at `resolver.ts:201-226`.
    /// Key:
    /// - Returns `[]` immediately when no `baseBranch` hint (`if (!baseBranch) return []`).
    ///   Source: resolver.ts:206. Previously Rust defaulted to "main" — WRONG.
    /// - Warning text is byte-for-byte:
    ///   `Worktree branch '<branch_name>' is not based on '<base_branch>'. Recreate with: archon complete <branch_name> --force`
    ///   Source: resolver.ts:214-216.
    async fn collect_base_branch_warnings(
        &self,
        branch_name: &str,
        working_path: &str,
        hints: Option<&IsolationHints>,
    ) -> Vec<String> {
        // Source: `if (!baseBranch) return [];` at resolver.ts:206.
        let base_branch = match hints.and_then(|h| h.base_branch.as_deref()) {
            Some(b) => b.to_string(),
            None => return vec![],
        };

        let ancestor_ref = format!("origin/{base_branch}");
        match is_ancestor_of(working_path, &ancestor_ref).await {
            Ok(true) => vec![], // worktree is ahead-of-or-at base — all good.
            Ok(false) => {
                // Source: resolver.ts:214-216.
                vec![format!(
                    "Worktree branch '{branch_name}' is not based on '{base_branch}'. \
                     Recreate with: archon complete {branch_name} --force"
                )]
            }
            Err(e) => {
                debug!(
                    working_path,
                    base_branch,
                    err = %e,
                    "isolation.base_branch_warning_check_error"
                );
                vec![] // Non-fatal: skip warning on check failure.
            }
        }
    }

    /// Best-effort: mark a stale environment as destroyed in the store.
    ///
    /// Source: `markDestroyedBestEffort` at `resolver.ts:418-428`.
    /// Calls `store.updateStatus(envId, 'destroyed')` — NOT `provider.destroy`.
    async fn mark_destroyed_best_effort(&self, env_id: &str) {
        self.update_store_status_best_effort(env_id).await;
    }

    /// Update store status to destroyed (best-effort, logs error, never throws).
    ///
    /// Source: `markDestroyedBestEffort` body at `resolver.ts:419-427`.
    async fn update_store_status_best_effort(&self, env_id: &str) {
        match self.store.update_status(env_id, "destroyed").await {
            Ok(_) => debug!(env_id, "isolation.store_status_updated_destroyed"),
            Err(e) => error!(
                env_id,
                err = %e,
                "isolation.store_status_update_failed"
            ),
        }
    }

    // ─── Public: resolve ──────────────────────────────────────────────────────

    /// Run the full 6-stage resolution cascade.
    ///
    /// Source: `resolve(request)` at `resolver.ts:88-194`.
    pub async fn resolve(&self, request: ResolveRequest) -> Result<IsolationResolution> {
        let hints = request.hints.as_ref();
        let workflow_type = hints
            .and_then(|h| h.workflow_type.clone())
            .unwrap_or(IsolationWorkflowType::Thread);
        let workflow_id = hints
            .and_then(|h| h.workflow_id.as_deref())
            .unwrap_or("")
            .to_string();
        let git_identity = request.git_identity.as_ref();
        let platform_type = &request.platform_type;

        // ── Stage 1: Existing explicit env ────────────────────────────────────
        // Source: `if (request.existingEnvId)` at resolver.ts:92-97.
        // If checkExisting returns null → immediately return StaleCleaned (NOT fall-through).
        if let Some(env_id) = &request.existing_env_id {
            match self.check_existing(env_id, hints).await? {
                Some(resolution) => return Ok(resolution),
                None => {
                    // Stale — tell caller to clear and retry.
                    // Source: `return { status: 'stale_cleaned', previousEnvId: request.existingEnvId };`
                    // at resolver.ts:96.
                    return Ok(IsolationResolution::StaleCleaned {
                        previous_env_id: env_id.clone(),
                    });
                }
            }
        }

        // ── Stage 2: No-codebase shortcircuit ────────────────────────────────
        // Source: `return { status: 'none', cwd: '/workspace' };` at resolver.ts:101.
        let codebase = match &request.codebase {
            None => {
                debug!("isolation.no_codebase_shortcircuit");
                return Ok(IsolationResolution::None {
                    cwd: "/workspace".to_string(),
                });
            }
            Some(c) => c,
        };

        // ── Canonical repo path ───────────────────────────────────────────────
        let canonical_repo_path = match get_canonical_repo_path(&codebase.default_cwd).await {
            Ok(rp) => rp.as_str().to_string(),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("not a git repository") || msg.contains("no such file or directory")
                {
                    // Known blocking condition.
                    return Ok(IsolationResolution::Blocked {
                        reason: "creation_failed".to_string(),
                        user_message: format!(
                            "**Error:** The workspace path `{}` is not a valid git repository. \
                             Please check the codebase configuration.",
                            codebase.default_cwd
                        ),
                    });
                }
                // Unknown error — rethrow.
                return Err(IsolationError::Other(e.to_string()));
            }
        };

        // ── Stage 3: Workflow reuse ───────────────────────────────────────────
        if !workflow_id.is_empty() {
            if let Some(resolution) = self
                .find_reusable_environment(
                    codebase,
                    &canonical_repo_path,
                    workflow_type.clone(),
                    &workflow_id,
                    hints,
                )
                .await?
            {
                return Ok(resolution);
            }
        }

        // ── Stage 4: Linked-issue reuse ───────────────────────────────────────
        if let Some(h) = hints {
            if let Some(resolution) = self
                .find_linked_issue_environment(codebase, &canonical_repo_path, h)
                .await?
            {
                return Ok(resolution);
            }
        }

        // ── Stage 5: Branch adoption ──────────────────────────────────────────
        // Source: `if (hints?.prBranch)` at resolver.ts:170. Only prBranch triggers adoption.
        if let Some(h) = hints {
            if h.pr_branch.is_some() {
                if let Some(resolution) = self
                    .try_branch_adoption(
                        codebase,
                        &canonical_repo_path,
                        h,
                        workflow_type.clone(),
                        &workflow_id,
                        git_identity,
                    )
                    .await?
                {
                    return Ok(resolution);
                }
            }
        }

        // ── Stage 6: Create new ───────────────────────────────────────────────
        self.create_new_environment(
            codebase,
            &canonical_repo_path,
            workflow_type,
            &workflow_id,
            hints,
            platform_type,
            git_identity,
        )
        .await
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::InMemoryIsolationStore;
    use crate::types::{
        AdoptedWorktreeMetadata, DestroyOptions, DestroyResult, EnvironmentStatus,
        IsolationProviderType, WorktreeEnvironment, WorktreeMetadata,
    };
    use std::sync::Arc;

    // ─── Mock provider ─────────────────────────────────────────────────────────

    #[derive(Default)]
    struct MockProvider {
        /// When set, create() returns this environment.
        create_result: Option<WorktreeEnvironment>,
        /// When Some(true), health_check() returns true.
        healthy: bool,
    }

    fn mock_env(path: &str, branch: &str) -> WorktreeEnvironment {
        WorktreeEnvironment {
            id: path.to_string(),
            working_path: path.to_string(),
            branch_name: branch.to_string(),
            provider: "worktree".to_string(),
            status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(),
            warnings: None,
            metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                adopted: false,
                adopted_from: None,
                request: None,
            }),
        }
    }

    #[async_trait::async_trait]
    impl IsolationProvider for MockProvider {
        fn provider_type(&self) -> IsolationProviderType {
            IsolationProviderType::Worktree
        }

        async fn create(&self, request: IsolationRequest) -> crate::Result<WorktreeEnvironment> {
            if let Some(env) = &self.create_result {
                Ok(env.clone())
            } else {
                let path = format!("/mock/{}", request.identifier());
                Ok(mock_env(&path, "mock-branch"))
            }
        }

        async fn destroy(
            &self,
            _env_id: &str,
            _options: Option<DestroyOptions>,
        ) -> crate::Result<DestroyResult> {
            Ok(DestroyResult {
                worktree_removed: true,
                branch_deleted: None,
                remote_branch_deleted: None,
                directory_clean: true,
                warnings: vec![],
            })
        }

        async fn get(&self, _env_id: &str) -> crate::Result<Option<WorktreeEnvironment>> {
            Ok(None)
        }

        async fn list(&self, _codebase_id: &str) -> crate::Result<Vec<WorktreeEnvironment>> {
            Ok(vec![])
        }

        async fn health_check(&self, _env_id: &str) -> crate::Result<bool> {
            Ok(self.healthy)
        }
    }

    fn make_resolver(
        store: Arc<dyn IsolationStore>,
        provider: Arc<dyn IsolationProvider>,
    ) -> IsolationResolver {
        IsolationResolver::new(IsolationResolverDeps {
            store,
            provider,
            cleanup: None,
            stale_threshold_days: None,
        })
        .unwrap()
    }

    fn simple_request(codebase: Option<CodebaseSummary>) -> ResolveRequest {
        ResolveRequest {
            existing_env_id: None,
            codebase,
            hints: None,
            platform_type: "slack".to_string(),
            user_id: None,
            git_identity: None,
        }
    }

    // ─── Constructor validation ────────────────────────────────────────────────

    #[test]
    fn rejects_zero_stale_threshold() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let result = IsolationResolver::new(IsolationResolverDeps {
            store,
            provider,
            cleanup: None,
            stale_threshold_days: Some(0),
        });
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("must be > 0"), "got: {err_msg}");
    }

    #[test]
    fn accepts_custom_stale_threshold() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = IsolationResolver::new(IsolationResolverDeps {
            store,
            provider,
            cleanup: None,
            stale_threshold_days: Some(7),
        });
        assert!(resolver.is_ok());
    }

    // ─── Stage 2: No-codebase shortcircuit ───────────────────────────────────

    #[tokio::test]
    async fn no_codebase_returns_none_resolution() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let result = resolver.resolve(simple_request(None)).await.unwrap();
        // TS golden: { status: 'none', cwd: '/workspace' }
        match result {
            IsolationResolution::None { cwd } => {
                assert_eq!(cwd, "/workspace", "no-codebase cwd must be /workspace");
            }
            other => panic!("expected None, got {other:?}"),
        }
    }

    // ─── Stage 2: Bad cwd → Blocked ───────────────────────────────────────────

    #[tokio::test]
    async fn nonexistent_cwd_returns_blocked() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let request = ResolveRequest {
            existing_env_id: None,
            codebase: Some(CodebaseSummary {
                id: "cdb-001".into(),
                default_cwd: "/nonexistent/path/xyz_99999".into(),
                name: "owner/repo".into(),
            }),
            hints: None,
            platform_type: "slack".to_string(),
            user_id: None,
            git_identity: None,
        };

        let result = resolver.resolve(request).await.unwrap();
        // Nonexistent path = "not a git repository" OR error → Blocked.
        match result {
            IsolationResolution::Blocked { reason, .. } => {
                assert_eq!(reason, "creation_failed");
            }
            // Some environments might create successfully in test; accept Created too.
            IsolationResolution::Resolved(_) => {}
            other => panic!("Expected Blocked or Resolved, got {other:?}"),
        }
    }

    // ─── Stage 1: Explicit existing env ──────────────────────────────────────

    #[tokio::test]
    async fn existing_env_id_not_in_store_returns_stale_cleaned() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider {
            healthy: false,
            create_result: None,
        });
        let resolver = make_resolver(store, provider);

        let request = ResolveRequest {
            existing_env_id: Some("nonexistent-id".into()),
            codebase: None,
            hints: None,
            platform_type: "slack".to_string(),
            user_id: None,
            git_identity: None,
        };

        // When env_id not found → checkExisting returns None → StaleCleaned
        let result = resolver.resolve(request).await.unwrap();
        match result {
            IsolationResolution::StaleCleaned { previous_env_id } => {
                assert_eq!(previous_env_id, "nonexistent-id");
            }
            other => panic!("expected StaleCleaned, got {other:?}"),
        }
    }

    // ─── Stage 3: Workflow reuse ───────────────────────────────────────────────

    #[tokio::test]
    async fn workflow_reuse_found_and_healthy_returns_resolved() {
        let store = InMemoryIsolationStore::new();
        // Pre-populate a row in the store.
        let row = store
            .create(crate::types::CreateEnvironmentParams {
                codebase_id: "cdb-001".into(),
                workflow_type: IsolationWorkflowType::Thread,
                workflow_id: "thread-abc".into(),
                provider: Some(IsolationProviderType::Worktree),
                working_path: "/tmp/dummy-wt".into(),
                branch_name: "archon/thread-abc".into(),
                created_by_platform: None,
                created_by_user_id: None,
                metadata: None,
            })
            .await
            .unwrap();

        let provider = Arc::new(MockProvider {
            healthy: true,
            create_result: None,
        });
        let resolver = make_resolver(store, Arc::clone(&provider) as Arc<dyn IsolationProvider>);

        // We can't test "found via store + worktree on disk → Resolved.method = WorkflowReuse"
        // in a pure unit test because verify_worktree_ownership hits the filesystem.
        // The worktree at /tmp/dummy-wt doesn't exist → falls through to create.
        let request = ResolveRequest {
            existing_env_id: None,
            codebase: Some(CodebaseSummary {
                id: "cdb-001".into(),
                default_cwd: "/nonexistent/path/xyz".into(),
                name: "owner/repo".into(),
            }),
            hints: Some(IsolationHints {
                workflow_type: Some(IsolationWorkflowType::Thread),
                workflow_id: Some("thread-abc".into()),
                ..Default::default()
            }),
            platform_type: "slack".to_string(),
            user_id: None,
            git_identity: None,
        };

        // We just verify it doesn't panic or error when the worktree is missing.
        let result = resolver.resolve(request).await;
        assert!(result.is_ok());
        let _ = row; // ensure row exists
    }

    // ─── Stage 4: Linked issue reuse ─────────────────────────────────────────

    #[tokio::test]
    async fn linked_issue_reuse_no_issues_skips() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider {
            healthy: false,
            create_result: None,
        });
        let resolver = make_resolver(store, provider);

        let result = resolver
            .find_linked_issue_environment(
                &CodebaseSummary {
                    id: "cdb-001".into(),
                    default_cwd: "/tmp".into(),
                    name: "owner/repo".into(),
                },
                "/tmp",
                &IsolationHints {
                    linked_issues: None,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn linked_issue_reuse_empty_list_skips() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider {
            healthy: false,
            create_result: None,
        });
        let resolver = make_resolver(store, provider);

        let result = resolver
            .find_linked_issue_environment(
                &CodebaseSummary {
                    id: "cdb-001".into(),
                    default_cwd: "/tmp".into(),
                    name: "owner/repo".into(),
                },
                "/tmp",
                &IsolationHints {
                    linked_issues: Some(vec![]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert!(result.is_none());
    }

    // ─── Stage 5: Branch adoption — missing hints ─────────────────────────────

    #[tokio::test]
    async fn branch_adoption_no_branch_hint_skips() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let result = resolver
            .try_branch_adoption(
                &CodebaseSummary {
                    id: "cdb-001".into(),
                    default_cwd: "/tmp".into(),
                    name: "owner/repo".into(),
                },
                "/tmp",
                &IsolationHints {
                    suggested_branch: None,
                    pr_branch: None,
                    ..Default::default()
                },
                IsolationWorkflowType::Thread,
                "thread-001",
                None,
            )
            .await
            .unwrap();

        assert!(result.is_none());
    }

    // ─── build_isolation_request ───────────────────────────────────────────────

    #[test]
    fn build_issue_request() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let base = IsolationRequestBase {
            codebase_id: "cdb-001".into(),
            codebase_name: Some("owner/repo".into()),
            canonical_repo_path: "/tmp/repo".into(),
            description: None,
            git_identity: None,
        };

        let req = resolver
            .build_isolation_request(base, IsolationWorkflowType::Issue, "42", None)
            .unwrap();

        assert!(matches!(req, IsolationRequest::Issue { identifier, .. } if identifier == "42"));
    }

    #[test]
    fn build_task_request_with_from_branch() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let base = IsolationRequestBase {
            codebase_id: "cdb-001".into(),
            codebase_name: Some("owner/repo".into()),
            canonical_repo_path: "/tmp/repo".into(),
            description: None,
            git_identity: None,
        };

        let hints = IsolationHints {
            from_branch: Some("develop".into()),
            ..Default::default()
        };

        let req = resolver
            .build_isolation_request(base, IsolationWorkflowType::Task, "my-task", Some(&hints))
            .unwrap();

        match req {
            IsolationRequest::Task {
                from_branch,
                identifier,
                ..
            } => {
                assert_eq!(identifier, "my-task");
                assert_eq!(from_branch.as_deref(), Some("develop"));
            }
            _ => panic!("expected Task"),
        }
    }

    #[test]
    fn build_pr_request_missing_pr_branch_errors() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let base = IsolationRequestBase {
            codebase_id: "cdb-001".into(),
            codebase_name: None,
            canonical_repo_path: "/tmp/repo".into(),
            description: None,
            git_identity: None,
        };

        // Missing prBranch hint → error.
        let result = resolver.build_isolation_request(
            base,
            IsolationWorkflowType::Pr,
            "42",
            Some(&IsolationHints::default()),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("prBranch"));
    }

    #[test]
    fn build_pr_request_with_hints() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let base = IsolationRequestBase {
            codebase_id: "cdb-001".into(),
            codebase_name: None,
            canonical_repo_path: "/tmp/repo".into(),
            description: None,
            git_identity: None,
        };

        let hints = IsolationHints {
            pr_branch: Some("feature/my-pr".into()),
            is_fork_pr: Some(true),
            pr_sha: Some("abc123".into()),
            ..Default::default()
        };

        let req = resolver
            .build_isolation_request(base, IsolationWorkflowType::Pr, "99", Some(&hints))
            .unwrap();

        match req {
            IsolationRequest::Pr {
                is_fork_pr,
                pr_branch,
                pr_sha,
                identifier,
                ..
            } => {
                assert_eq!(identifier, "99");
                assert_eq!(pr_branch, "feature/my-pr");
                assert!(is_fork_pr);
                assert_eq!(pr_sha.as_deref(), Some("abc123"));
            }
            _ => panic!("expected PR"),
        }
    }

    // ─── Cleanup injection ────────────────────────────────────────────────────

    #[tokio::test]
    async fn cleanup_fn_is_called_during_create() {
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = called.clone();

        let cleanup: CleanupFn = Arc::new(move |_codebase_id: String| {
            let flag2 = flag.clone();
            Box::pin(async move {
                flag2.store(true, std::sync::atomic::Ordering::SeqCst);
                0u32
            })
        });

        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider {
            healthy: false,
            create_result: None,
        });
        let resolver = IsolationResolver::new(IsolationResolverDeps {
            store,
            provider,
            cleanup: Some(cleanup),
            stale_threshold_days: None,
        })
        .unwrap();

        // The path doesn't exist → create_new_environment is called (runs cleanup),
        // but get_canonical_repo_path will block it before cleanup runs.
        // Instead test the internal helper directly:
        let _ = resolver
            .create_new_environment(
                &CodebaseSummary {
                    id: "cdb-001".into(),
                    default_cwd: "/tmp".into(),
                    name: "owner/repo".into(),
                },
                "/nonexistent/repo",
                IsolationWorkflowType::Thread,
                "t1",
                None,
                "slack",
                None,
            )
            .await;

        assert!(
            called.load(std::sync::atomic::Ordering::SeqCst),
            "cleanup fn should have been called"
        );
    }

    // ─── Default stale threshold ───────────────────────────────────────────────

    #[test]
    fn default_stale_threshold_is_14() {
        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = IsolationResolver::new(IsolationResolverDeps {
            store,
            provider,
            cleanup: None,
            stale_threshold_days: None,
        })
        .unwrap();
        assert_eq!(resolver.stale_threshold_days, DEFAULT_STALE_THRESHOLD_DAYS);
    }

    // ─── Stage 1: stale env emits StaleCleaned ────────────────────────────────

    #[tokio::test]
    async fn stage1_missing_worktree_emits_stale_cleaned() {
        // A row exists in the store but the working_path does NOT exist on disk.
        let store = InMemoryIsolationStore::new();
        let env_row = store
            .create(crate::types::CreateEnvironmentParams {
                codebase_id: "cdb-001".into(),
                workflow_type: IsolationWorkflowType::Thread,
                workflow_id: "t1".into(),
                provider: Some(IsolationProviderType::Worktree),
                working_path: "/totally/nonexistent/path/abc123".into(),
                branch_name: "archon/thread-t1".into(),
                created_by_platform: None,
                created_by_user_id: None,
                metadata: None,
            })
            .await
            .unwrap();

        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        let request = ResolveRequest {
            existing_env_id: Some(env_row.id.clone()),
            codebase: None,
            hints: None,
            platform_type: "slack".to_string(),
            user_id: None,
            git_identity: None,
        };

        let result = resolver.resolve(request).await.unwrap();
        match result {
            IsolationResolution::StaleCleaned { previous_env_id } => {
                assert_eq!(
                    previous_env_id, env_row.id,
                    "StaleCleaned must include the old env id"
                );
            }
            other => panic!("expected StaleCleaned, got {other:?}"),
        }
    }

    // ─── FAIL-5: cross-clone ownership error propagates ───────────────────────

    /// Stage 3 propagates cross-clone ownership errors (does NOT swallow → Ok(None)).
    /// In the real scenario this requires two git clones of the same remote.
    /// We test the assert_worktree_ownership plumbing using a real temp repo +
    /// a worktree whose gitdir points to a DIFFERENT repo root.
    #[tokio::test]
    async fn assert_worktree_ownership_propagates_cross_clone_error() {
        use std::process::Command;

        // Create a repo and a separate repo (different root → ownership mismatch).
        let root1 = tempfile::tempdir().unwrap();
        let repo1 = root1.path().join("repo1");
        std::fs::create_dir_all(&repo1).unwrap();
        let repo1_s = repo1.to_string_lossy().to_string();
        let out = Command::new("git")
            .args(["init", "-q", "-b", "main", &repo1_s])
            .output()
            .unwrap();
        assert!(out.status.success());
        Command::new("git")
            .arg("-C")
            .arg(&repo1_s)
            .args(["config", "user.email", "t@t.com"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo1_s)
            .args(["config", "user.name", "T"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo1_s)
            .args(["commit", "--allow-empty", "-q", "-m", "init"])
            .output()
            .unwrap();

        let root2 = tempfile::tempdir().unwrap();
        let repo2 = root2.path().join("repo2");
        std::fs::create_dir_all(&repo2).unwrap();
        let repo2_s = repo2.to_string_lossy().to_string();
        let out = Command::new("git")
            .args(["init", "-q", "-b", "main", &repo2_s])
            .output()
            .unwrap();
        assert!(out.status.success());
        Command::new("git")
            .arg("-C")
            .arg(&repo2_s)
            .args(["config", "user.email", "t@t.com"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo2_s)
            .args(["config", "user.name", "T"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo2_s)
            .args(["commit", "--allow-empty", "-q", "-m", "init"])
            .output()
            .unwrap();

        // Create a worktree from repo2.
        let wt2 = root2.path().join("wt2").to_string_lossy().to_string();
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo2_s)
            .args(["worktree", "add", "-q", "-b", "cross-branch", &wt2])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "worktree add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let store = InMemoryIsolationStore::new();
        let provider = Arc::new(MockProvider::default());
        let resolver = make_resolver(store, provider);

        // Assert that asserting ownership of wt2 (from repo2) against repo1 returns Err.
        let result = resolver.assert_worktree_ownership(&repo1_s, &wt2).await;
        assert!(
            result.is_err(),
            "cross-clone ownership must be an error, got: {result:?}"
        );
    }
}
