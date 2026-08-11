// crates/openflows-harness/src/a2a_client.rs
//! A2A HTTP client for harness workers to communicate with nexus relay.
//!
//! Used by both Sentinel (verify request) and Forge (verify serve) roles.
//! The client is a thin wrapper around reqwest that handles JSON-RPC
//! envelope format expected by the A2A relay.

use a2a_protocol::{VerifyRequest, VerifyResult};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::{debug, info, warn};

/// A2A relay client for communicating with the nexus-hosted relay.
#[allow(dead_code)]
pub struct A2AClient {
    http_client: reqwest::Client,
    relay_url: String,
    pair_id: String,
    role: String, // "sentinel" or "forge"
}

impl A2AClient {
    /// Create a new A2A client pointing at the nexus relay.
    ///
    /// The relay address is read from `A2A_RELAY_ADDR` env var, which the
    /// workspace template must inject with the nexus relay's network address
    /// (in the Coder docker deployment: `openflows-nexus:3000`). The
    /// loopback fallback is only a local-testing convenience — a provisioned
    /// workspace that omits `A2A_RELAY_ADDR` would otherwise silently target
    /// its own interface (issue #143 / PR review), so it warns here.
    pub fn new(pair_id: String, role: String) -> Result<Self> {
        let relay_addr = match std::env::var("A2A_RELAY_ADDR") {
            Ok(addr) if !addr.trim().is_empty() => addr,
            _ => {
                warn!(
                    "A2A_RELAY_ADDR is not set; defaulting to 127.0.0.1:3000. \
                     Provisioned workspaces must set A2A_RELAY_ADDR to the nexus relay \
                     (e.g. openflows-nexus:3000) — loopback will not reach the relay."
                );
                "127.0.0.1:3000".to_string()
            }
        };
        let relay_url = format!("http://{}", relay_addr);

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http_client,
            relay_url,
            pair_id,
            role,
        })
    }

    /// Check if the relay is healthy.
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/health", self.relay_url);
        let response = self.http_client.get(&url).send().await?;
        if response.status().is_success() {
            info!("A2A relay health check passed");
            Ok(())
        } else {
            Err(anyhow!("A2A relay unhealthy: {}", response.status()))
        }
    }

    /// Submit a verify request (Sentinel-side, task 3).
    /// Sends message/send RPC to relay, gets back task_id.
    pub async fn submit_verify_request(&self, req: &VerifyRequest) -> Result<String> {
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "method": "message/send",
            "params": req,
            "id": uuid::Uuid::new_v4().to_string(),
        });

        let url = format!("{}/rpc", self.relay_url);
        let response = self
            .http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to send verify request to relay")?;

        let body: Value = response.json().await?;

        // Parse JSON-RPC response
        if let Some(error) = body.get("error").and_then(|e| e.as_object()) {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("A2A RPC error: {}", msg));
        }

        let task_id = body
            .get("result")
            .and_then(|r| r.get("task_id"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("No task_id in response"))?
            .to_string();

        debug!(task_id = %task_id, "Verify request submitted");
        Ok(task_id)
    }

    /// Get a task's current status (Sentinel polling after submit).
    pub async fn get_task_status(&self, task_id: &str) -> Result<Option<VerifyResult>> {
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "method": "tasks/get",
            "params": {
                "task_id": task_id,
            },
            "id": uuid::Uuid::new_v4().to_string(),
        });

        let url = format!("{}/rpc", self.relay_url);
        let response = self
            .http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to get task status from relay")?;

        let body: Value = response.json().await?;

        if let Some(error) = body.get("error").and_then(|e| e.as_object()) {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("A2A RPC error: {}", msg));
        }

        let result = body.get("result");
        let status = result
            .and_then(|r| r.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");

        if status == "completed" {
            let verify_result: VerifyResult = result
                .and_then(|r| r.get("result").cloned())
                .map(serde_json::from_value)
                .transpose()?
                .context("completed task has no serializable result")?;
            return Ok(Some(verify_result));
        }

        Ok(None)
    }

    /// Claim the next pending task for this pair (Forge executor role).
    /// Returns the claimed `VerifyRequest` (with its task_id via a wrapper)
    /// or `None` when no task is pending for the pair.
    pub async fn claim_next_task(&self) -> Result<Option<(String, VerifyRequest)>> {
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "method": "tasks/claim",
            "params": {
                "pair_id": self.pair_id,
                "role": self.role,
            },
            "id": uuid::Uuid::new_v4().to_string(),
        });

        let url = format!("{}/rpc", self.relay_url);
        let response = self
            .http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to claim task from relay")?;

        let body: Value = response.json().await?;

        if let Some(error) = body.get("error").and_then(|e| e.as_object()) {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("A2A RPC error: {}", msg));
        }

        let task = body.get("result").cloned().unwrap_or(Value::Null);
        if task.is_null() {
            return Ok(None);
        }

        let task_id = task
            .get("task_id")
            .and_then(|t| t.as_str())
            .context("claimed task missing task_id")?
            .to_string();
        let request: VerifyRequest = serde_json::from_value(
            task.get("request")
                .cloned()
                .context("claimed task missing request")?,
        )?;

        debug!(task_id = %task_id, "Task claimed");
        Ok(Some((task_id, request)))
    }

    /// Submit a terminal result for a claimed task (Forge executor role).
    pub async fn complete_task(&self, result: &VerifyResult) -> Result<()> {
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "method": "tasks/complete",
            "params": {
                "task_id": result.task_id,
                "result": result,
            },
            "id": uuid::Uuid::new_v4().to_string(),
        });

        let url = format!("{}/rpc", self.relay_url);
        let response = self
            .http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to complete task on relay")?;

        let body: Value = response.json().await?;

        if let Some(error) = body.get("error").and_then(|e| e.as_object()) {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("A2A RPC error: {}", msg));
        }

        debug!(task_id = %result.task_id, "Task completion submitted");
        Ok(())
    }

    /// Cancel a running task (Sentinel-side).
    pub async fn cancel_task(&self, task_id: &str) -> Result<()> {
        let rpc_request = json!({
            "jsonrpc": "2.0",
            "method": "tasks/cancel",
            "params": {
                "task_id": task_id,
            },
            "id": uuid::Uuid::new_v4().to_string(),
        });

        let url = format!("{}/rpc", self.relay_url);
        let _response = self
            .http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to cancel task")?;

        debug!(task_id = %task_id, "Task cancellation sent");
        Ok(())
    }
}
