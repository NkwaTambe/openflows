// crates/agent-nexus/src/a2a/verify_handler.rs
//! Handler for `verify` A2A task type. Validates requests, routes to
//! executor, manages result dedup and persistence.

use super::routing::A2ARelay;
use a2a_protocol::VerifyRequest;
use anyhow::Result;
use serde_json::json;
use tracing::warn;

/// Validate and submit a verify request. Returns task_id on success.
/// On validation failure, logs to `audit:a2a:rejected` and returns error.
pub async fn submit_verify_request(
    relay: &A2ARelay,
    req: &VerifyRequest,
    requester_pair_id: &str,
) -> Result<String> {
    // Validate request
    if let Err(e) = relay.validate_verify_request(req, requester_pair_id) {
        // Log rejection to audit trail
        log_rejected_request(relay, req, &e.to_string()).await;
        return Err(e);
    }

    // Idempotency: check if task already exists
    let task_id = relay.check_or_create_task(req).await?;

    // TODO (task 2.4): Route to executor (Forge) for the pair
    // For now, just return the task_id; the executor will pull it via
    // `tasks/get` in the JSON-RPC handler

    Ok(task_id)
}

/// Log a rejected request to the audit trail in Redis.
async fn log_rejected_request(_relay: &A2ARelay, req: &VerifyRequest, reason: &str) {
    let _entry = json!({
        "pair_id": &req.pair_id,
        "argv": &req.argv,
        "reason": reason,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // Append to audit:a2a:rejected (append-only log)
    // TODO: use Redis LPUSH/RPUSH for list, or JSON array in a single key
    warn!(
        pair_id = %req.pair_id,
        argv = ?req.argv,
        reason,
        "Verify request rejected"
    );
}
