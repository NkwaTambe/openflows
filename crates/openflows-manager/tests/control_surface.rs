//! Integration tests for the authenticated `/api/v1` control surface:
//! tenant add/list, control pause/resume, and task assignment. These run
//! against the real router with an in-memory store, so no Redis is required.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use openflows_manager::server::{create_router, AppState, TEST_AUTH_TOKEN};
use serde_json::{json, Value};
use tower::ServiceExt;

const NO_TOKEN: Option<&str> = None;

/// Send a request through the router and return (status, parsed JSON body).
async fn send(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn app() -> Router {
    create_router(AppState::for_tests())
}

// ── Auth ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn api_requires_auth() {
    let app = app();
    let (status, body) = send(&app, Method::GET, "/api/v1/tenants", None, NO_TOKEN).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn api_rejects_wrong_token() {
    let app = app();
    let (status, _) = send(&app, Method::GET, "/api/v1/tenants", None, Some("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_stays_open_without_auth() {
    let app = app();
    let (status, body) = send(&app, Method::GET, "/health", None, NO_TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready");
}

// ── Tenants ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn tenant_add_and_list_roundtrip() {
    let app = app();

    // Empty list initially.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/tenants",
        None,
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenants"].as_array().unwrap().len(), 0);

    // Add a tenant with a fleet.
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/tenants",
        Some(json!({"repo": "acme/rep", "name": "acme", "fleet": 2})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "acme");
    assert_eq!(body["fleet"], 2);
    assert_eq!(body["control_mode"], "auto");

    // Add a tenant with no explicit name (defaults to repo owner).
    send(
        &app,
        Method::POST,
        "/api/v1/tenants",
        Some(json!({"repo": "other/repo"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;

    // List reflects both, with repositories.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/tenants",
        None,
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tenants = body["tenants"].as_array().unwrap();
    assert_eq!(tenants.len(), 2);
    let names: Vec<&str> = tenants.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"acme"));
    assert!(names.contains(&"other"));
}

#[tokio::test]
async fn tenant_add_validates_repo_shape() {
    let app = app();
    // Missing '/' in repo.
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/tenants",
        Some(json!({"repo": "not-a-repo"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── Control ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn control_pause_resume_roundtrip() {
    let app = app();
    send(
        &app,
        Method::POST,
        "/api/v1/tenants",
        Some(json!({"repo": "acme/rep", "name": "acme"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;

    // Default is auto.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/tenants/acme/control",
        None,
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["control_mode"], "auto");

    // Pause.
    let (status, body) = send(
        &app,
        Method::PUT,
        "/api/v1/tenants/acme/control",
        Some(json!({"mode": "paused"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["control_mode"], "paused");
    assert_eq!(body["previous_control_mode"], "auto");

    // Read back.
    let (_, body) = send(
        &app,
        Method::GET,
        "/api/v1/tenants/acme/control",
        None,
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(body["control_mode"], "paused");

    // Resume.
    let (status, _) = send(
        &app,
        Method::PUT,
        "/api/v1/tenants/acme/control",
        Some(json!({"mode": "auto"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn control_rejects_invalid_mode() {
    let app = app();
    let (status, body) = send(
        &app,
        Method::PUT,
        "/api/v1/tenants/acme/control",
        Some(json!({"mode": "bogus"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("invalid mode"));
}

// ── Tasks ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn assign_task_is_visible_to_tenant() {
    let app = app();
    send(
        &app,
        Method::POST,
        "/api/v1/tenants",
        Some(json!({"repo": "acme/rep", "name": "acme"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;

    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/tenants/acme/tasks",
        Some(json!({"title": "Fix bug", "body": "details", "payload": {"kind": "bug"}})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["task"]["title"], "Fix bug");
    assert_eq!(body["task"]["payload"]["kind"], "bug");
    assert_eq!(body["tenant"], "acme");

    // Second task gets a distinct id (proves the list persists/appends).
    let (_, body2) = send(
        &app,
        Method::POST,
        "/api/v1/tenants/acme/tasks",
        Some(json!({"title": "Second"})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_ne!(body["task"]["id"], body2["task"]["id"]);
}

#[tokio::test]
async fn assign_task_requires_title() {
    let app = app();
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/tenants/acme/tasks",
        Some(json!({"title": " "})),
        Some(TEST_AUTH_TOKEN),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
