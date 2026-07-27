use crate::types::{CreateEnvironmentParams, IsolationEnvironmentRow, IsolationWorkflowType};
use crate::Result;
/// Isolation store interface (MAP→hf seam).
///
/// Ports `packages/isolation/src/store.ts`.
///
/// `IIsolationStore` defines the methods the resolver uses for durable
/// environment state. The concrete impl (MAP→hf) is a separate cycle.
/// Here we define the Rust trait + the data shapes it passes.
use async_trait::async_trait;

/// Durable isolation environment store.
///
/// Ports `IIsolationStore` from `store.ts:7-17`.
///
/// Methods:
/// - `get_by_id`            ← `getById(id)`
/// - `find_active_by_workflow` ← `findActiveByWorkflow(codebaseId, workflowType, workflowId)`
/// - `create`               ← `create(env)`
/// - `update_status`        ← `updateStatus(id, status)`
/// - `count_active_by_codebase` ← `countActiveByCodebase(codebaseId)`
#[async_trait]
pub trait IsolationStore: Send + Sync {
    /// Look up an environment by its id.
    /// Returns `None` when not found.
    async fn get_by_id(&self, id: &str) -> Result<Option<IsolationEnvironmentRow>>;

    /// Find the active environment for a given (codebase, workflow_type, workflow_id) triple.
    /// Returns `None` when no such record exists.
    async fn find_active_by_workflow(
        &self,
        codebase_id: &str,
        workflow_type: IsolationWorkflowType,
        workflow_id: &str,
    ) -> Result<Option<IsolationEnvironmentRow>>;

    /// Persist a new isolation environment record.
    async fn create(&self, env: CreateEnvironmentParams) -> Result<IsolationEnvironmentRow>;

    /// Update the `status` of an existing record.
    async fn update_status(&self, id: &str, status: &str) -> Result<()>;

    /// Count active environments for a codebase (used by the resolver for
    /// `makeRoom` / capacity-check logic).
    async fn count_active_by_codebase(&self, codebase_id: &str) -> Result<u32>;
}

/// In-memory stub of `IIsolationStore` for testing.
///
/// The real hf-backed impl is a future MAP cycle.
#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::types::{EnvironmentStatus, IsolationProviderType};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Simple in-memory store for unit tests.
    #[derive(Default)]
    pub struct InMemoryIsolationStore {
        rows: Mutex<HashMap<String, IsolationEnvironmentRow>>,
    }

    impl InMemoryIsolationStore {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }
    }

    #[async_trait]
    impl IsolationStore for InMemoryIsolationStore {
        async fn get_by_id(&self, id: &str) -> Result<Option<IsolationEnvironmentRow>> {
            Ok(self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(id).cloned())
        }

        async fn find_active_by_workflow(
            &self,
            codebase_id: &str,
            workflow_type: IsolationWorkflowType,
            workflow_id: &str,
        ) -> Result<Option<IsolationEnvironmentRow>> {
            let rows = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(rows
                .values()
                .find(|r| {
                    r.codebase_id == codebase_id
                        && r.workflow_type == workflow_type
                        && r.workflow_id == workflow_id
                        && r.status == EnvironmentStatus::Active
                })
                .cloned())
        }

        async fn create(&self, env: CreateEnvironmentParams) -> Result<IsolationEnvironmentRow> {
            let row = IsolationEnvironmentRow {
                id: format!("env-{}", uuid_v4()),
                codebase_id: env.codebase_id,
                workflow_type: env.workflow_type,
                workflow_id: env.workflow_id,
                provider: env.provider.unwrap_or(IsolationProviderType::Worktree),
                working_path: env.working_path,
                branch_name: env.branch_name,
                status: EnvironmentStatus::Active,
                created_at: Utc::now(),
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

        async fn update_status(&self, id: &str, status: &str) -> Result<()> {
            let new_status = match status {
                "active" => EnvironmentStatus::Active,
                "destroyed" => EnvironmentStatus::Destroyed,
                other => {
                    return Err(crate::IsolationError::InvalidStatus(other.to_string()));
                }
            };
            let mut rows = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(row) = rows.get_mut(id) {
                row.status = new_status;
            }
            Ok(())
        }

        async fn count_active_by_codebase(&self, codebase_id: &str) -> Result<u32> {
            let rows = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = rows
                .values()
                .filter(|r| r.codebase_id == codebase_id && r.status == EnvironmentStatus::Active)
                .count();
            Ok(count as u32)
        }
    }

    fn uuid_v4() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!("{:x}", t)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::InMemoryIsolationStore;
    use super::*;
    use crate::types::{CreateEnvironmentParams, IsolationWorkflowType};

    fn sample_params() -> CreateEnvironmentParams {
        CreateEnvironmentParams {
            codebase_id: "cdb-001".to_string(),
            workflow_type: IsolationWorkflowType::Issue,
            workflow_id: "42".to_string(),
            provider: None,
            working_path: "/tmp/worktrees/branch".to_string(),
            branch_name: "issue-42".to_string(),
            created_by_platform: Some("slack".to_string()),
            created_by_user_id: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_by_id() {
        let store = InMemoryIsolationStore::new();
        let row = store.create(sample_params()).await.unwrap();
        assert!(!row.id.is_empty());
        let found = store.get_by_id(&row.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().codebase_id, "cdb-001");
    }

    #[tokio::test]
    async fn get_by_id_returns_none_when_missing() {
        let store = InMemoryIsolationStore::new();
        let found = store.get_by_id("no-such-id").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn find_active_by_workflow_matches() {
        let store = InMemoryIsolationStore::new();
        store.create(sample_params()).await.unwrap();
        let found = store
            .find_active_by_workflow("cdb-001", IsolationWorkflowType::Issue, "42")
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn find_active_by_workflow_no_match() {
        let store = InMemoryIsolationStore::new();
        store.create(sample_params()).await.unwrap();
        let found = store
            .find_active_by_workflow("cdb-001", IsolationWorkflowType::Issue, "9999")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn update_status_changes_status() {
        let store = InMemoryIsolationStore::new();
        let row = store.create(sample_params()).await.unwrap();
        store.update_status(&row.id, "destroyed").await.unwrap();
        let updated = store.get_by_id(&row.id).await.unwrap().unwrap();
        assert_eq!(updated.status, crate::types::EnvironmentStatus::Destroyed);
    }

    #[tokio::test]
    async fn count_active_by_codebase() {
        let store = InMemoryIsolationStore::new();
        store.create(sample_params()).await.unwrap();

        let mut p2 = sample_params();
        p2.workflow_id = "43".to_string();
        store.create(p2).await.unwrap();

        let count = store.count_active_by_codebase("cdb-001").await.unwrap();
        assert_eq!(count, 2);

        let other = store.count_active_by_codebase("cdb-999").await.unwrap();
        assert_eq!(other, 0);
    }

    #[tokio::test]
    async fn count_excludes_destroyed() {
        let store = InMemoryIsolationStore::new();
        let row = store.create(sample_params()).await.unwrap();
        store.update_status(&row.id, "destroyed").await.unwrap();
        let count = store.count_active_by_codebase("cdb-001").await.unwrap();
        assert_eq!(count, 0);
    }
}
