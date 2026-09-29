//! Direct task assignment to a tenant.
//!
//! An operator can push work into a tenant without waiting for a GitHub issue
//! by posting a task. Tasks are appended to the tenant-scoped `ns:{tenant}:tasks`
//! array so they are visible to (and consumed by) the tenant's controller.

use crate::server::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pocketflow_core::SharedStore;
use serde::{Deserialize, Serialize};

/// Fully-qualified Redis key holding a tenant's assigned tasks.
pub fn tasks_key(tenant: &str) -> String {
    format!("ns:{}:tasks", tenant)
}

/// A unit of work assigned directly to a tenant through the control surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    /// Opaque caller-supplied payload (e.g. structured task fields).
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: u64,
}

/// Request body for `POST /api/v1/tenants/{name}/tasks`.
#[derive(Debug, Deserialize)]
pub struct AssignTaskRequest {
    /// Human-readable task title.
    pub title: String,
    /// Optional detail / acceptance text.
    #[serde(default)]
    pub body: String,
    /// Where the task came from (e.g. `cli`, `api`).
    #[serde(default = "default_source")]
    pub source: String,
    /// Optional structured payload carried alongside the task.
    #[serde(default)]
    pub payload: serde_json::Value,
}

fn default_source() -> String {
    "api".to_string()
}

/// Append a task to a tenant's task list and return it.
pub async fn append_task(store: &SharedStore, tenant: &str, req: AssignTaskRequest) -> Task {
    let mut tasks: Vec<Task> = store
        .raw_get(&tasks_key(tenant))
        .await
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let task = Task {
        id: generate_task_id(&tasks),
        title: req.title,
        body: req.body,
        source: req.source,
        payload: req.payload,
        created_at: now_millis(),
    };
    tasks.push(task.clone());
    store
        .raw_set(
            &tasks_key(tenant),
            serde_json::to_value(&tasks).unwrap_or_default(),
        )
        .await;
    task
}

fn generate_task_id(tasks: &[Task]) -> String {
    // A short, deterministic-ish human-friendly id that grows with existing
    // tasks. Collision-free for practical purposes within a single tenant.
    let n = tasks.len() + 1;
    format!("task-{n:03}")
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// POST /api/v1/tenants/{name}/tasks
pub async fn assign_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<AssignTaskRequest>,
) -> impl IntoResponse {
    if req.title.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "title is required" })),
        );
    }
    let task = append_task(state.store(), &name, req).await;
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "tenant": name, "task": task })),
    )
}
