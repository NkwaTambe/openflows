//! Command vocabulary and execution for the OpenFlows remote CLI.
//!
//! The subcommand surface mirrors the local `openflows` CLI where sensible
//! (`tenant`, and a new `control` group), but every command talks to a deployed
//! manager over HTTP instead of reading Redis/Coder directly.

use crate::client::{AssignTaskRequest, Client};
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Store the manager URL + bearer token locally.
    Login {
        /// Base URL of the deployed OpenFlows manager.
        #[arg(long)]
        url: String,
        /// Bearer token (defaults to the OPENFLOWS_MANAGER_TOKEN env var).
        #[arg(long)]
        token: Option<String>,
    },
    /// Manage tenants on the deployed instance.
    Tenant {
        #[command(subcommand)]
        action: TenantAction,
    },
    /// Assign work directly to a tenant.
    Tasks {
        #[command(subcommand)]
        action: TasksAction,
    },
    /// Steer a tenant's fleet control state.
    Control {
        #[command(subcommand)]
        action: ControlAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum TenantAction {
    /// List tenants on the deployed instance.
    List,
    /// Add a tenant bound to a GitHub repository.
    Add {
        /// Repository in `owner/repo` format.
        repo: String,
        /// Tenant name (defaults to the repo owner).
        #[arg(long)]
        name: Option<String>,
        /// Fleet size: number of FORGE-SENTINEL pairs (>= 1).
        #[arg(long)]
        fleet: Option<u32>,
    },
}

#[derive(Debug, Subcommand)]
pub enum TasksAction {
    /// Assign a task to a tenant directly.
    ///
    /// Supply the task as `--title`/`--body` flags, as an inline `--json-input`,
    /// or from a JSON file with `--file`.
    Assign {
        /// Tenant to assign the task to.
        tenant: String,
        /// Task title (used with --body).
        #[arg(long)]
        title: Option<String>,
        /// Task detail / acceptance text.
        #[arg(long)]
        body: Option<String>,
        /// Path to a JSON file containing the task request.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Inline JSON containing the task request.
        #[arg(long)]
        json_input: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ControlAction {
    /// Read a tenant's current control mode.
    Get { tenant: String },
    /// Set a tenant's control mode (auto|paused).
    Set { tenant: String, mode: String },
    /// Halt a tenant's fleet (control mode `paused`).
    Pause { tenant: String },
    /// Resume a tenant's fleet (control mode `auto`).
    Resume { tenant: String },
}

/// Dispatch a resolved command against the client.
pub async fn dispatch(cmd: Command, client: &Client, json: bool) -> Result<()> {
    match cmd {
        Command::Login { .. } => unreachable!("login is handled before client construction"),
        Command::Tenant { action } => tenant(action, client, json).await,
        Command::Tasks { action } => tasks(action, client, json).await,
        Command::Control { action } => control(action, client, json).await,
    }
}

/// Emit output either as pretty JSON or a human string.
fn emit(json: bool, value: Value, human: impl FnOnce() -> String) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    } else {
        println!("{}", human());
    }
}

async fn tenant(action: TenantAction, client: &Client, json: bool) -> Result<()> {
    match action {
        TenantAction::List => {
            let resp = client.list_tenants().await?;
            let value = json!(resp);
            emit(json, value, || {
                if resp.tenants.is_empty() {
                    "  (no tenants found)".to_string()
                } else {
                    let mut out = String::from("Tenants:");
                    for t in &resp.tenants {
                        let repo = t.repository.as_deref().unwrap_or("-");
                        out.push_str(&format!(
                            "\n  - {} (repo: {}, control: {})",
                            t.name, repo, t.control_mode
                        ));
                    }
                    out
                }
            });
        }
        TenantAction::Add { repo, name, fleet } => {
            let resp = client.add_tenant(&repo, name.as_deref(), fleet).await?;
            let value = json!(resp);
            emit(json, value, || {
                format!(
                    "✓ Tenant '{}' added (repo: {}, control: {})",
                    resp.name, resp.repo, resp.control_mode
                )
            });
        }
    }
    Ok(())
}

async fn tasks(action: TasksAction, client: &Client, json: bool) -> Result<()> {
    let TasksAction::Assign {
        tenant,
        title,
        body,
        file,
        json_input,
    } = action;

    // Build the request body from one of the accepted input forms.
    let (mut resolved_title, mut resolved_body, resolved_payload): (String, String, Value) =
        if let Some(path) = file {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read task file {}", path.display()))?;
            let parsed: Value = serde_json::from_str(&raw)
                .with_context(|| format!("task file {} is not valid JSON", path.display()))?;
            task_fields(parsed)?
        } else if let Some(inline) = json_input {
            let parsed: Value =
                serde_json::from_str(&inline).context("--json-input is not valid JSON")?;
            task_fields(parsed)?
        } else {
            let title = title
                .clone()
                .context("missing task input: provide --title, --json-input, or --file")?;
            (title, body.clone().unwrap_or_default(), Value::Null)
        };

    // Flags can override fields that came from JSON input.
    if let Some(t) = title {
        resolved_title = t;
    }
    if let Some(b) = body {
        resolved_body = b;
    }

    let req = AssignTaskRequest {
        title: resolved_title,
        body: resolved_body,
        source: "cli".to_string(),
        payload: resolved_payload,
    };
    let resp = client.assign_task(&tenant, req).await?;
    let value = json!(resp);
    emit(json, value, || {
        format!(
            "✓ Assigned task '{}' (id: {}) to tenant '{}'",
            resp.task.title, resp.task.id, resp.tenant
        )
    });
    Ok(())
}

/// Split a task JSON object into `(title, body, remaining-payload)`.
fn task_fields(parsed: Value) -> Result<(String, String, Value)> {
    let obj = parsed.as_object().context("task JSON must be an object")?;
    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .context("task JSON must include a 'title' string")?
        .to_string();
    let body = obj
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Prefer an explicit top-level `payload`; otherwise keep any other fields
    // as the payload.
    let payload = if let Some(p) = obj.get("payload") {
        p.clone()
    } else {
        let mut rest = parsed.clone();
        if let Some(map) = rest.as_object_mut() {
            map.remove("title");
            map.remove("body");
        }
        if !rest.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            rest
        } else {
            Value::Null
        }
    };
    Ok((title, body, payload))
}

async fn control(action: ControlAction, client: &Client, json: bool) -> Result<()> {
    let (tenant, mode, verb) = match action {
        ControlAction::Get { tenant } => {
            let resp = client.get_control(&tenant).await?;
            let value = json!(resp);
            emit(json, value, || {
                format!(
                    "Tenant '{}' control mode: {}",
                    resp.tenant, resp.control_mode
                )
            });
            return Ok(());
        }
        ControlAction::Set { tenant, mode } => {
            validate_mode(&mode)?;
            (tenant, mode, "set")
        }
        ControlAction::Pause { tenant } => (tenant, "paused".to_string(), "paused"),
        ControlAction::Resume { tenant } => (tenant, "auto".to_string(), "resumed"),
    };
    let resp = client.set_control(&tenant, &mode).await?;
    let value = json!(resp);
    emit(json, value, || {
        format!(
            "✓ Tenant '{}' {} (control mode: {})",
            resp.tenant, verb, resp.control_mode
        )
    });
    Ok(())
}

fn validate_mode(mode: &str) -> Result<()> {
    // Only fully-implemented modes are accepted; drained/targeted are reserved
    // for future steering and rejected here for a clear, local error.
    let valid = ["auto", "paused"];
    if !valid.contains(&mode.to_lowercase().as_str()) {
        bail!(
            "invalid mode '{}' — supported modes: {} (drained/targeted are not yet implemented)",
            mode,
            valid.join(", ")
        );
    }
    Ok(())
}
