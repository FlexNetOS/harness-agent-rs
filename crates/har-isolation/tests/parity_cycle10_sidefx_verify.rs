//! Cycle-10 re-verification: side-effect differential for IS-03 FAIL-1 / FAIL-5.
//!
//! The committed golden (parity_cycle10_resolver.rs) asserts stage + resolution +
//! the FAIL-3/FAIL-4 return-value divergences. This file proves the two
//! SIDE-EFFECT-ONLY divergences a return-value diff misses, matching the TS
//! oracle (__parity_resolver_oracle_c10.ts):
//! - FAIL-1: stale stage-1 path calls store.update_status(id,"destroyed")
//!   (DB row), NOT provider.destroy (physical worktree).
//! - FAIL-5: cross-clone reuse PROPAGATES (Err) — no create, no destroy.
//!
//! VERIFIER-OWNED transient check; safe to keep (it is a real parity assertion).

use std::process::Command;
use std::sync::{Arc, Mutex};

use har_isolation::resolver::{IsolationResolver, IsolationResolverDeps};
use har_isolation::store::IsolationStore;
use har_isolation::types::{
    AdoptedWorktreeMetadata, CodebaseSummary, CreateEnvironmentParams, DestroyOptions,
    DestroyResult, EnvironmentStatus, IsolationEnvironmentRow, IsolationHints, IsolationProvider,
    IsolationProviderType, IsolationRequest, IsolationResolution, IsolationWorkflowType,
    ResolveRequest, WorktreeEnvironment, WorktreeMetadata,
};

#[derive(Default)]
struct Fx {
    update_status_calls: Vec<(String, String)>,
    destroy_calls: Vec<String>,
    create_calls: usize,
}

#[derive(Default)]
struct InMemStore {
    rows: Mutex<std::collections::HashMap<String, IsolationEnvironmentRow>>,
    seq: std::sync::atomic::AtomicU64,
    fx: Arc<Mutex<Fx>>,
}
impl InMemStore {
    fn new(fx: Arc<Mutex<Fx>>) -> Arc<Self> {
        Arc::new(Self {
            fx,
            ..Default::default()
        })
    }
}
#[async_trait::async_trait]
impl IsolationStore for InMemStore {
    async fn get_by_id(&self, id: &str) -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        Ok(self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(id).cloned())
    }
    async fn find_active_by_workflow(
        &self,
        codebase_id: &str,
        wt: IsolationWorkflowType,
        wid: &str,
    ) -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        let rows = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(rows
            .values()
            .find(|r| {
                r.codebase_id == codebase_id
                    && r.workflow_type == wt
                    && r.workflow_id == wid
                    && r.status == EnvironmentStatus::Active
            })
            .cloned())
    }
    async fn create(
        &self,
        env: CreateEnvironmentParams,
    ) -> har_isolation::Result<IsolationEnvironmentRow> {
        self.fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).create_calls += 1;
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let row = IsolationEnvironmentRow {
            id: format!("env-{n}"),
            codebase_id: env.codebase_id,
            workflow_type: env.workflow_type,
            workflow_id: env.workflow_id,
            provider: env.provider.unwrap_or(IsolationProviderType::Worktree),
            working_path: env.working_path,
            branch_name: env.branch_name,
            status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(),
            created_by_platform: env.created_by_platform,
            created_by_user_id: env.created_by_user_id,
            metadata: env.metadata.unwrap_or_default(),
        };
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(row.id.clone(), row.clone());
        Ok(row)
    }
    async fn update_status(&self, id: &str, status: &str) -> har_isolation::Result<()> {
        self.fx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_status_calls
            .push((id.to_string(), status.to_string()));
        let s = match status {
            "active" => EnvironmentStatus::Active,
            "destroyed" => EnvironmentStatus::Destroyed,
            o => return Err(har_isolation::IsolationError::InvalidStatus(o.to_string())),
        };
        if let Some(r) = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_mut(id) {
            r.status = s;
        }
        Ok(())
    }
    async fn count_active_by_codebase(&self, codebase_id: &str) -> har_isolation::Result<u32> {
        Ok(self
            .rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|r| r.codebase_id == codebase_id && r.status == EnvironmentStatus::Active)
            .count() as u32)
    }
}

struct MockProvider {
    fx: Arc<Mutex<Fx>>,
    create_branch: String,
}
#[async_trait::async_trait]
impl IsolationProvider for MockProvider {
    fn provider_type(&self) -> IsolationProviderType {
        IsolationProviderType::Worktree
    }
    async fn create(&self, _r: IsolationRequest) -> har_isolation::Result<WorktreeEnvironment> {
        Ok(WorktreeEnvironment {
            id: "/new/wt".into(),
            provider: "worktree".into(),
            working_path: "/new/wt".into(),
            branch_name: self.create_branch.clone(),
            status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(),
            warnings: None,
            metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                adopted: false,
                adopted_from: None,
                request: None,
            }),
        })
    }
    async fn destroy(
        &self,
        e: &str,
        _o: Option<DestroyOptions>,
    ) -> har_isolation::Result<DestroyResult> {
        self.fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).destroy_calls.push(e.to_string());
        Ok(DestroyResult {
            worktree_removed: true,
            branch_deleted: None,
            remote_branch_deleted: None,
            directory_clean: true,
            warnings: vec![],
        })
    }
    async fn get(&self, _e: &str) -> har_isolation::Result<Option<WorktreeEnvironment>> {
        Ok(None)
    }
    async fn list(&self, _c: &str) -> har_isolation::Result<Vec<WorktreeEnvironment>> {
        Ok(vec![])
    }
    async fn health_check(&self, _e: &str) -> har_isolation::Result<bool> {
        Ok(true)
    }
}

fn git(cwd: &str, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn mk(store: Arc<dyn IsolationStore>, fx: Arc<Mutex<Fx>>, branch: &str) -> IsolationResolver {
    IsolationResolver::new(IsolationResolverDeps {
        store,
        provider: Arc::new(MockProvider {
            fx,
            create_branch: branch.to_string(),
        }),
        cleanup: None,
        stale_threshold_days: None,
    })
    .unwrap()
}

async fn seed(
    store: &Arc<dyn IsolationStore>,
    wt: IsolationWorkflowType,
    wid: &str,
    path: &str,
    branch: &str,
) -> String {
    store
        .create(CreateEnvironmentParams {
            codebase_id: "cdb-001".into(),
            workflow_type: wt,
            workflow_id: wid.into(),
            provider: Some(IsolationProviderType::Worktree),
            working_path: path.into(),
            branch_name: branch.into(),
            created_by_platform: Some("slack".into()),
            created_by_user_id: None,
            metadata: None,
        })
        .await
        .unwrap()
        .id
}

fn cb(repo: &str) -> CodebaseSummary {
    CodebaseSummary {
        id: "cdb-001".into(),
        default_cwd: repo.into(),
        name: "owner/repo".into(),
    }
}

// FAIL-1: stale stage-1 path → store.update_status(id,"destroyed"), NOT provider.destroy.
#[tokio::test]
async fn fail1_stale_calls_store_update_status_not_provider_destroy() {
    let fx = Arc::new(Mutex::new(Fx::default()));
    let store: Arc<dyn IsolationStore> = InMemStore::new(fx.clone());
    let id = seed(
        &store,
        IsolationWorkflowType::Thread,
        "w1",
        "/totally/missing/wt",
        "archon/thread-w1",
    )
    .await;
    // reset create bookkeeping from seeding
    fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).create_calls = 0;

    let res = mk(store.clone(), fx.clone(), "archon/thread-w1")
        .resolve(ResolveRequest {
            existing_env_id: Some(id.clone()),
            codebase: None,
            hints: None,
            platform_type: "slack".into(),
            user_id: None,
            git_identity: None,
        })
        .await
        .unwrap();

    assert!(
        matches!(res, IsolationResolution::StaleCleaned { ref previous_env_id } if *previous_env_id == id)
    );
    let g = fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // TS oracle: updateStatusCalls = [{id, "destroyed"}], destroyCalls = []
    assert_eq!(
        g.update_status_calls,
        vec![(id.clone(), "destroyed".to_string())],
        "must mark DB row destroyed"
    );
    assert!(
        g.destroy_calls.is_empty(),
        "must NOT physically destroy worktree, got {:?}",
        g.destroy_calls
    );
}

// FAIL-5: cross-clone reuse PROPAGATES (Err) — no create, no destroy.
#[tokio::test]
async fn fail5_cross_clone_reuse_propagates_no_create() {
    let root = tempfile::tempdir().unwrap();
    let repo1 = root.path().join("repo1").to_string_lossy().to_string();
    let repo2 = root.path().join("repo2").to_string_lossy().to_string();
    std::fs::create_dir_all(&repo1).unwrap();
    std::fs::create_dir_all(&repo2).unwrap();
    for repo in [&repo1, &repo2] {
        let out = Command::new("git")
            .args(["init", "-q", "-b", "main", repo])
            .output()
            .unwrap();
        assert!(out.status.success());
        git(repo, &["config", "user.email", "t@t.com"]);
        git(repo, &["config", "user.name", "T"]);
        git(repo, &["commit", "--allow-empty", "-q", "-m", "init"]);
    }
    let wt2 = root.path().join("wt2").to_string_lossy().to_string();
    git(
        &repo2,
        &["worktree", "add", "-q", "-b", "cross-branch", &wt2],
    );

    let fx = Arc::new(Mutex::new(Fx::default()));
    let store: Arc<dyn IsolationStore> = InMemStore::new(fx.clone());
    // active workflow row for cdb-001 (repo1) points at repo2's worktree
    seed(
        &store,
        IsolationWorkflowType::Thread,
        "wX",
        &wt2,
        "cross-branch",
    )
    .await;
    fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).create_calls = 0;

    let res = mk(store.clone(), fx.clone(), "archon/thread-wX")
        .resolve(ResolveRequest {
            existing_env_id: None,
            codebase: Some(cb(&repo1)),
            hints: Some(IsolationHints {
                workflow_type: Some(IsolationWorkflowType::Thread),
                workflow_id: Some("wX".into()),
                ..Default::default()
            }),
            platform_type: "slack".into(),
            user_id: None,
            git_identity: None,
        })
        .await;

    // TS oracle: threw == true, createCalls == 0
    assert!(
        res.is_err(),
        "cross-clone reuse must propagate Err, got {res:?}"
    );
    let g = fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        g.create_calls, 0,
        "must NOT fall through to create on cross-clone conflict"
    );
}

// FAIL-3 (empty-default): no baseBranch hint → NO warnings emitted (warnings == None).
#[tokio::test]
async fn fail3_no_basebranch_hint_emits_no_warning() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo").to_string_lossy().to_string();
    std::fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-q", "-b", "main", &repo])
        .output()
        .unwrap();
    assert!(out.status.success());
    git(&repo, &["config", "user.email", "t@t.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["commit", "--allow-empty", "-q", "-m", "init"]);
    let wt = root.path().join("wt").to_string_lossy().to_string();
    git(&repo, &["worktree", "add", "-q", "-b", "feature/x", &wt]);

    let fx = Arc::new(Mutex::new(Fx::default()));
    let store: Arc<dyn IsolationStore> = InMemStore::new(fx.clone());
    seed(
        &store,
        IsolationWorkflowType::Thread,
        "w1",
        &wt,
        "feature/x",
    )
    .await;

    let res = mk(store.clone(), fx.clone(), "feature/x")
        .resolve(ResolveRequest {
            existing_env_id: None,
            codebase: Some(cb(&repo)),
            hints: Some(IsolationHints {
                workflow_type: Some(IsolationWorkflowType::Thread),
                workflow_id: Some("w1".into()),
                // NO base_branch
                ..Default::default()
            }),
            platform_type: "slack".into(),
            user_id: None,
            git_identity: None,
        })
        .await
        .unwrap();
    // TS oracle: resolved/workflow_reuse, "warnings" key ABSENT
    match res {
        IsolationResolution::Resolved(p) => {
            assert!(
                p.warnings.is_none(),
                "no-hint must emit no warnings, got {:?}",
                p.warnings
            );
        }
        other => panic!("expected resolved/workflow_reuse, got {other:?}"),
    }
}

// FAIL-4 (trigger): suggestedBranch set but prBranch absent → adoption does NOT trigger; falls to create.
#[tokio::test]
async fn fail4_suggested_branch_only_does_not_adopt() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo").to_string_lossy().to_string();
    std::fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-q", "-b", "main", &repo])
        .output()
        .unwrap();
    assert!(out.status.success());
    git(&repo, &["config", "user.email", "t@t.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["commit", "--allow-empty", "-q", "-m", "init"]);
    let wt = root.path().join("wt").to_string_lossy().to_string();
    git(&repo, &["worktree", "add", "-q", "-b", "feature/x", &wt]);

    let fx = Arc::new(Mutex::new(Fx::default()));
    let store: Arc<dyn IsolationStore> = InMemStore::new(fx.clone());

    let res = mk(store.clone(), fx.clone(), "archon/thread-fresh")
        .resolve(ResolveRequest {
            existing_env_id: None,
            codebase: Some(cb(&repo)),
            hints: Some(IsolationHints {
                workflow_type: Some(IsolationWorkflowType::Thread),
                workflow_id: Some("sbonly".into()),
                suggested_branch: Some("feature/x".into()),
                // NO pr_branch
                ..Default::default()
            }),
            platform_type: "slack".into(),
            user_id: None,
            git_identity: None,
        })
        .await
        .unwrap();
    // TS oracle: resolved/created (NOT branch_adoption)
    match res {
        IsolationResolution::Resolved(p) => {
            assert!(
                matches!(
                    p.method,
                    har_isolation::types::ResolutionMethod::Created { .. }
                ),
                "suggestedBranch-only must fall to create, got {:?}",
                p.method
            );
        }
        other => panic!("expected resolved/created, got {other:?}"),
    }
}

// STAGE-6 ORPHAN CLEANUP: store.create failure → provider.destroy(workingPath),
// NOT store.update_status. TS golden (resolver.ts:536-541):
//   destroyCalls = ["/orphan/wt"], updateStatusCalls = [].
struct FailingCreateStore {
    fx: Arc<Mutex<Fx>>,
}
#[async_trait::async_trait]
impl IsolationStore for FailingCreateStore {
    async fn get_by_id(&self, _id: &str) -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        Ok(None)
    }
    async fn find_active_by_workflow(
        &self,
        _c: &str,
        _w: IsolationWorkflowType,
        _i: &str,
    ) -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        Ok(None)
    }
    async fn create(
        &self,
        _e: CreateEnvironmentParams,
    ) -> har_isolation::Result<IsolationEnvironmentRow> {
        Err(har_isolation::IsolationError::Other(
            "DB write failed (simulated)".into(),
        ))
    }
    async fn update_status(&self, id: &str, status: &str) -> har_isolation::Result<()> {
        self.fx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_status_calls
            .push((id.to_string(), status.to_string()));
        Ok(())
    }
    async fn count_active_by_codebase(&self, _c: &str) -> har_isolation::Result<u32> {
        Ok(0)
    }
}

/// NEW DIVERGENCE found during cycle-10 re-verification (FAIL-1-followon).
/// The FAIL-1 fix repurposed `mark_destroyed_best_effort` to call
/// `store.update_status`, which is correct for the stale-cleanup paths
/// (stages 1/3/4). But stage-6 orphan cleanup (resolver.rs:432) still calls
/// that same helper, so it now does the WRONG side effect: it marks a DB row
/// (with a filesystem PATH as the id) instead of physically destroying the
/// orphaned worktree. TS golden (resolver.ts:536-541) calls
/// `provider.destroy(workingPath, {force:true})`.
/// Asserts the TS golden → passes after resolver.rs:432 was fixed to call provider.destroy.
#[tokio::test]
async fn stage6_orphan_cleanup_uses_provider_destroy_not_store_update() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo").to_string_lossy().to_string();
    std::fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-q", "-b", "main", &repo])
        .output()
        .unwrap();
    assert!(out.status.success());
    git(&repo, &["config", "user.email", "t@t.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["commit", "--allow-empty", "-q", "-m", "init"]);

    let fx = Arc::new(Mutex::new(Fx::default()));
    let store: Arc<dyn IsolationStore> = Arc::new(FailingCreateStore { fx: fx.clone() });

    let res = mk(store.clone(), fx.clone(), "archon/thread-x")
        .resolve(ResolveRequest {
            existing_env_id: None,
            codebase: Some(cb(&repo)),
            hints: Some(IsolationHints {
                workflow_type: Some(IsolationWorkflowType::Thread),
                workflow_id: Some("x".into()),
                ..Default::default()
            }),
            platform_type: "slack".into(),
            user_id: None,
            git_identity: None,
        })
        .await;

    assert!(
        res.is_err(),
        "store-create failure must propagate, got {res:?}"
    );
    let g = fx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // TS golden: provider.destroy called with the orphan worktree path; no store.update_status.
    assert_eq!(g.destroy_calls, vec!["/new/wt".to_string()],
        "orphan cleanup must call provider.destroy(workingPath); got destroy={:?} update_status={:?}",
        g.destroy_calls, g.update_status_calls);
    assert!(
        g.update_status_calls.is_empty(),
        "orphan cleanup must NOT call store.update_status; got {:?}",
        g.update_status_calls
    );
}
