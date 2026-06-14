//! Cycle-10 differential parity for IS-03 IsolationResolver cascade.
//!
//! Drives the SAME scenarios as the bun source oracle
//! (`packages/isolation/src/resolver.ts` via `__parity_resolver_oracle.ts`) over
//! REAL temp git repos + a real linked worktree, and asserts the resolution
//! stage/status matches the TS golden. Where the Rust port diverges from source,
//! the assertion is written to the TS golden and will FAIL — that is the gate.

use std::process::Command;
use std::sync::Arc;

use har_isolation::resolver::{IsolationResolver, IsolationResolverDeps};
use har_isolation::store::IsolationStore;
use har_isolation::types::{
    CodebaseSummary, CreateEnvironmentParams, DestroyOptions, DestroyResult, IsolationEnvironmentRow,
    IsolationHints, IsolationProvider, IsolationProviderType, IsolationRequest, IsolationResolution,
    IsolationWorkflowType, ResolveRequest, ResolutionMethod, WorktreeEnvironment,
    WorktreeMetadata, AdoptedWorktreeMetadata, EnvironmentStatus,
};

// ─── Minimal in-memory store (integration-test local; mirrors test_support) ──
#[derive(Default)]
struct InMemStore {
    rows: std::sync::Mutex<std::collections::HashMap<String, IsolationEnvironmentRow>>,
    seq: std::sync::atomic::AtomicU64,
}
impl InMemStore {
    fn new() -> Arc<Self> { Arc::new(Self::default()) }
}
#[async_trait::async_trait]
impl IsolationStore for InMemStore {
    async fn get_by_id(&self, id: &str) -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        Ok(self.rows.lock().unwrap().get(id).cloned())
    }
    async fn find_active_by_workflow(&self, codebase_id: &str, wt: IsolationWorkflowType, wid: &str)
        -> har_isolation::Result<Option<IsolationEnvironmentRow>> {
        let rows = self.rows.lock().unwrap();
        Ok(rows.values().find(|r| r.codebase_id == codebase_id && r.workflow_type == wt
            && r.workflow_id == wid && r.status == EnvironmentStatus::Active).cloned())
    }
    async fn create(&self, env: CreateEnvironmentParams) -> har_isolation::Result<IsolationEnvironmentRow> {
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let row = IsolationEnvironmentRow {
            id: format!("env-{n}"), codebase_id: env.codebase_id, workflow_type: env.workflow_type,
            workflow_id: env.workflow_id, provider: env.provider.unwrap_or(IsolationProviderType::Worktree),
            working_path: env.working_path, branch_name: env.branch_name,
            status: EnvironmentStatus::Active, created_at: chrono::Utc::now(),
            created_by_platform: env.created_by_platform, created_by_user_id: env.created_by_user_id,
            metadata: env.metadata.unwrap_or_default(),
        };
        self.rows.lock().unwrap().insert(row.id.clone(), row.clone());
        Ok(row)
    }
    async fn update_status(&self, id: &str, status: &str) -> har_isolation::Result<()> {
        let s = match status { "active" => EnvironmentStatus::Active, "destroyed" => EnvironmentStatus::Destroyed,
            o => return Err(har_isolation::IsolationError::InvalidStatus(o.to_string())) };
        if let Some(r) = self.rows.lock().unwrap().get_mut(id) { r.status = s; }
        Ok(())
    }
    async fn count_active_by_codebase(&self, codebase_id: &str) -> har_isolation::Result<u32> {
        Ok(self.rows.lock().unwrap().values().filter(|r| r.codebase_id == codebase_id && r.status == EnvironmentStatus::Active).count() as u32)
    }
}

fn git(cwd: &str, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

struct Repo {
    _root: tempfile::TempDir,
    repo: String,
    worktree: String,
    branch: String,
}

fn setup_repo() -> Repo {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let repo_s = repo.to_string_lossy().to_string();
    let out = Command::new("git").args(["init", "-q", "-b", "main", &repo_s]).output().unwrap();
    assert!(out.status.success());
    git(&repo_s, &["config", "user.email", "t@t.com"]);
    git(&repo_s, &["config", "user.name", "T"]);
    git(&repo_s, &["commit", "--allow-empty", "-q", "-m", "init"]);
    let branch = "feature/x".to_string();
    let worktree = root.path().join("wt").to_string_lossy().to_string();
    git(&repo_s, &["worktree", "add", "-q", "-b", &branch, &worktree]);
    Repo { _root: root, repo: repo_s, worktree, branch }
}

#[derive(Clone)]
struct MockProvider {
    create_branch: String,
}
#[async_trait::async_trait]
impl IsolationProvider for MockProvider {
    fn provider_type(&self) -> IsolationProviderType { IsolationProviderType::Worktree }
    async fn create(&self, _r: IsolationRequest) -> har_isolation::Result<WorktreeEnvironment> {
        Ok(WorktreeEnvironment {
            id: "/new/wt".into(), provider: "worktree".into(), working_path: "/new/wt".into(),
            branch_name: self.create_branch.clone(), status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(), warnings: None,
            metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata { adopted: false, adopted_from: None, request: None }),
        })
    }
    async fn destroy(&self, _e: &str, _o: Option<DestroyOptions>) -> har_isolation::Result<DestroyResult> {
        Ok(DestroyResult { worktree_removed: true, branch_deleted: None, remote_branch_deleted: None, directory_clean: true, warnings: vec![] })
    }
    async fn get(&self, _e: &str) -> har_isolation::Result<Option<WorktreeEnvironment>> { Ok(None) }
    async fn list(&self, _c: &str) -> har_isolation::Result<Vec<WorktreeEnvironment>> { Ok(vec![]) }
    async fn health_check(&self, _e: &str) -> har_isolation::Result<bool> { Ok(true) }
}

fn resolver(store: Arc<dyn IsolationStore>, branch: &str) -> IsolationResolver {
    IsolationResolver::new(IsolationResolverDeps {
        store,
        provider: Arc::new(MockProvider { create_branch: branch.to_string() }),
        cleanup: None,
        stale_threshold_days: None,
    }).unwrap()
}

fn cb(repo: &str) -> CodebaseSummary {
    CodebaseSummary { id: "cdb-001".into(), default_cwd: repo.into(), name: "owner/repo".into() }
}

fn method_str(r: &IsolationResolution) -> String {
    match r {
        IsolationResolution::Resolved(p) => match &p.method {
            ResolutionMethod::Existing => "resolved/existing".into(),
            ResolutionMethod::WorkflowReuse => "resolved/workflow_reuse".into(),
            ResolutionMethod::LinkedIssueReuse { issue_number } => format!("resolved/linked_issue_reuse:{issue_number}"),
            ResolutionMethod::BranchAdoption { branch } => format!("resolved/branch_adoption:{branch}"),
            ResolutionMethod::Created { .. } => "resolved/created".into(),
        },
        IsolationResolution::StaleCleaned { .. } => "stale_cleaned".into(),
        IsolationResolution::None { .. } => "none".into(),
        IsolationResolution::Blocked { .. } => "blocked".into(),
    }
}

// Seed an env row at a chosen working_path/branch via the store's create().
async fn seed(store: &Arc<dyn IsolationStore>, wt: IsolationWorkflowType, wid: &str, path: &str, branch: &str) {
    store.create(CreateEnvironmentParams {
        codebase_id: "cdb-001".into(), workflow_type: wt, workflow_id: wid.into(),
        provider: Some(IsolationProviderType::Worktree), working_path: path.into(),
        branch_name: branch.into(), created_by_platform: Some("slack".into()),
        created_by_user_id: None, metadata: None,
    }).await.unwrap();
}

#[tokio::test]
async fn stage1_existing_resolved() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    seed(&store, IsolationWorkflowType::Thread, "w1", &r.worktree, &r.branch).await;
    // grab the seeded id
    let id = {
        let row = store.find_active_by_workflow("cdb-001", IsolationWorkflowType::Thread, "w1").await.unwrap().unwrap();
        row.id
    };
    let res = resolver(store, &r.branch).resolve(ResolveRequest {
        existing_env_id: Some(id), codebase: Some(cb(&r.repo)),
        hints: None, platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    // TS golden: resolved/existing
    assert_eq!(method_str(&res), "resolved/existing", "stage1 existing");
}

#[tokio::test]
async fn stage1b_missing_worktree_is_stale_cleaned() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    seed(&store, IsolationWorkflowType::Thread, "w1", "/totally/missing/wt", "archon/thread-w1").await;
    let id = store.find_active_by_workflow("cdb-001", IsolationWorkflowType::Thread, "w1").await.unwrap().unwrap().id;
    let res = resolver(store, "archon/thread-w1").resolve(ResolveRequest {
        existing_env_id: Some(id), codebase: Some(cb(&r.repo)),
        hints: None, platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    // TS golden: stale_cleaned (NOT a fall-through to create/none)
    assert_eq!(method_str(&res), "stale_cleaned", "stage1b stale_cleaned");
}

#[tokio::test]
async fn stage2_no_codebase_is_none_with_workspace_cwd() {
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    let res = resolver(store, "b").resolve(ResolveRequest {
        existing_env_id: None, codebase: None, hints: None,
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    // TS golden: { status: none, cwd: '/workspace' }
    match res {
        IsolationResolution::None { cwd } => assert_eq!(cwd, "/workspace", "stage2 cwd"),
        other => panic!("expected None, got {other:?}"),
    }
}

#[tokio::test]
async fn stage3_workflow_reuse() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    seed(&store, IsolationWorkflowType::Thread, "w1", &r.worktree, &r.branch).await;
    let res = resolver(store, &r.branch).resolve(ResolveRequest {
        existing_env_id: None, codebase: Some(cb(&r.repo)),
        hints: Some(IsolationHints { workflow_type: Some(IsolationWorkflowType::Thread), workflow_id: Some("w1".into()), ..Default::default() }),
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    assert_eq!(method_str(&res), "resolved/workflow_reuse", "stage3 reuse");
}

#[tokio::test]
async fn stage3_basebranch_mismatch_warning_text() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    seed(&store, IsolationWorkflowType::Thread, "w1", &r.worktree, &r.branch).await;
    let res = resolver(store, &r.branch).resolve(ResolveRequest {
        existing_env_id: None, codebase: Some(cb(&r.repo)),
        hints: Some(IsolationHints { workflow_type: Some(IsolationWorkflowType::Thread), workflow_id: Some("w1".into()), base_branch: Some("nonexistent-base".into()), ..Default::default() }),
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    if let IsolationResolution::Resolved(p) = &res {
        let w = p.warnings.clone().unwrap_or_default();
        assert!(!w.is_empty(), "expected a base-branch warning");
        // TS golden warning string (byte-for-byte):
        assert_eq!(
            w[0],
            "Worktree branch 'feature/x' is not based on 'nonexistent-base'. Recreate with: archon complete feature/x --force",
            "stage3 warning text"
        );
    } else { panic!("expected resolved, got {res:?}"); }
}

#[tokio::test]
async fn stage4_linked_issue_reuse() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    seed(&store, IsolationWorkflowType::Issue, "99", &r.worktree, &r.branch).await;
    let res = resolver(store, &r.branch).resolve(ResolveRequest {
        existing_env_id: None, codebase: Some(cb(&r.repo)),
        hints: Some(IsolationHints { workflow_type: Some(IsolationWorkflowType::Thread), workflow_id: Some("wX".into()), linked_issues: Some(vec![99]), ..Default::default() }),
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    assert_eq!(method_str(&res), "resolved/linked_issue_reuse:99", "stage4 linked");
}

#[tokio::test]
async fn stage5_branch_adoption() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    let res = resolver(Arc::clone(&store), &r.branch).resolve(ResolveRequest {
        existing_env_id: None, codebase: Some(cb(&r.repo)),
        hints: Some(IsolationHints { workflow_type: Some(IsolationWorkflowType::Pr), workflow_id: Some("pr1".into()), pr_branch: Some(r.branch.clone()), ..Default::default() }),
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    assert_eq!(method_str(&res), format!("resolved/branch_adoption:{}", r.branch), "stage5 adopt");
    // TS golden: store.create called with metadata {adopted:true, adopted_from:'skill'}
    let row = store.find_active_by_workflow("cdb-001", IsolationWorkflowType::Pr, "pr1").await.unwrap()
        .expect("adoption should persist a row");
    let meta = serde_json::to_value(&row.metadata).unwrap();
    assert_eq!(meta.get("adopted").and_then(|v| v.as_bool()), Some(true), "adopted flag");
    assert_eq!(meta.get("adopted_from").and_then(|v| v.as_str()), Some("skill"), "adopted_from");
}

#[tokio::test]
async fn stage6_create_new() {
    let r = setup_repo();
    let store: Arc<dyn IsolationStore> = InMemStore::new();
    let res = resolver(store, "archon/thread-fresh").resolve(ResolveRequest {
        existing_env_id: None, codebase: Some(cb(&r.repo)),
        hints: Some(IsolationHints { workflow_type: Some(IsolationWorkflowType::Thread), workflow_id: Some("fresh".into()), ..Default::default() }),
        platform_type: "slack".into(), user_id: None, git_identity: None,
    }).await.unwrap();
    assert_eq!(method_str(&res), "resolved/created", "stage6 create");
}
