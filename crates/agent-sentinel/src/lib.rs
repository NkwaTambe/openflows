//! agent-sentinel — SENTINEL adversarial review node (Coder-only redesign).
//!
//! Thin flow node that reads harness-written review keys from SharedStore
//! and routes based on the sentinel's verdict (approve → vessel, reject → forge).
//! The actual review intelligence lives in the Coder Agent (control plane).

use anyhow::Result;
use async_trait::async_trait;
use coder_client::{ChatStatus, CoderClient};
use config::state::{
    full_ticket_key, full_ticket_key_flat, review_action_key, review_chat_key, KEY_PENDING_PRS,
    KEY_TICKETS, KEY_TICKET_CHAT, KEY_TICKET_CHAT_ACTION, KEY_TICKET_STATUS, KEY_WORKER_SLOTS,
    REVIEW_TYPE_PLANNING_GATE, REVIEW_TYPE_PR,
};
use config::{Envconfig, Ticket, TicketStatus, WorkerSlot, WorkerStatus};
use pocketflow_core::{node::PAUSE_SIGNAL, Action, Node, SharedStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, info, warn};

const ACTION_REVIEW_APPROVE: &str = "review_approve";
const ACTION_REVIEW_REJECT: &str = "review_reject";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPayload {
    pub verdict: String,
    pub report: String,
    pub pr_number: Option<u64>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub round: u64,
    #[serde(default)]
    pub head: String,
}

pub struct SentinelNode {
    #[allow(dead_code)]
    registry_path: std::path::PathBuf,
}

impl SentinelNode {
    pub fn new(registry_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            registry_path: registry_path.into(),
        }
    }

    async fn coder_client_from_store(store: &SharedStore) -> Option<CoderClient> {
        let coder = config::CoderConfig::init_from_env().ok();
        let coder_url: Option<String> = store
            .get_typed("coder_url")
            .await
            .or_else(|| coder.as_ref().map(|c| c.url.clone()));
        let coder_token: Option<String> = coder
            .and_then(|c| c.session_token)
            .or_else(|| std::env::var("CODER_API_TOKEN").ok());
        let coder_token = if coder_token.as_deref().is_some_and(|t| !t.is_empty()) {
            coder_token
        } else {
            store.get_typed("coder_api_token").await
        };
        match (coder_url, coder_token) {
            (Some(url), Some(token)) if !url.is_empty() && !token.is_empty() => {
                let client = CoderClient::new(&url, &token);
                client.resolve_current_user().await.ok();
                Some(client)
            }
            _ => None,
        }
    }

    async fn send_rejection_follow_up(
        client: &CoderClient,
        chat_id: &str,
        ticket_id: &str,
        report: &str,
    ) -> Result<()> {
        let follow_up = format!(
            "Your review was REJECTED. Please address the following issues and re-submit:\n\n{}",
            report
        );
        client
            .send_chat_message(
                chat_id,
                vec![coder_client::types::ChatInputPart::text(&follow_up)],
            )
            .await?;
        info!(chat_id, ticket_id, "Sent rejection follow-up to forge chat");
        Ok(())
    }

    /// Reduce a worker id (`forge-1`) to its role name (`forge`), used for
    /// looking up chat bindings stored under the role name. Matches
    /// `ForgePairNode::worker_role` / `NexusNode::worker_role`.
    fn worker_role(worker_id: &str) -> &str {
        worker_id
            .rsplit_once('-')
            .map(|(base, _)| base)
            .unwrap_or(worker_id)
    }

    async fn release_sentinel_slots_for_ticket(store: &SharedStore, ticket_id: &str) -> Result<()> {
        let mut slots: HashMap<String, WorkerSlot> =
            store.get_typed(KEY_WORKER_SLOTS).await.unwrap_or_default();
        let mut changed = false;

        for slot in slots.values_mut() {
            if Self::worker_role(&slot.id) != "sentinel" {
                continue;
            }
            let assigned_ticket = match &slot.status {
                WorkerStatus::Assigned { ticket_id, .. }
                | WorkerStatus::Working { ticket_id, .. }
                | WorkerStatus::Done { ticket_id, .. }
                | WorkerStatus::Suspended { ticket_id, .. } => Some(ticket_id.as_str()),
                WorkerStatus::Idle => None,
            };
            if assigned_ticket == Some(ticket_id) {
                slot.status = WorkerStatus::Idle;
                changed = true;
            }
        }

        if changed {
            store
                .set(KEY_WORKER_SLOTS, serde_json::to_value(slots)?)
                .await;
        }
        Ok(())
    }

    async fn remove_rejected_pr_from_pending(
        store: &SharedStore,
        ticket_id: &str,
        pr_number: Option<u64>,
    ) {
        let mut pending_prs: Vec<Value> =
            store.get_typed(KEY_PENDING_PRS).await.unwrap_or_default();
        let before = pending_prs.len();
        pending_prs.retain(|pr| {
            let same_ticket = pr.get("ticket_id").and_then(|v| v.as_str()) == Some(ticket_id);
            let same_pr = pr_number
                .and_then(|n| pr.get("number").and_then(|v| v.as_u64()).map(|m| m == n))
                .unwrap_or(false);
            !(same_ticket || same_pr)
        });

        if pending_prs.len() != before {
            let removed = before - pending_prs.len();
            store.set(KEY_PENDING_PRS, json!(pending_prs)).await;
            info!(
                ticket_id,
                pr_number, removed, "Removed rejected PR from pending_prs so Forge can rework"
            );
        }
    }

    /// Construct a GitHub REST client for SENTINEL's review submission from the
    /// environment (external-auth token, same source VESSEL uses).
    fn github_client_from_env() -> Option<github::GithubRestClient> {
        let token = config::GithubConfig::init_from_env()
            .ok()
            .and_then(|g| g.resolve_token())
            .filter(|t| !t.is_empty())?;
        Some(github::GithubRestClient::new(token))
    }

    /// Parse the store's `"repository"` value (`owner/repo`) into its parts.
    fn parse_repository(repository: Option<&str>) -> (String, String) {
        match repository.and_then(|r| r.split_once('/')) {
            Some((owner, repo)) => (owner.to_string(), repo.to_string()),
            None => (String::new(), String::new()),
        }
    }

    /// Resolve the PR number for a review verdict.
    ///
    /// The SENTINEL chat's `review submit` command may omit `--pr`, which leaves
    /// `ReviewPayload.pr_number == None`. When that happens we fall back to the
    /// PR number Forge recorded when it opened the PR
    /// (`ticket:{id}:pr` -> `{pr_number, branch, title}`). This guarantees the
    /// GitHub review submission (approve / request-changes) always targets the
    /// right PR instead of being silently skipped.
    ///
    /// A supplied `--pr` that disagrees with the ticket's recorded PR is treated
    /// as a reviewer mistake: we log a warning and use the ticket's recorded PR
    /// so a wrong `--pr` can never post APPROVE/REQUEST_CHANGES to another PR.
    #[cfg(test)]
    async fn resolve_pr_number(
        store: &SharedStore,
        ticket_id: &str,
        pr_number: Option<u64>,
    ) -> Option<u64> {
        let pr_key = full_ticket_key_flat(ticket_id, "pr");
        #[derive(serde::Deserialize)]
        struct StoredPr {
            pr_number: u64,
        }
        let stored = store
            .get_typed::<StoredPr>(&pr_key)
            .await
            .map(|p| p.pr_number);

        match (pr_number, stored) {
            (Some(supplied), Some(recorded)) if supplied == recorded => Some(supplied),
            (Some(supplied), Some(recorded)) => {
                warn!(
                    supplied_pr = supplied,
                    recorded_pr = recorded,
                    ticket_id,
                    "Verdict --pr does not match the ticket's recorded PR — using the recorded PR"
                );
                Some(recorded)
            }
            (Some(supplied), None) => Some(supplied),
            (None, stored) => stored,
        }
    }

    /// Derive inline review comments from a SENTINEL report body. Lines matching
    /// a `path:line — message` (or `path:line message`) shape become GitHub
    /// inline review comments so a REQUEST_CHANGES review carries actionable
    /// file:line guidance for FORGE to address.
    fn parse_report_comments(report: &str) -> Vec<github::ReviewCommentInput> {
        let re = regex::Regex::new(r"(?m)^\s*([^\s:]+):(\d+)[\s:\-–—]*\s*(.*)$").unwrap();
        let mut comments = Vec::new();
        for caps in re.captures_iter(report) {
            let Some(path) = caps.get(1) else { continue };
            let Some(line) = caps.get(2) else { continue };
            let msg = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("").trim();
            let Ok(line_num) = line.as_str().parse::<u64>() else {
                continue;
            };
            if !path.as_str().is_empty() && !msg.is_empty() {
                comments.push(github::ReviewCommentInput {
                    path: path.as_str().to_string(),
                    line: line_num,
                    body: msg.to_string(),
                });
            }
        }
        comments
    }

    /// Submit SENTINEL's verdict as a GitHub PR review. Non-fatal: any failure
    /// (e.g. missing token, insufficient scope, network) is logged and the
    /// sharedstore verdict flow is never blocked by it.
    async fn submit_github_review(
        store: &SharedStore,
        pr_number: u64,
        event: &str,
        body: &str,
        comments: Vec<github::ReviewCommentInput>,
        head: &str,
    ) -> bool {
        let repository: Option<String> = store.get_typed("repository").await;
        let (owner, repo) = Self::parse_repository(repository.as_deref());
        if owner.is_empty() || repo.is_empty() {
            warn!(
                pr_number,
                event, "Repository info missing — cannot submit GitHub PR review"
            );
            return false;
        }
        let Some(client) = Self::github_client_from_env() else {
            warn!(
                pr_number,
                event, "No GitHub token — cannot submit GitHub PR review"
            );
            return false;
        };
        if let Err(e) = client
            .submit_pull_request_review(&owner, &repo, pr_number, event, body, comments, Some(head))
            .await
        {
            warn!(
                pr_number,
                event,
                error = %e,
                "Failed to submit GitHub PR review (non-fatal)"
            );
        } else {
            info!(pr_number, event, "Submitted GitHub PR review");
            return true;
        }
        false
    }
}

#[async_trait]
impl Node for SentinelNode {
    fn name(&self) -> &str {
        "sentinel"
    }

    async fn prep(&self, store: &SharedStore) -> Result<Value> {
        let tickets: Vec<Ticket> = store.get_typed(KEY_TICKETS).await.unwrap_or_default();
        let _slots: HashMap<String, WorkerSlot> =
            store.get_typed(KEY_WORKER_SLOTS).await.unwrap_or_default();

        let mut reviewable = Vec::new();
        let planning_gate_pending: Vec<Value> = Vec::new();

        for ticket in &tickets {
            let worker_id = match &ticket.status {
                TicketStatus::InProgress { worker_id } => worker_id.clone(),
                TicketStatus::Assigned { worker_id } => worker_id.clone(),
                _ => continue,
            };

            let lifecycle = store.lifecycle(&ticket.id).await?;
            let has_review = lifecycle.pr_delivery.is_some();
            if let Some(review) = &lifecycle.pr_delivery {
                reviewable.push(json!({"ticket_id":ticket.id,"worker_id":worker_id,"verdict":if review.approved {"approve"} else {"reject"},"report":review.report,"revision":review.revision,"round":review.round,"head":review.head,"pr_number":lifecycle.pr_number,"review_type":"pr_review"}));
            }

            let status_key = full_ticket_key_flat(&ticket.id, KEY_TICKET_STATUS);
            let status_json: Option<Value> = store.get_typed(&status_key).await;
            let phase = status_json
                .as_ref()
                .and_then(|v| v.get("phase"))
                .and_then(|v| v.as_str());
            // Monitor the review-type-scoped SENTINEL chat for the current phase so
            // a planning-gate chat and a PR-review chat are tracked independently.
            let (monitor_chat_key, monitor_action_key) = if phase == Some("planning") {
                (
                    review_chat_key(&ticket.id, REVIEW_TYPE_PLANNING_GATE),
                    review_action_key(&ticket.id, REVIEW_TYPE_PLANNING_GATE),
                )
            } else {
                (
                    review_chat_key(&ticket.id, REVIEW_TYPE_PR),
                    review_action_key(&ticket.id, REVIEW_TYPE_PR),
                )
            };
            let chat_id: Option<String> = store.get_typed(&monitor_chat_key).await;
            if let Some(chat_id) = chat_id {
                if let Some(client) = Self::coder_client_from_store(store).await {
                    if let Ok(chat) = client.get_chat(&chat_id).await {
                        let action_key = monitor_action_key;
                        let last_action: Option<String> = store.get_typed(&action_key).await;

                        match chat.status() {
                            ChatStatus::Running => {
                                debug!(
                                    ticket_id = %ticket.id,
                                    "Sentinel chat still running — waiting for review"
                                );
                            }
                            ChatStatus::Waiting
                                if (last_action.as_deref() == Some("completed")
                                    || last_action.is_none())
                                    && !has_review =>
                            {
                                info!(
                                    ticket_id = %ticket.id,
                                    "Sentinel chat waiting but no review written yet — sending follow-up"
                                );
                            }
                            ChatStatus::Error => {
                                warn!(
                                    ticket_id = %ticket.id,
                                    "Sentinel chat in error status"
                                );
                                store.set(&action_key, json!("interrupted")).await;
                            }
                            ChatStatus::RequiresAction => {
                                info!(
                                    ticket_id = %ticket.id,
                                    "Sentinel chat requires_action"
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(json!({
            "reviewable": reviewable,
            "planning_gate_pending": planning_gate_pending,
        }))
    }

    async fn exec(&self, prep_result: Value) -> Result<Value> {
        let reviewable = prep_result["reviewable"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let planning_gate_pending = prep_result["planning_gate_pending"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if reviewable.is_empty() && planning_gate_pending.is_empty() {
            return Ok(
                json!({ "verdicts": [], "has_reviews": false, "has_planning_gates": false }),
            );
        }

        info!(
            review_count = reviewable.len(),
            planning_gate_count = planning_gate_pending.len(),
            "Sentinel: processing reviews and planning gates"
        );

        let mut verdicts = Vec::new();
        for review in &reviewable {
            let ticket_id = review["ticket_id"].as_str().unwrap_or("");
            let worker_id = review["worker_id"].as_str().unwrap_or("");
            let verdict = review["verdict"].as_str().unwrap_or("");
            let review_type = review["review_type"].as_str().unwrap_or("pr_review");
            verdicts.push(json!({
                "ticket_id": ticket_id,
                "worker_id": worker_id,
                "verdict": verdict,
                "review_type": review_type,
                "revision": review["revision"],
                "round": review["round"],
                "head": review["head"],
            }));
        }

        // Planning gate tickets are pending review by SENTINEL (chat is active).
        // The actual review (approve/reject) happens inside the chat — the
        // controller just needs to route these tickets correctly.
        // If a planning gate has been approved by the chat, it will be
        // detected in post() via the gate key in SharedStore.
        for gate in &planning_gate_pending {
            let ticket_id = gate["ticket_id"].as_str().unwrap_or("");
            let worker_id = gate["worker_id"].as_str().unwrap_or("");
            verdicts.push(json!({
                "ticket_id": ticket_id,
                "worker_id": worker_id,
                "verdict": "planning_gate_pending",
                "review_type": "planning_gate",
            }));
        }

        Ok(json!({
            "verdicts": verdicts,
            "has_reviews": !reviewable.is_empty(),
            "has_planning_gates": !planning_gate_pending.is_empty(),
        }))
    }

    async fn post(&self, store: &SharedStore, exec_result: Value) -> Result<Action> {
        let verdicts = exec_result["verdicts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut any_approved = false;
        let mut any_rejected = false;
        let client = Self::coder_client_from_store(store).await;
        for verdict in verdicts {
            let Some(ticket) = verdict["ticket_id"].as_str() else {
                continue;
            };
            let state = store.lifecycle(ticket).await?;
            let Some(decision) = &state.pr_delivery else {
                continue;
            };
            // The rejection is already durable and has returned the worker to building.
            // Deliver feedback independently of GitHub availability.
            if !decision.approved {
                Self::remove_rejected_pr_from_pending(store, ticket, state.pr_number).await;
                let marker = format!("ticket:{ticket}:review_feedback:{}", decision.round);
                if store.get(&marker).await.is_none() {
                    if let (Some(client), Some(chat)) = (
                        &client,
                        store
                            .get_typed::<String>(&full_ticket_key(ticket, KEY_TICKET_CHAT, "forge"))
                            .await,
                    ) {
                        if Self::send_rejection_follow_up(client, &chat, ticket, &decision.report)
                            .await
                            .is_ok()
                        {
                            store.set(&marker, json!(true)).await;
                        }
                    } else {
                        store
                            .set(
                                &full_ticket_key(ticket, KEY_TICKET_CHAT_ACTION, "forge"),
                                json!("resume_needed"),
                            )
                            .await;
                    }
                }
                any_rejected = true;
            }
            let Some(pr) = state.pr_number else { continue };
            if !Self::submit_github_review(
                store,
                pr,
                if decision.approved {
                    "APPROVE"
                } else {
                    "REQUEST_CHANGES"
                },
                &decision.report,
                if decision.approved {
                    vec![]
                } else {
                    Self::parse_report_comments(&decision.report)
                },
                decision.head.as_deref().unwrap_or(""),
            )
            .await
            {
                continue;
            }
            store
                .transition(
                    ticket,
                    state.version,
                    "sentinel",
                    config::lifecycle::Event::ReviewDelivered {
                        round: decision.round,
                    },
                )
                .await?;
            Self::release_sentinel_slots_for_ticket(store, ticket).await?;
            let chat_key = review_chat_key(ticket, REVIEW_TYPE_PR);
            if let (Some(client), Some(chat)) =
                (&client, store.get_typed::<String>(&chat_key).await)
            {
                let _ = client.archive_chat(&chat).await;
            }
            store.del(&chat_key).await;
            store
                .set(
                    &review_action_key(ticket, REVIEW_TYPE_PR),
                    json!("completed"),
                )
                .await;
            any_approved |= decision.approved;
        }
        Ok(Action::new(if any_approved {
            ACTION_REVIEW_APPROVE
        } else if any_rejected {
            ACTION_REVIEW_REJECT
        } else {
            PAUSE_SIGNAL
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> SentinelNode {
        SentinelNode::new("sentinel.agent.md")
    }

    #[test]
    fn worker_role_strips_numeric_suffix() {
        assert_eq!(SentinelNode::worker_role("forge-1"), "forge");
        assert_eq!(SentinelNode::worker_role("forge-42"), "forge");
        assert_eq!(SentinelNode::worker_role("sentinel"), "sentinel");
        assert_eq!(SentinelNode::worker_role("vessel-1"), "vessel");
    }

    #[tokio::test]
    async fn exec_preserves_worker_id_for_pr_review_and_planning_gate() {
        let prep = json!({
            "reviewable": [{
                "ticket_id": "T-1",
                "worker_id": "forge-1",
                "verdict": "reject",
                "report": "fix it",
                "review_type": "pr_review",
            }],
            "planning_gate_pending": [{
                "ticket_id": "T-2",
                "worker_id": "forge-2",
                "review_type": "planning_gate",
            }],
        });

        let out = node().exec(prep).await.unwrap();
        let verdicts = out["verdicts"].as_array().unwrap();

        let pr = verdicts
            .iter()
            .find(|v| v["review_type"] == "pr_review")
            .unwrap();
        assert_eq!(pr["worker_id"], "forge-1");
        assert_eq!(pr["verdict"], "reject");

        let gate = verdicts
            .iter()
            .find(|v| v["review_type"] == "planning_gate")
            .unwrap();
        assert_eq!(gate["worker_id"], "forge-2");
        assert_eq!(gate["verdict"], "planning_gate_pending");
    }

    #[tokio::test]
    async fn post_reject_preserves_delivery_when_github_is_unavailable() {
        use config::lifecycle::{Decision, Lifecycle, Phase};
        let store = SharedStore::new_in_memory();
        let ticket = "T-42";
        let decision = Decision {
            round: 3,
            actor: "sentinel".into(),
            approved: false,
            report: "Fix failing test".into(),
            revision: 1,
            head: Some("abc".into()),
        };
        let state = Lifecycle {
            phase: Phase::Building,
            version: 8,
            pr_number: Some(42),
            feedback: Some(decision.report.clone()),
            pr_delivery: Some(decision),
            ..Default::default()
        };
        store
            .set(
                &full_ticket_key_flat(ticket, KEY_TICKET_STATUS),
                serde_json::to_value(state).unwrap(),
            )
            .await;
        let node = SentinelNode::new("registry.json");
        let result = node
            .post(&store, json!({"verdicts":[{"ticket_id":ticket}]}))
            .await
            .unwrap();
        assert_eq!(result.as_str(), ACTION_REVIEW_REJECT);
        let state = store.lifecycle(ticket).await.unwrap();
        assert_eq!(state.phase, Phase::Building);
        assert!(state.pr_delivery.is_some());
        assert_eq!(
            store
                .get_typed::<String>(&full_ticket_key(ticket, KEY_TICKET_CHAT_ACTION, "forge"))
                .await
                .as_deref(),
            Some("resume_needed")
        );
    }

    #[test]
    fn action_constants_match_expected_action_names() {
        assert_eq!(ACTION_REVIEW_APPROVE, "review_approve");
        assert_eq!(ACTION_REVIEW_REJECT, "review_reject");
    }

    #[test]
    fn parse_report_comments_extracts_file_line_guidance() {
        let report = "The PR needs work:\n\n\
                      src/api.rs:78 — missing pagination; required per spec\n\
                      src/api.rs:78  also add a cursor param\n\
                      tests/integration.rs:12 — flaky assertion\n\
                      not-a-location line\n\
                      src/models.rs:5 —\n";
        let comments = SentinelNode::parse_report_comments(report);
        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].path, "src/api.rs");
        assert_eq!(comments[0].line, 78);
        assert!(comments[0].body.contains("missing pagination"));
        assert_eq!(comments[1].path, "src/api.rs");
        assert_eq!(comments[1].line, 78);
        assert!(comments[1].body.contains("cursor param"));
        assert_eq!(comments[2].path, "tests/integration.rs");
        assert_eq!(comments[2].line, 12);
    }

    #[test]
    fn parse_report_comments_empty_for_no_matches() {
        assert!(SentinelNode::parse_report_comments("no guidance here").is_empty());
        assert!(SentinelNode::parse_report_comments("").is_empty());
    }

    #[test]
    fn parse_repository_splits_owner_repo() {
        assert_eq!(
            SentinelNode::parse_repository(Some("owner/repo")),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            SentinelNode::parse_repository(Some("single")),
            (String::new(), String::new())
        );
        assert_eq!(
            SentinelNode::parse_repository(None),
            (String::new(), String::new())
        );
    }

    #[tokio::test]
    async fn resolve_pr_number_uses_verdict_when_present() {
        let store = SharedStore::new_in_memory();
        let pr = SentinelNode::resolve_pr_number(&store, "T-1", Some(58)).await;
        assert_eq!(pr, Some(58));
    }

    #[tokio::test]
    async fn resolve_pr_number_backfills_from_ticket_pr_info() {
        let store = SharedStore::new_in_memory();
        let ticket_id = "T-2";
        // Simulate the verdict omitting --pr (pr_number None) while Forge
        // recorded the opened PR at ticket:{id}:pr.
        store
            .set(
                &full_ticket_key_flat(ticket_id, "pr"),
                json!({ "pr_number": 58, "branch": "feature/T-2", "title": "T-2 work" }),
            )
            .await;
        let pr = SentinelNode::resolve_pr_number(&store, ticket_id, None).await;
        assert_eq!(pr, Some(58));
    }

    #[tokio::test]
    async fn resolve_pr_number_none_when_no_verdict_and_no_pr_info() {
        let store = SharedStore::new_in_memory();
        let pr = SentinelNode::resolve_pr_number(&store, "T-3", None).await;
        assert_eq!(pr, None);
    }
}
