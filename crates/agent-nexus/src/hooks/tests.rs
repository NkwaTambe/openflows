// crates/agent-nexus/src/hooks/tests.rs
//! Unit + end-to-end tests for the lifecycle-hook consumer.

use super::jwt::{sha256_hex, verify_hook_jwt};
use super::types::{HookDecision, HookEvent, HookPayload};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};

const SECRET: &str = "a-very-long-secret-of-at-least-thirty-two-bytes!";
const URL: &str = "http://localhost:3001/experimental/hooks/chat";
const CHAT: &str = "chat-abc";
const EVENT: &str = "session_start";

fn sign(secret: &str, claims: &Value) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

fn make_payload(dispatch_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": EVENT,
        "meta": {
            "dispatch_id": dispatch_id,
            "schema_version": 1,
            "chat_id": CHAT,
            "owner_id": "owner-1"
        },
        "data": {"source": "startup"}
    }))
    .unwrap()
}

fn token_for(dispatch_id: &str, body: &[u8]) -> String {
    sign(
        SECRET,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": dispatch_id,
            "type": EVENT,
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": sha256_hex(body),
        }),
    )
}

fn verify(secret: &str, token: &str, dispatch_id: &str, body: &[u8]) -> anyhow::Result<()> {
    verify_hook_jwt(
        Some(&format!("Bearer {token}")),
        secret,
        URL,
        dispatch_id,
        CHAT,
        EVENT,
        body,
    )?;
    Ok(())
}

#[test]
fn verifies_a_valid_hs256_jwt() {
    let body = make_payload("dispatch-1");
    let token = token_for("dispatch-1", &body);
    verify(SECRET, &token, "dispatch-1", &body).expect("valid JWT should verify");
}

#[test]
fn accepts_deployment_id_issuer() {
    // Coder signs `iss` with its per-deployment ID (not the literal "coder").
    // The consumer must accept any non-empty issuer, matching Coder's reference
    // consumer. This was the bug that rejected all real dispatches.
    let body = make_payload("d-iss");
    let token = sign(
        SECRET,
        &json!({
            "iss": "00000000-0000-0000-0000-000000000000",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": "d-iss",
            "type": EVENT,
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": sha256_hex(&body),
        }),
    );
    verify(SECRET, &token, "d-iss", &body).expect("deployment-ID issuer should verify");
}

#[test]
fn rejects_missing_bearer_header() {
    let body = make_payload("d1");
    assert!(verify_hook_jwt(None, SECRET, URL, "d1", CHAT, EVENT, &body).is_err());
}

#[test]
fn rejects_wrong_secret() {
    let wrong = "a-different-secret-that-is-also-long-enough!!!";
    let body = make_payload("d1");
    let token = sign(
        wrong,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": "d1",
            "type": EVENT,
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": sha256_hex(&body),
        }),
    );
    assert!(verify(SECRET, &token, "d1", &body).is_err());
}

#[test]
fn rejects_dispatch_id_mismatch() {
    let body = make_payload("d1");
    assert!(verify(SECRET, &token_for("OTHER", &body), "d1", &body).is_err());
}

#[test]
fn rejects_expired_token() {
    let body = make_payload("d1");
    let token = sign(
        SECRET,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 1usize,
            "jti": "d1",
            "type": EVENT,
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": sha256_hex(&body),
        }),
    );
    assert!(verify(SECRET, &token, "d1", &body).is_err());
}

#[test]
fn rejects_body_sha256_mismatch() {
    let body = make_payload("d1");
    let token = sign(
        SECRET,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": "d1",
            "type": EVENT,
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": "deadbeef",
        }),
    );
    assert!(verify(SECRET, &token, "d1", &body).is_err());
}

#[test]
fn rejects_type_claim_mismatch() {
    let body = make_payload("d1");
    let token = sign(
        SECRET,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": "d1",
            "type": "stop",
            "sub": format!("coder:chat:{}", CHAT),
            "body_sha256": sha256_hex(&body),
        }),
    );
    assert!(verify(SECRET, &token, "d1", &body).is_err());
}

#[test]
fn rejects_chat_subject_mismatch() {
    let body = make_payload("d1");
    let token = sign(
        SECRET,
        &json!({
            "iss": "coder",
            "aud": URL,
            "exp": 4_000_000_000usize,
            "jti": "d1",
            "type": EVENT,
            "sub": "coder:chat:OTHER-CHAT",
            "body_sha256": sha256_hex(&body),
        }),
    );
    assert!(verify(SECRET, &token, "d1", &body).is_err());
}

#[test]
fn mutable_events_allow_deny_rewrite() {
    assert!(HookEvent::UserPromptSubmit.is_mutable());
    assert!(HookEvent::PreToolUse.is_mutable());
    assert!(!HookEvent::SessionStart.is_mutable());
    assert!(!HookEvent::Stop.is_mutable());

    let deny = HookDecision::deny("blocked by policy");
    assert!(deny.deny);
    assert_eq!(deny.reason.as_deref(), Some("blocked by policy"));

    let observe = HookDecision::observe();
    assert!(!observe.deny);
}

#[test]
fn parses_hook_payload_real_coder_envelope() {
    let raw = json!({
        "type": "pre_tool_use",
        "meta": {
            "dispatch_id": "dis-1",
            "schema_version": 1,
            "chat_id": "c-1",
            "owner_id": "o-1",
            "workspace_id": "ws-1",
            "turn_id": "t-1"
        },
        "data": {"tool_use_id": "tu-1", "tool_name": "Bash", "tool_input": {"command": "rm -rf /"}}
    });
    let payload: HookPayload = serde_json::from_value(raw).unwrap();
    assert_eq!(payload.event, HookEvent::PreToolUse);
    assert!(payload.event.is_mutable());
    assert_eq!(payload.meta.chat_id, "c-1");
    assert_eq!(payload.meta.dispatch_id, "dis-1");
    assert_eq!(payload.data["tool_name"], "Bash");
}

#[tokio::test]
async fn consumer_accepts_signed_dispatch_end_to_end() {
    use super::server::create_router;
    use super::simulate::dispatch_simulated_event;
    use config::env::CoderHooksConfig;
    use pocketflow_core::SharedStore;
    use std::sync::Arc;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hook_url = format!("http://{addr}/experimental/hooks/chat");

    // The consumer derives its audience from hook_addr + hook_host. Set both so
    // the derived URL matches what the (reachable) listener is bound to.
    let config = CoderHooksConfig {
        chat_hook_secret: Some(SECRET.to_string()),
        chat_hook_timeout_ms: 1500,
        chat_hook_enabled: true,
        chat_hook_allow_insecure: true,
        hook_addr: addr.to_string(),
        hook_host: "127.0.0.1".to_string(),
        chat_hook_url: None,
        hook_logs: false,
    };
    assert_eq!(config.hook_public_url().as_deref(), Some(hook_url.as_str()));

    let store = Arc::new(SharedStore::new_in_memory());
    let router = create_router(store, config, None, None);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let (status, body) =
        dispatch_simulated_event(&hook_url, SECRET, "session_start", CHAT, "dis-abc")
            .await
            .expect("dispatch should succeed");
    assert_eq!(
        status, 200,
        "consumer should accept a valid signed dispatch: {body}"
    );
}

#[tokio::test]
async fn consumer_accepts_dispatch_when_chat_hook_url_is_audience() {
    // Regression for the InvalidAudience drift: when CODER_CHAT_HOOK_URL is the
    // configured audience, the consumer must validate against it even if it
    // differs from the address derived from the bind host/port. This mirrors a
    // topology where Coder POSTs to a hook URL that is not the loopback-derived
    // value (e.g. ${OPENFLOWS_HOOK_PORT} vs {hook_addr} port).
    use super::server::create_router;
    use super::simulate::dispatch_simulated_event;
    use config::env::CoderHooksConfig;
    use pocketflow_core::SharedStore;
    use std::sync::Arc;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let coder_hook_url = format!("http://127.0.0.1:{}/experimental/hooks/chat", addr.port());

    // The bind-derived address uses a *different* port than the listener. If the
    // consumer wrongly used the derived value as its `aud` it would reject the
    // token; because CODER_CHAT_HOOK_URL is authoritative, it accepts it.
    let config = CoderHooksConfig {
        chat_hook_secret: Some(SECRET.to_string()),
        chat_hook_timeout_ms: 1500,
        chat_hook_enabled: true,
        chat_hook_allow_insecure: true,
        hook_addr: format!("127.0.0.1:{}", addr.port() + 1),
        hook_host: "127.0.0.1".to_string(),
        // The authoritative audience is Coder's CODER_CHAT_HOOK_URL.
        chat_hook_url: Some(coder_hook_url.clone()),
        hook_logs: false,
    };
    assert_eq!(
        config.hook_public_url().as_deref(),
        Some(coder_hook_url.as_str())
    );

    let store = Arc::new(SharedStore::new_in_memory());
    let router = create_router(store, config, None, None);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Sign with Coder's configured URL as the aud and POST to the reachable
    // listener; the consumer must accept it even though its bind-derived URL
    // would have been a different port.
    let (status, body) =
        dispatch_simulated_event(&coder_hook_url, SECRET, "session_start", CHAT, "dis-aud-1")
            .await
            .expect("dispatch should succeed");
    assert_eq!(
        status, 200,
        "consumer should accept a dispatch signed with the authoritative \
         CODER_CHAT_HOOK_URL audience: {body}"
    );
}
