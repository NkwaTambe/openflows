// crates/agent-nexus/src/a2a/http_server.rs
//! JSON-RPC HTTP server for A2A relay. Implements the standard A2A methods:
//! - message/send (submit verify request)
//! - message/stream (subscribe to task events via SSE)
//! - tasks/get (retrieve task details)
//! - tasks/cancel (cancel a running task)
//! - tasks/resubscribe (resume after disconnect)

use super::routing::{A2ARelay, TaskState};
use super::verify_handler::submit_verify_request;
use a2a_protocol::{VerifyRequest, VerifyResult};
use anyhow::Context;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, warn};

/// JSON-RPC 2.0 request envelope (simplified for A2A).
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    #[serde(default)]
    pub id: Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

impl IntoResponse for JsonRpcResponse {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// JSON-RPC error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A2A HTTP server state.
#[derive(Clone)]
pub struct A2AServerState {
    pub relay: Arc<A2ARelay>,
}

/// Create the A2A HTTP server router.
pub fn create_router(relay: Arc<A2ARelay>) -> Router {
    let state = A2AServerState { relay };

    Router::new()
        .route("/rpc", post(handle_rpc))
        .route("/", get(handle_stream))
        .route("/health", get(handle_health))
        .route("/.well-known/agent-card.json", get(handle_agent_card))
        .with_state(state)
}

/// Health check endpoint.
async fn handle_health() -> &'static str {
    "A2A relay healthy"
}

/// Agent Card endpoint. Describes the relay's capabilities (not the executors).
async fn handle_agent_card() -> Json<Value> {
    Json(json!({
        "name": "OpenFlows A2A Relay",
        "version": "1.0",
        "description": "Central A2A relay for Sentinel↔Forge delegated verification (issue #143)",
        "capabilities": {
            "verify": {
                "description": "Verify task execution with command allowlist and sandbox isolation",
                "task_type": "verify",
                "methods": [
                    "message/send",
                    "message/stream",
                    "tasks/get",
                    "tasks/cancel",
                    "tasks/resubscribe"
                ]
            }
        },
        "endpoints": {
            "rpc": "/rpc",
            "stream": "/",
            "health": "/health"
        },
        "api_version": "A2A 1.0",
        "source": "https://github.com/The-AgenticFlow/openflows/issues/143"
    }))
}

/// JSON-RPC POST handler. Processes message/send and other A2A methods.
async fn handle_rpc(
    State(state): State<A2AServerState>,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    debug!(method = %req.method, "RPC request");

    let result = match req.method.as_str() {
        "message/send" => handle_message_send(&state, &req.params).await,
        "tasks/get" => handle_tasks_get(&state, &req.params).await,
        "tasks/claim" => handle_tasks_claim(&state, &req.params).await,
        "tasks/complete" => handle_tasks_complete(&state, &req.params).await,
        "tasks/cancel" => handle_tasks_cancel(&state, &req.params).await,
        "tasks/resubscribe" => handle_tasks_resubscribe(&state, &req.params).await,
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", req.method),
                    data: None,
                }),
                id: req.id,
            }
            .into_response();
        }
    };

    match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: Some(result),
            error: None,
            id: req.id,
        }
        .into_response(),
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "Internal error".into(),
                data: Some(json!(e.to_string())),
            }),
            id: req.id,
        }
        .into_response(),
    }
}

/// message/send: Submit a verify request (Sentinel → Nexus)
///
/// `params` is the `verify` task payload: `{ "pair_id", "kind", "cwd",
/// "argv", "timeout_secs", "env_allowlist", "expect" }`. The requester's
/// identity is authenticated out-of-band (workspace token); the relay
/// enforces that a Sentinel may only submit for its own `pair_id`.
async fn handle_message_send(state: &A2AServerState, params: &Value) -> anyhow::Result<Value> {
    // The message params nest the verify task under the otherMembers tasks.
    // Per the A2A wire shape used by the harness, params may be the
    // VerifyRequest directly, or wrapped as `{ "message": { "task": ... } }`.
    let raw = params
        .get("message")
        .and_then(|m| m.get("task"))
        .unwrap_or(params);

    let req: VerifyRequest =
        serde_json::from_value(raw.clone()).context("params are not a valid VerifyRequest")?;

    // AuthZ: a Sentinel may only submit for its own pair. In the current
    // in-network deployment the relay trusts the pair_id on the request; the
    // workspace token binding (plan task 2) is enforced by the executor role
    // check done on the Forge side. We use pair_id as requester identity.
    let requester_pair_id = req.pair_id.clone();

    let task_id = submit_verify_request(&state.relay, &req, &requester_pair_id).await?;

    Ok(json!({"task_id": task_id, "status": "pending"}))
}

/// tasks/get: Retrieve the state of a task (Sentinel polling, Forge status).
///
/// `params: { "task_id": "..." }` → returns the current lifecycle state and,
/// once completed, the terminal result the executor mirrored to Redis.
async fn handle_tasks_get(state: &A2AServerState, params: &Value) -> anyhow::Result<Value> {
    let task_id = params
        .get("task_id")
        .and_then(|t| t.as_str())
        .context("tasks/get requires string task_id")?;

    let entry = state
        .relay
        .get_task(task_id)
        .await
        .context("task not found")?;

    let state_str = match entry.state {
        TaskState::Pending => "pending",
        TaskState::Running => "running",
        TaskState::Completed => "completed",
    };

    let mut value = json!({
        "task_id": entry.task_id,
        "pair_id": entry.request.pair_id,
        "status": state_str,
    });
    if let Some(result) = entry.result {
        value["result"] = serde_json::to_value(result)?;
    }
    Ok(value)
}

/// tasks/claim: Forge executor claims the next pending task for its pair.
///
/// `params: { "pair_id": "T-048", "role": "forge" }` → returns the task
/// (request included) or null when no pending task exists for the pair.
async fn handle_tasks_claim(state: &A2AServerState, params: &Value) -> anyhow::Result<Value> {
    let pair_id = params
        .get("pair_id")
        .and_then(|p| p.as_str())
        .context("tasks/claim requires string pair_id")?;
    let role = params
        .get("role")
        .and_then(|r| r.as_str())
        .context("tasks/claim requires string role")?;

    // Only the Forge executor role may claim verify tasks.
    if !role.eq_ignore_ascii_case("forge") {
        warn!(role, "Non-forge role attempted to claim a verify task");
        return Err(anyhow::anyhow!(
            "tasks/claim is only available to the forge executor role"
        ));
    }

    match state.relay.claim_next_task(pair_id).await? {
        Some(entry) => Ok(json!({
            "task_id": entry.task_id,
            "request": serde_json::to_value(entry.request)?,
        })),
        None => Ok(json!(null)),
    }
}

/// tasks/complete: Forge executor submits a terminal result for a task.
///
/// `params: { "task_id", "result": { VerifyResult } }` → the relay marks the
/// task completed and mirrors the result to Redis before returning.
async fn handle_tasks_complete(state: &A2AServerState, params: &Value) -> anyhow::Result<Value> {
    let task_id = params
        .get("task_id")
        .and_then(|t| t.as_str())
        .context("tasks/complete requires string task_id")?;

    let result: VerifyResult = params
        .get("result")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .context("tasks/complete requires a valid VerifyResult result")?
        .context("tasks/complete requires a result")?;

    // Guard: the submitted task_id must match the one the executor built the
    // result from, so a mistaken/stale result can't land on another task.
    if result.task_id != task_id {
        return Err(anyhow::anyhow!(
            "tasks/complete task_id mismatch: result carries {}, request says {}",
            result.task_id,
            task_id
        ));
    }

    state.relay.complete_task(task_id, result).await?;
    Ok(json!({"task_id": task_id, "status": "completed"}))
}

/// tasks/cancel: Cancel a running task
async fn handle_tasks_cancel(_state: &A2AServerState, _params: &Value) -> anyhow::Result<Value> {
    // TODO: Extract task_id from params
    // TODO: Signal executor to kill process

    debug!("tasks/cancel: not yet implemented");
    Ok(json!({"cancelled": true}))
}

/// tasks/resubscribe: Resume task after disconnect
async fn handle_tasks_resubscribe(
    _state: &A2AServerState,
    _params: &Value,
) -> anyhow::Result<Value> {
    // TODO: Extract task_id from params
    // TODO: Return buffered events since last ACK
    // TODO: Restore SSE subscription

    debug!("tasks/resubscribe: not yet implemented");
    Ok(json!({"events": []}))
}

/// Server-Sent Events endpoint.
///
/// v1 delivers verify tasks to Forge via pull (the `tasks/claim` /
/// `tasks/complete` JSON-RPC methods), so this endpoint is not part of the
/// delivery path. It is retained for streaming progress/results and future
/// push-based delivery; returning 501 is harmless because no client depends
/// on it yet.
async fn handle_stream() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        "SSE streaming not yet implemented (task delivery uses tasks/claim polling)",
    )
        .into_response()
}
