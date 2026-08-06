//! openflows-harness — typed SharedStore CLI for Coder Agent worker workspaces.
//!
//! The Coder Agent invokes this binary via shell (`execute` tool), guided by
//! role skills. It is the ONLY thing that reads/writes Redis from inside a
//! workspace. All writes are validated against typed schemas from `config::state`.
//!
//! Required environment (injected by the workspace template):
//!   REDIS_URL          — Redis SharedStore URL
//!   OPENFLOWS_TENANT   — Tenant identifier (key prefix)
//!   OPENFLOWS_TICKET   — Current ticket ID (e.g., "T-42")
//!   OPENFLOWS_ROLE     — Current role (forge, sentinel, vessel, lore)

mod store;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "openflows-harness")]
#[command(about = "Typed SharedStore CLI for Coder Agent worker workspaces")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read the task dispatch for this ticket+role
    #[command(name = "dispatch")]
    Dispatch {
        #[command(subcommand)]
        action: DispatchAction,
    },
    /// Set the current phase for this ticket
    #[command(name = "status")]
    Status {
        #[command(subcommand)]
        action: StatusAction,
    },
    /// Write a handoff contract (forge → sentinel)
    #[command(name = "handoff")]
    Handoff {
        #[command(subcommand)]
        action: HandoffAction,
    },
    /// Record that a PR was opened
    #[command(name = "pr")]
    Pr {
        #[command(subcommand)]
        action: PrAction,
    },
    /// Submit a review verdict (sentinel)
    #[command(name = "review")]
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Record that a merge completed (vessel)
    #[command(name = "merge")]
    Merge {
        #[command(subcommand)]
        action: MergeAction,
    },
    /// Manage heartbeat writing (daemonized)
    #[command(name = "heartbeat")]
    Heartbeat {
        #[command(subcommand)]
        action: HeartbeatAction,
    },
    /// Manage phase gates (SENTINEL approval for phase transitions)
    #[command(name = "gate")]
    Gate {
        #[command(subcommand)]
        action: GateAction,
    },
    /// Delegated verification via A2A relay (issue #143)
    #[command(name = "verify")]
    Verify {
        #[command(subcommand)]
        action: VerifyAction,
    },
}

#[derive(Subcommand)]
enum DispatchAction {
    /// Read the dispatch payload for this ticket+role
    Read,
}

#[derive(Subcommand)]
enum StatusAction {
    /// Set the current phase
    Set {
        /// Phase: planning, building, testing, review_ready, blocked
        phase: String,
    },
    /// Read the current status JSON for this ticket (empty JSON if unset)
    Get,
}

#[derive(Subcommand)]
enum HandoffAction {
    /// Write a handoff contract
    Write {
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        notes: Option<String>,
    },
}

#[derive(Subcommand)]
enum PrAction {
    /// Read the recorded PR info for this ticket (empty JSON if unset)
    Get,
    /// Record that a PR was opened
    Opened {
        #[arg(long)]
        pr: u64,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        title: String,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    /// Submit a review verdict
    Submit {
        #[arg(long)]
        verdict: String,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        pr: Option<u64>,
    },
}

#[derive(Subcommand)]
enum MergeAction {
    /// Record that a merge completed
    Done {
        #[arg(long)]
        pr: u64,
        #[arg(long)]
        sha: String,
    },
}

#[derive(Subcommand)]
enum HeartbeatAction {
    /// Start daemonized heartbeat writing (every 30s)
    Start,
    /// Stop heartbeat writing
    Stop,
}

#[derive(Subcommand)]
enum GateAction {
    /// Approve a gated phase transition (SENTINEL → FORGE)
    Approve {
        /// Phase to approve (e.g., "planning")
        #[arg(long)]
        phase: String,
        /// Optional notes about the approval
        #[arg(long)]
        notes: Option<String>,
    },
    /// Check gate approval status
    Status {
        /// Phase to check
        #[arg(long)]
        phase: String,
    },
}

#[derive(Subcommand)]
enum VerifyAction {
    /// Submit a verify request (SENTINEL-side, task 3 of issue #143)
    Request {
        /// Command argv to execute (must be allowlisted)
        #[arg(long)]
        argv: Vec<String>,
        /// Command execution timeout in seconds
        #[arg(long, default_value = "600")]
        timeout_secs: u64,
        /// Expected exit code (if None, any exit code is acceptable)
        #[arg(long)]
        expect_exit: Option<i32>,
        /// Optional comma-separated list of artifact paths to hash
        #[arg(long)]
        artifacts: Option<String>,
    },
    /// Long-running executor (FORGE-side, task 3 of issue #143)
    Serve,
    /// List recent verification results (humans/audit)
    List {
        /// Filter by pair ID (optional)
        #[arg(long)]
        pair_id: Option<String>,
    },
}

fn require_env(name: &str) -> Result<String> {
    std::env::var(name).context(format!(
        "{} is not set. This must be injected by the workspace template.",
        name
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let redis_url = require_env("REDIS_URL")?;
    let tenant = require_env("OPENFLOWS_TENANT")?;
    let ticket = require_env("OPENFLOWS_TICKET")?;
    let role = require_env("OPENFLOWS_ROLE")?;

    let store = store::HarnessStore::new(&redis_url, &tenant).await?;

    match cli.command {
        Commands::Dispatch {
            action: DispatchAction::Read,
        } => {
            store.dispatch_read(&ticket, &role).await?;
        }
        Commands::Status {
            action: StatusAction::Set { phase },
        } => {
            store.status_set(&ticket, &role, &phase).await?;
        }
        Commands::Status {
            action: StatusAction::Get,
        } => {
            store.status_get(&ticket).await?;
        }
        Commands::Handoff {
            action: HandoffAction::Write { contract, notes },
        } => {
            store
                .handoff_write(&ticket, &contract, notes.as_deref())
                .await?;
        }
        Commands::Pr {
            action: PrAction::Opened { pr, branch, title },
        } => {
            store.pr_opened(&ticket, &pr, &branch, &title).await?;
        }
        Commands::Pr {
            action: PrAction::Get,
        } => {
            store.pr_get(&ticket).await?;
        }
        Commands::Review {
            action:
                ReviewAction::Submit {
                    verdict,
                    report,
                    pr,
                },
        } => {
            store
                .review_submit(&ticket, &role, &verdict, &report, pr)
                .await?;
        }
        Commands::Merge {
            action: MergeAction::Done { pr, sha },
        } => {
            store.merge_done(&ticket, &pr, &sha).await?;
        }
        Commands::Heartbeat {
            action: HeartbeatAction::Start,
        } => {
            store.heartbeat_start(&ticket, &role).await?;
        }
        Commands::Heartbeat {
            action: HeartbeatAction::Stop,
        } => {
            store.heartbeat_stop(&ticket, &role).await?;
        }
        Commands::Gate {
            action: GateAction::Approve { phase, notes },
        } => {
            store
                .gate_approve(&ticket, &role, &phase, notes.as_deref())
                .await?;
        }
        Commands::Gate {
            action: GateAction::Status { phase },
        } => {
            store.gate_status(&ticket, &phase).await?;
        }
        Commands::Verify {
            action: VerifyAction::Request {
                argv,
                timeout_secs,
                expect_exit,
                artifacts,
            },
        } => {
            store
                .verify_request(&ticket, argv, timeout_secs, expect_exit, artifacts.as_deref())
                .await?;
        }
        Commands::Verify {
            action: VerifyAction::Serve,
        } => {
            store.verify_serve(&ticket, &role).await?;
        }
        Commands::Verify {
            action: VerifyAction::List { pair_id },
        } => {
            store.verify_list(pair_id.as_deref()).await?;
        }
    }

    Ok(())
}
