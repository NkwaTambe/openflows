// crates/agent-nexus/src/a2a/http_server.rs
//! JSON-RPC HTTP server for A2A relay. Implements the standard A2A methods:
//! - message/send (submit verify request)
//! - message/stream (subscribe to task events via SSE)
//! - tasks/get (retrieve task details)
//! - tasks/cancel (cancel a running task)
//! - tasks/resubscribe (resume after disconnect)

use super::routing::A2ARelay;
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
use tracing::debug;

/// JSON-RPC 2.0 request envelope (simplified for A2A).
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
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
async fn handle_message_send(
    _state: &A2AServerState,
    _params: &Value,
) -> anyhow::Result<Value> {
    // TODO: Parse params as verify request
    // TODO: Extract pair_id and requester identity from request context
    // TODO: Call submit_verify_request
    // TODO: Return task_id

    debug!("message/send: not yet implemented");
    Ok(json!({"task_id": "placeholder"}))
}

/// tasks/get: Retrieve task details (pull-based polling)
async fn handle_tasks_get(
    _state: &A2AServerState,
    _params: &Value,
) -> anyhow::Result<Value> {
    // TODO: Extract task_id from params
    // TODO: Return task state (pending, completed, etc.) + result if terminal

    debug!("tasks/get: not yet implemented");
    Ok(json!({"status": "pending"}))
}

/// tasks/cancel: Cancel a running task
async fn handle_tasks_cancel(
    _state: &A2AServerState,
    _params: &Value,
) -> anyhow::Result<Value> {
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

/// Server-Sent Events handler. Establishes persistent connection for streaming
/// task progress and results.
async fn handle_stream() -> impl IntoResponse {
    // TODO: Implement SSE subscription
    // TODO: Extract pair_id and role from headers/auth
    // TODO: Register session in relay
    // TODO: Stream task assignments, progress, results
    // TODO: Deregister on disconnect

    (StatusCode::NOT_IMPLEMENTED, "SSE not yet implemented").into_response()
}
