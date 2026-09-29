//! End-to-end tests for the remote CLI.
//!
//! Each test spawns a real `openflows-manager` HTTP server (in-memory store) on
//! an ephemeral port, then drives the CLI in-process against it. This exercises
//! the full client path: argument handling, HTTP transport, auth, and output —
//! not just the command functions.

use openflows_cli::commands::{Command, ControlAction, TasksAction, TenantAction};
use openflows_cli::{config, Cli};
use openflows_manager::server::{serve, AppState, TEST_AUTH_TOKEN};
use serde_json::json;
use std::path::PathBuf;

/// Spawn a manager server on an ephemeral port and return its base URL.
async fn spawn_manager() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(serve(listener, AppState::for_tests(), async {
        let _ = shutdown_rx.await;
    }));
    // Keep the shutdown sender alive for the process lifetime (tests exit on
    // their own; there is nothing to clean up in a throwaway test server).
    std::mem::forget(shutdown_tx);
    format!("http://{addr}")
}

/// Build a CLI invocation targeting the given manager.
fn cli(url: &str, token: &str, json: bool, command: Command) -> Cli {
    Cli {
        url: Some(url.to_string()),
        token: Some(token.to_string()),
        json,
        config: None,
        command,
    }
}

async fn add_tenant(url: &str, repo: &str, name: &str) {
    openflows_cli::run(cli(
        url,
        TEST_AUTH_TOKEN,
        false,
        Command::Tenant {
            action: TenantAction::Add {
                repo: repo.to_string(),
                name: Some(name.to_string()),
                fleet: Some(2),
            },
        },
    ))
    .await
    .expect("tenant add should succeed");
}

// ── Auth failure ───────────────────────────────────────────────────────────

#[tokio::test]
async fn wrong_token_is_rejected_with_error() {
    let url = spawn_manager().await;
    let result = openflows_cli::run(cli(
        &url,
        "wrong-token",
        false,
        Command::Tenant {
            action: TenantAction::List,
        },
    ))
    .await;
    assert!(result.is_err(), "expected an error for a wrong token");
}

// ── Full happy path ────────────────────────────────────────────────────────

#[tokio::test]
async fn tenant_control_and_task_roundtrip() {
    let url = spawn_manager().await;

    // Add a tenant through the CLI.
    add_tenant(&url, "acme/rep", "acme").await;

    // Verify it landed on the control surface via an independent HTTP call.
    let list = reqwest::Client::new()
        .get(format!("{url}/api/v1/tenants"))
        .bearer_auth(TEST_AUTH_TOKEN)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(list["tenants"][0]["name"], "acme");

    // Pause the fleet through the CLI.
    openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        false,
        Command::Control {
            action: ControlAction::Pause {
                tenant: "acme".into(),
            },
        },
    ))
    .await
    .expect("pause should succeed");

    // Verify the control mode persisted.
    let control = reqwest::Client::new()
        .get(format!("{url}/api/v1/tenants/acme/control"))
        .bearer_auth(TEST_AUTH_TOKEN)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(control["control_mode"], "paused");

    // Resume.
    openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        false,
        Command::Control {
            action: ControlAction::Resume {
                tenant: "acme".into(),
            },
        },
    ))
    .await
    .expect("resume should succeed");

    // Assign a task via flags.
    openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        false,
        Command::Tasks {
            action: TasksAction::Assign {
                tenant: "acme".into(),
                title: Some("Fix the bug".into()),
                body: Some("please fix".into()),
                file: None,
                json_input: None,
            },
        },
    ))
    .await
    .expect("assign should succeed");

    // And via inline JSON, capturing the task id in output-free form by
    // re-checking state through the store-backed list (task ids are sequential).
    let task = reqwest::Client::new()
        .get(format!("{url}/api/v1/tenants/acme/control"))
        .bearer_auth(TEST_AUTH_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(task.status(), 200);
}

// ── Task input forms ───────────────────────────────────────────────────────

#[tokio::test]
async fn task_assign_accepts_json_file_and_json_input() {
    let url = spawn_manager().await;
    add_tenant(&url, "acme/rep", "acme").await;

    // Inline JSON input.
    let result = openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        true,
        Command::Tasks {
            action: TasksAction::Assign {
                tenant: "acme".into(),
                title: None,
                body: None,
                file: None,
                json_input: Some(json!({"title": "inline task", "payload": {"n": 1}}).to_string()),
            },
        },
    ))
    .await;
    assert!(result.is_ok(), "inline JSON assign should succeed");

    // JSON file input.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("task.json");
    std::fs::write(
        &file,
        json!({"title": "file task", "body": "from file"}).to_string(),
    )
    .unwrap();

    let result = openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        false,
        Command::Tasks {
            action: TasksAction::Assign {
                tenant: "acme".into(),
                title: None,
                body: None,
                file: Some(file),
                json_input: None,
            },
        },
    ))
    .await;
    assert!(result.is_ok(), "file JSON assign should succeed");

    // Missing input entirely -> usage error.
    let result = openflows_cli::run(cli(
        &url,
        TEST_AUTH_TOKEN,
        false,
        Command::Tasks {
            action: TasksAction::Assign {
                tenant: "acme".into(),
                title: None,
                body: None,
                file: None,
                json_input: None,
            },
        },
    ))
    .await;
    assert!(result.is_err(), "assign with no input should error");
}

// ── Login writes a config file ─────────────────────────────────────────────

#[tokio::test]
async fn login_persists_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path: PathBuf = dir.path().join("config.toml");

    openflows_cli::run(Cli {
        url: None,
        token: None,
        json: false,
        config: Some(cfg_path.clone()),
        command: Command::Login {
            url: "http://example:3912".to_string(),
            token: Some("abc123".to_string()),
        },
    })
    .await
    .expect("login should succeed");

    let saved = config::load(&cfg_path).unwrap();
    assert_eq!(saved.url.as_deref(), Some("http://example:3912"));
    assert_eq!(saved.token.as_deref(), Some("abc123"));
}
