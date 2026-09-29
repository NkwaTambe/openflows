//! HTTP client for the OpenFlows Manager `/api/v1` control surface.
//!
//! Thin typed wrapper over `reqwest`. Every request carries the bearer token
//! and surfaces a clear error on non-success responses. Response shapes mirror
//! the manager's handlers.

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

/// A parsed error body returned by the manager.
#[derive(Debug, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl Client {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let mut req = self
            .http
            .request(method, self.url(path))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("failed to reach manager at {}", self.base_url))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            // Try to surface the manager's structured message.
            let detail = if let Ok(parsed) = serde_json::from_str::<ApiErrorBody>(&text) {
                parsed
                    .message
                    .or(parsed.error)
                    .unwrap_or_else(|| text.clone())
            } else {
                text
            };
            return Err(anyhow!("manager returned {status}: {detail}"));
        }
        resp.json::<T>()
            .await
            .context("failed to decode manager response")
    }

    // ── Tenants ─────────────────────────────────────────────────────────

    pub async fn list_tenants(&self) -> Result<ListTenantsResponse> {
        self.send_json(
            reqwest::Method::GET,
            "/api/v1/tenants",
            None::<&serde_json::Value>,
        )
        .await
    }

    pub async fn add_tenant(
        &self,
        repo: &str,
        name: Option<&str>,
        fleet: Option<u32>,
    ) -> Result<AddTenantResponse> {
        let body = AddTenantRequest {
            repo: repo.to_string(),
            name: name.map(String::from),
            fleet,
        };
        self.send_json(reqwest::Method::POST, "/api/v1/tenants", Some(&body))
            .await
    }

    // ── Control ─────────────────────────────────────────────────────────

    pub async fn get_control(&self, tenant: &str) -> Result<ControlResponse> {
        self.send_json(
            reqwest::Method::GET,
            &format!("/api/v1/tenants/{tenant}/control"),
            None::<&serde_json::Value>,
        )
        .await
    }

    pub async fn set_control(&self, tenant: &str, mode: &str) -> Result<ControlResponse> {
        let body = SetControlRequest {
            mode: mode.to_string(),
        };
        self.send_json(
            reqwest::Method::PUT,
            &format!("/api/v1/tenants/{tenant}/control"),
            Some(&body),
        )
        .await
    }

    // ── Tasks ───────────────────────────────────────────────────────────

    pub async fn assign_task(
        &self,
        tenant: &str,
        req: AssignTaskRequest,
    ) -> Result<AssignTaskResponse> {
        self.send_json(
            reqwest::Method::POST,
            &format!("/api/v1/tenants/{tenant}/tasks"),
            Some(&req),
        )
        .await
    }
}

// ── Wire types (mirror the manager) ────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ListTenantsResponse {
    pub tenants: Vec<TenantSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TenantSummary {
    pub name: String,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub control_mode: String,
}

#[derive(Debug, Serialize)]
pub struct AddTenantRequest {
    pub repo: String,
    pub name: Option<String>,
    pub fleet: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddTenantResponse {
    pub name: String,
    pub repo: String,
    pub fleet: Option<u32>,
    pub control_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub tenant: String,
    pub control_mode: String,
    #[serde(default)]
    pub previous_control_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetControlRequest {
    pub mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssignTaskRequest {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssignTaskResponse {
    pub tenant: String,
    pub task: Task,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: u64,
}
