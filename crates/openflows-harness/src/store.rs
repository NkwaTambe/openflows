//! Harness SharedStore — typed, validated Redis I/O with tenant namespacing.
//!
//! All keys are prefixed with `ns:{tenant}:` for tenant isolation.
//! All writes are validated against serde schemas from `config::state`.

use anyhow::{bail, Context, Result};
use config::state::{full_ticket_key, full_ticket_key_flat, heartbeat_key, HeartbeatRecord};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

/// Valid phases for the `status set` command.
const VALID_PHASES: &[&str] = &["planning", "building", "testing", "review_ready", "blocked"];

/// Phases that require SENTINEL approval before transitioning FROM them.
/// FORGE cannot move past these phases until SENTINEL writes a gate approval.
const GATED_PHASES: &[&str] = &["planning"];

/// Gate approval payload written by SENTINEL to allow FORGE to proceed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateApproval {
    pub phase: String,
    pub approved_by: String,
    pub ts: u64,
    pub notes: Option<String>,
}

/// Valid verdicts for the `review submit` command.
const VALID_VERDICTS: &[&str] = &["approve", "reject"];

/// Dispatch payload written by the Controller for a worker to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchPayload {
    pub ticket_id: String,
    pub title: String,
    pub body: String,
    pub branch: Option<String>,
    pub contract_path: Option<String>,
}

/// PR info written by the harness when forge opens a PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrInfo {
    pub pr_number: u64,
    pub branch: String,
    pub title: String,
}

/// Handoff payload written by forge for sentinel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffPayload {
    pub contract_md: String,
    pub notes: Option<String>,
}

/// Review payload written by sentinel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPayload {
    pub verdict: String,
    pub report: String,
    pub pr_number: Option<u64>,
}

/// Merge payload written by vessel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePayload {
    pub pr_number: u64,
    pub sha: String,
    pub merged: bool,
}

pub struct HarnessStore {
    client: fred::clients::Client,
    tenant: String,
}

/// Authorize an approver role for a gated phase transition. Only SENTINEL may
/// approve a gate; FORGE/other roles must be rejected so an agent cannot approve
/// its own plan and sidestep the mandatory review checkpoint. Comparison is
/// case-insensitive because the worker CLI derives the role from
/// `OPENFLOWS_ROLE` and operators may spell the role with varying case.
pub fn authorize_gate_approver(role: &str) -> Result<()> {
    if !role.eq_ignore_ascii_case("sentinel") {
        bail!(
            "Gate approval rejected: approver role '{}' is not SENTINEL. \
             Only SENTINEL may approve a gated phase transition; FORGE/other \
             roles are not authorized to approve their own plan.",
            role
        );
    }
    Ok(())
}

impl HarnessStore {
    pub async fn new(redis_url: &str, tenant: &str) -> Result<Self> {
        let config = Config::from_url(redis_url)?;
        let client = Builder::from_config(config).build()?;
        client.init().await.context("Failed to connect to Redis")?;
        Ok(Self {
            client,
            tenant: tenant.to_string(),
        })
    }

    /// Build a tenant-namespaced key.
    fn key(&self, k: &str) -> String {
        format!("ns:{}:{}", self.tenant, k)
    }

    /// Read the dispatch payload for this ticket+role.
    pub async fn dispatch_read(&self, ticket: &str, role: &str) -> Result<()> {
        let key = self.key(&full_ticket_key(ticket, "dispatch", role));
        let val: Option<String> = self.client.get(&key).await.context("Redis GET failed")?;
        match val {
            Some(json_str) => {
                let payload: DispatchPayload =
                    serde_json::from_str(&json_str).context("Failed to parse dispatch payload")?;
                let output = serde_json::to_string_pretty(&payload)?;
                println!("{}", output);
                debug!(key = %key, "dispatch read");
            }
            None => {
                bail!(
                    "No dispatch found for ticket {} role {}. \
                     The Controller may not have assigned work yet.",
                    ticket,
                    role
                );
            }
        }
        Ok(())
    }

    /// Set the current phase for this ticket.
    ///
    /// Enforces gated phase transitions: FORGE cannot move past `planning`
    /// until SENTINEL approves via `gate approve`.
    pub async fn status_set(&self, ticket: &str, role: &str, phase: &str) -> Result<()> {
        if !VALID_PHASES.contains(&phase) {
            bail!(
                "Invalid phase '{}'. Valid phases: {}",
                phase,
                VALID_PHASES.join(", ")
            );
        }

        // Check current phase and enforce gating
        let status_key = self.key(&full_ticket_key_flat(ticket, "status"));
        let current_status: Option<String> = self
            .client
            .get(&status_key)
            .await
            .context("Redis GET failed")?;

        if let Some(ref status_json) = current_status {
            if let Ok(status) = serde_json::from_str::<serde_json::Value>(status_json) {
                if let Some(current_phase) = status.get("phase").and_then(|p| p.as_str()) {
                    // If transitioning FROM a gated phase, check for approval
                    if GATED_PHASES.contains(&current_phase) && current_phase != phase {
                        let gate_key =
                            self.key(&format!("ticket:{}:gate:{}", ticket, current_phase));
                        let gate_approval: Option<String> = self
                            .client
                            .get(&gate_key)
                            .await
                            .context("Redis GET failed")?;

                        if gate_approval.is_none() {
                            bail!(
                                "Cannot transition from '{}' to '{}' without SENTINEL approval.\n\
                                 SENTINEL must run: openflows-harness gate approve --phase {}\n\
                                 This ensures your plan is reviewed before implementation begins.",
                                current_phase,
                                phase,
                                current_phase
                            );
                        }
                        info!(
                            ticket,
                            from = current_phase,
                            to = phase,
                            "Gate approval verified, allowing transition"
                        );
                    }
                }
            }
        }

        let val = serde_json::json!({
            "phase": phase,
            "role": role,
            "ts": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        });
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&status_key, val.to_string(), None, None, false)
            .await;
        println!("Wrote: {}", status_key);
        info!(key = %status_key, phase, "status set");
        Ok(())
    }

    /// Approve a gated phase transition (SENTINEL only).
    ///
    /// This allows FORGE to proceed past a gated phase (e.g., `planning` → `building`).
    ///
    /// Only the SENTINEL role may approve a gate. FORGE cannot approve its own
    /// plan — if it could, the mandatory review checkpoint would be bypassable by
    /// the very agent it is meant to supervise. The worker CLI (`openflows-harness
    /// gate approve`) derives `role` from `OPENFLOWS_ROLE`, so only a SENTINEL
    /// workspace can satisfy this; the admin `openflows gate approve` CLI passes
    /// `--approver` (defaulting to SENTINEL). Any other role is rejected.
    pub async fn gate_approve(
        &self,
        ticket: &str,
        role: &str,
        phase: &str,
        notes: Option<&str>,
    ) -> Result<()> {
        authorize_gate_approver(role)?;
        if !GATED_PHASES.contains(&phase) {
            bail!(
                "Phase '{}' is not a gated phase. Gated phases: {}",
                phase,
                GATED_PHASES.join(", ")
            );
        }

        // Verify current phase matches
        let status_key = self.key(&full_ticket_key_flat(ticket, "status"));
        let current_status: Option<String> = self
            .client
            .get(&status_key)
            .await
            .context("Redis GET failed")?;

        if let Some(ref status_json) = current_status {
            if let Ok(status) = serde_json::from_str::<serde_json::Value>(status_json) {
                if let Some(current_phase) = status.get("phase").and_then(|p| p.as_str()) {
                    if current_phase != phase {
                        bail!(
                            "Cannot approve '{}' gate — current phase is '{}'. \
                             FORGE must be in the '{}' phase to receive approval.",
                            phase,
                            current_phase,
                            phase
                        );
                    }
                }
            }
        } else {
            bail!(
                "No status found for ticket {}. FORGE must set status to '{}' first.",
                ticket,
                phase
            );
        }

        let approval = GateApproval {
            phase: phase.to_string(),
            approved_by: role.to_string(),
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            notes: notes.map(|s| s.to_string()),
        };

        let gate_key = self.key(&format!("ticket:{}:gate:{}", ticket, phase));
        let json = serde_json::to_string(&approval)?;
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&gate_key, json, None, None, false)
            .await;

        println!("Gate approved: {} phase '{}' by {}", ticket, phase, role);
        info!(key = %gate_key, phase, role, "gate approved");
        Ok(())
    }

    /// Check if a gated phase has been approved.
    pub async fn gate_status(&self, ticket: &str, phase: &str) -> Result<()> {
        let gate_key = self.key(&format!("ticket:{}:gate:{}", ticket, phase));
        let approval: Option<String> = self
            .client
            .get(&gate_key)
            .await
            .context("Redis GET failed")?;

        match approval {
            Some(json) => {
                let approval: GateApproval =
                    serde_json::from_str(&json).context("Failed to parse gate approval")?;
                println!(
                    "✓ Gate '{}' approved by {} at {}",
                    approval.phase, approval.approved_by, approval.ts
                );
                if let Some(notes) = approval.notes {
                    println!("  Notes: {}", notes);
                }
            }
            None => {
                println!("✗ Gate '{}' not yet approved", phase);
            }
        }
        Ok(())
    }

    /// Read the current status JSON for this ticket. Prints `{}` when unset
    /// so hook scripts can always parse the output.
    pub async fn status_get(&self, ticket: &str) -> Result<()> {
        let key = self.key(&full_ticket_key_flat(ticket, "status"));
        let val: Option<String> = self.client.get(&key).await.context("Redis GET failed")?;
        println!("{}", val.unwrap_or_else(|| "{}".to_string()));
        debug!(key = %key, "status read");
        Ok(())
    }

    /// Read the recorded PR info for this ticket. Prints `{}` when unset.
    pub async fn pr_get(&self, ticket: &str) -> Result<()> {
        let key = self.key(&full_ticket_key_flat(ticket, "pr"));
        let val: Option<String> = self.client.get(&key).await.context("Redis GET failed")?;
        println!("{}", val.unwrap_or_else(|| "{}".to_string()));
        debug!(key = %key, "pr read");
        Ok(())
    }

    /// Write a handoff contract (forge → sentinel).
    pub async fn handoff_write(
        &self,
        ticket: &str,
        contract_path: &Path,
        notes: Option<&str>,
    ) -> Result<()> {
        let contract_md = std::fs::read_to_string(contract_path).context(format!(
            "Failed to read contract file: {}",
            contract_path.display()
        ))?;
        let payload = HandoffPayload {
            contract_md,
            notes: notes.map(|s| s.to_string()),
        };
        let key = self.key(&full_ticket_key_flat(ticket, "handoff"));
        let json = serde_json::to_string(&payload)?;
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&key, json, None, None, false)
            .await;
        println!("Wrote: {}", key);
        info!(key = %key, "handoff written");
        Ok(())
    }

    /// Record that a PR was opened.
    pub async fn pr_opened(&self, ticket: &str, pr: &u64, branch: &str, title: &str) -> Result<()> {
        let payload = PrInfo {
            pr_number: *pr,
            branch: branch.to_string(),
            title: title.to_string(),
        };
        let key = self.key(&full_ticket_key_flat(ticket, "pr"));
        let json = serde_json::to_string(&payload)?;
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&key, json, None, None, false)
            .await;
        println!("Wrote: {} (pr #{})", key, pr);
        info!(key = %key, pr, "pr opened");
        Ok(())
    }

    /// Submit a review verdict (sentinel).
    pub async fn review_submit(
        &self,
        ticket: &str,
        role: &str,
        verdict: &str,
        report_path: &Path,
        pr: Option<u64>,
    ) -> Result<()> {
        if !VALID_VERDICTS.contains(&verdict) {
            bail!(
                "Invalid verdict '{}'. Valid verdicts: {}",
                verdict,
                VALID_VERDICTS.join(", ")
            );
        }
        let report = std::fs::read_to_string(report_path).context(format!(
            "Failed to read report file: {}",
            report_path.display()
        ))?;
        let payload = ReviewPayload {
            verdict: verdict.to_string(),
            report,
            pr_number: pr,
        };
        let key = self.key(&full_ticket_key(ticket, "review", role));
        let json = serde_json::to_string(&payload)?;
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&key, json, None, None, false)
            .await;
        println!("Wrote: {} (verdict: {})", key, verdict);
        info!(key = %key, verdict, "review submitted");
        Ok(())
    }

    /// Record that a merge completed (vessel).
    pub async fn merge_done(&self, ticket: &str, pr: &u64, sha: &str) -> Result<()> {
        let payload = MergePayload {
            pr_number: *pr,
            sha: sha.to_string(),
            merged: true,
        };
        let key = self.key(&full_ticket_key_flat(ticket, "deployment"));
        let json = serde_json::to_string(&payload)?;
        let _: Result<(), _> = self
            .client
            .set::<(), _, _>(&key, json, None, None, false)
            .await;
        println!("Wrote: {} (pr #{}, merged)", key, pr);
        info!(key = %key, pr, "merge done");
        Ok(())
    }

    /// Start daemonized heartbeat writing (every 30s).
    pub async fn heartbeat_start(&self, ticket: &str, role: &str) -> Result<()> {
        let key = self.key(&heartbeat_key(role, ticket));
        info!(key = %key, "Starting heartbeat writer (30s interval)");

        loop {
            let record = HeartbeatRecord {
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                ws_id: std::env::var("CODER_WORKSPACE_ID").unwrap_or_default(),
                status: "running".to_string(),
            };
            let json = serde_json::to_string(&record)?;
            let _: Result<(), _> = self
                .client
                .set::<(), _, _>(
                    &key,
                    &json,
                    Some(fred::types::Expiration::EX(120)),
                    None,
                    false,
                )
                .await;
            debug!(key = %key, "heartbeat written");
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    }

    /// Stop heartbeat writing (delete the key).
    pub async fn heartbeat_stop(&self, ticket: &str, role: &str) -> Result<()> {
        let key = self.key(&heartbeat_key(role, ticket));
        let _: Result<i64, _> = self.client.del(&key).await;
        println!("Deleted: {}", key);
        info!(key = %key, "heartbeat stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_phases() {
        assert!(VALID_PHASES.contains(&"planning"));
        assert!(VALID_PHASES.contains(&"building"));
        assert!(!VALID_PHASES.contains(&"invalid_phase"));
    }

    #[test]
    fn test_valid_verdicts() {
        assert!(VALID_VERDICTS.contains(&"approve"));
        assert!(VALID_VERDICTS.contains(&"reject"));
        assert!(!VALID_VERDICTS.contains(&"maybe"));
    }

    #[test]
    fn test_dispatch_payload_serde() {
        let payload = DispatchPayload {
            ticket_id: "T-42".to_string(),
            title: "Fix bug".to_string(),
            body: "The bug is in auth.rs".to_string(),
            branch: Some("forge-t-42".to_string()),
            contract_path: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let decoded: DispatchPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.ticket_id, "T-42");
        assert_eq!(decoded.title, "Fix bug");
    }

    #[test]
    fn test_key_namespacing() {
        let tenant = "acme";
        let ticket = "T-42";
        let key = format!(
            "ns:{}:{}",
            tenant,
            full_ticket_key(ticket, "dispatch", "forge")
        );
        assert_eq!(key, "ns:acme:ticket:T-42:dispatch:forge");
    }

    #[test]
    fn test_gate_approver_accepts_sentinel_case_insensitive() {
        assert!(authorize_gate_approver("sentinel").is_ok());
        assert!(authorize_gate_approver("SENTINEL").is_ok());
        assert!(authorize_gate_approver("Sentinel").is_ok());
    }

    #[test]
    fn test_gate_approver_rejects_forge_and_others() {
        // FORGE must never approve its own plan — the review checkpoint exists
        // to supervise it. Vessel/Lore/empty/unknown roles are also rejected.
        assert!(authorize_gate_approver("forge").is_err());
        assert!(authorize_gate_approver("vessel").is_err());
        assert!(authorize_gate_approver("lore").is_err());
        assert!(authorize_gate_approver("").is_err());
        assert!(authorize_gate_approver("admin").is_err());
    }
}
