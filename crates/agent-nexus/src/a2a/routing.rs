// crates/agent-nexus/src/a2a/routing.rs
//! Pair-scoped routing table and session management for the A2A relay.
//!
//! Maintains connections from Sentinel and Forge workspaces, keyed by
//! `(pair_id, role)`. Routes verify requests from Sentinel to the
//! corresponding Forge executor.

use a2a_protocol::{VerifyRequest, VerifyResult};
use anyhow::{anyhow, Result};
use pocketflow_core::SharedStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, warn};

/// A2A relay session for a single connected workspace. Holds task queue
/// and result channel.
#[derive(Clone)]
pub struct A2ASession {
    pub pair_id: String,
    pub role: String,        // "sentinel" or "forge"
    pub workspace_id: String, // For audit; e.g., "forge-T-048"
    // TODO: event channel for streaming progress back to workspace
}

/// Pair-scoped task entry, keyed by idempotency hash.
#[derive(Clone)]
pub struct TaskEntry {
    pub task_id: String,
    pub request: VerifyRequest,
    pub idempotency_key: String,
    // TODO: result channel, progress channel
}

/// Central A2A relay hosted by nexus. Manages pair-scoped routing,
/// command allowlist validation, idempotency dedup, and result mirroring
/// to Redis.
pub struct A2ARelay {
    store: Arc<SharedStore>,
    // (pair_id, role) → connected session
    sessions: Arc<RwLock<HashMap<(String, String), A2ASession>>>,
    // (pair_id, idempotency_key) → task entry (for dedup)
    // TODO: add TTL or bounded eviction
    tasks: Arc<Mutex<HashMap<String, TaskEntry>>>,
}

impl A2ARelay {
    /// Create a new A2A relay.
    pub fn new(store: Arc<SharedStore>) -> Self {
        Self {
            store,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a connected workspace session.
    pub async fn register_session(
        &self,
        pair_id: String,
        role: String,
        workspace_id: String,
    ) -> Result<()> {
        let key = (pair_id.clone(), role.clone());
        let session = A2ASession {
            pair_id: pair_id.clone(),
            role: role.clone(),
            workspace_id,
        };
        self.sessions.write().await.insert(key.clone(), session);
        debug!(pair_id = %key.0, role = %key.1, "Session registered");
        Ok(())
    }

    /// Deregister a connected workspace session.
    pub async fn deregister_session(&self, pair_id: &str, role: &str) -> Result<()> {
        let key = (pair_id.to_string(), role.to_string());
        self.sessions.write().await.remove(&key);
        debug!(pair_id, role, "Session deregistered");
        Ok(())
    }

    /// Get a registered session for a given pair and role.
    pub async fn get_session(&self, pair_id: &str, role: &str) -> Option<A2ASession> {
        let key = (pair_id.to_string(), role.to_string());
        self.sessions.read().await.get(&key).cloned()
    }

    /// Validate a verify request: check allowlist, cwd, pair_id match.
    /// Returns error if validation fails (caller logs to audit:a2a:rejected).
    ///
    /// Validation rules (task 2.2):
    /// 1. Pair IDs must match (Sentinel can only submit requests for its own pair)
    /// 2. Command must be in the allowlist
    /// 3. cwd must be one of the allowed values ("repo", "worktree")
    /// 4. timeout_secs must be reasonable (not negative, not excessive)
    pub fn validate_verify_request(
        &self,
        req: &VerifyRequest,
        requester_pair_id: &str,
    ) -> Result<()> {
        // Rule 1: Pair IDs must match
        if req.pair_id != requester_pair_id {
            return Err(anyhow!(
                "pair_id mismatch: request says {}, requester is {}",
                req.pair_id,
                requester_pair_id
            ));
        }

        // Rule 2: Command allowlist
        if !a2a_protocol::is_allowlisted(&req.argv) {
            return Err(anyhow!(
                "command not allowlisted: {}",
                req.argv.join(" ")
            ));
        }

        // Rule 3: cwd validation
        // Only "repo" and "worktree" are allowed; nothing else.
        // The actual path traversal check (doesn't escape pair root) happens
        // in the executor (task 5), since nexus doesn't have workspace FS.
        use a2a_protocol::VerifyCwd;
        match req.cwd {
            VerifyCwd::Repo | VerifyCwd::Worktree => {}
            // If serde can't parse VerifyCwd, this is already rejected
            // by serde deserialization, not by this function.
        }

        // Rule 4: timeout_secs sanity check
        const MAX_TIMEOUT_SECS: u64 = 3600; // 1 hour is the hard limit
        if req.timeout_secs == 0 {
            return Err(anyhow!("timeout_secs must be > 0"));
        }
        if req.timeout_secs > MAX_TIMEOUT_SECS {
            return Err(anyhow!(
                "timeout_secs exceeds maximum ({})",
                MAX_TIMEOUT_SECS
            ));
        }

        Ok(())
    }

    /// Check idempotency: if a task with the same key was recently
    /// submitted, return its task_id. Otherwise, generate a new task_id
    /// and record it.
    pub async fn check_or_create_task(&self, req: &VerifyRequest) -> Result<String> {
        let idempotency_key = req.idempotency_seed()?;

        let mut tasks = self.tasks.lock().await;
        if let Some(entry) = tasks.get(&idempotency_key) {
            debug!(
                task_id = %entry.task_id,
                pair_id = %req.pair_id,
                "Task already exists (dedup)"
            );
            return Ok(entry.task_id.clone());
        }

        // Generate new task_id (uuid v4)
        let task_id = uuid::Uuid::new_v4().to_string();

        let entry = TaskEntry {
            task_id: task_id.clone(),
            request: req.clone(),
            idempotency_key,
        };
        tasks.insert(entry.idempotency_key.clone(), entry);

        debug!(
            task_id = %task_id,
            pair_id = %req.pair_id,
            "New task created"
        );
        Ok(task_id)
    }

    /// Mirror a terminal task result to Redis before acking completion.
    /// Writes to:
    /// - `pair:{pair_id}:verification` (latest result)
    /// - `audit:a2a:{task_id}:result` (immutable result artifact)
    /// - `audit:a2a:{task_id}:request` (original request for replay)
    pub async fn mirror_result(&self, result: &VerifyResult) -> Result<()> {
        let verification_key = a2a_protocol::verification_key(&result.executor.workspace);

        // Mirror to pair:{pair_id}:verification
        self.store
            .set(&verification_key, serde_json::to_value(result)?)
            .await;

        // Mirror to audit:a2a:{task_id}:result
        let audit_result_key = format!("{}:result", a2a_protocol::audit_task_key(&result.task_id));
        self.store
            .set(&audit_result_key, serde_json::to_value(result)?)
            .await;

        debug!(
            task_id = %result.task_id,
            pair_id = %result.executor.workspace,
            "Result mirrored to Redis"
        );
        Ok(())
    }

    /// Get a task's stored request for replay (used by resubscribe).
    pub async fn get_task_request(&self, task_id: &str) -> Option<VerifyRequest> {
        let audit_request_key = format!("{}:request", a2a_protocol::audit_task_key(task_id));
        match self
            .store
            .get(&audit_request_key)
            .await
            .and_then(|v| serde_json::from_value::<VerifyRequest>(v).ok())
        {
            Some(req) => Some(req),
            None => {
                warn!(task_id, "Request not found in audit trail");
                None
            }
        }
    }

    /// List all active sessions (for inspection/monitoring).
    pub async fn list_sessions(&self) -> Vec<A2ASession> {
        self.sessions
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// List all tracked tasks (for inspection/monitoring).
    pub async fn list_tasks(&self) -> Vec<TaskEntry> {
        self.tasks.lock().await.values().cloned().collect()
    }
}
