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
use tracing::{debug, info};

/// A2A relay client for communicating with the nexus-hosted relay.
pub struct A2AClient {
    http_client: reqwest::Client,
    relay_url: String,
    pair_id: String,
    role: String, // "sentinel" or "forge"
}

impl A2AClient {
    /// Create a new A2A client pointing at the nexus relay.
    /// 
    /// The relay address is read from A2A_RELAY_ADDR env var 
    /// (default: 127.0.0.1:3000).
    pub fn new(pair_id: String, role: String) -> Result<Self> {
        let relay_addr = std::env::var("A2A_RELAY_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string());
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
            Err(anyhow!(
                "A2A relay unhealthy: {}",
                response.status()
            ))
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
        let response = self.http_client
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
        let response = self.http_client
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

        // TODO: Parse result into VerifyResult when available
        // For now, return None (result not yet available)
        Ok(None)
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
        let _response = self.http_client
            .post(&url)
            .json(&rpc_request)
            .send()
            .await
            .context("Failed to cancel task")?;

        debug!(task_id = %task_id, "Task cancellation sent");
        Ok(())
    }
}
