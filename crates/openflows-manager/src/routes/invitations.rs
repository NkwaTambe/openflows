//! Invitation acceptance routes.
//!
//! The invitation URL is the delivery mechanism, but possession of the URL
//! alone grants no membership: acceptance requires the authenticated user's
//! GitHub identity to match the invitee recorded on the invitation. The page is
//! rendered by GET; acceptance happens only via POST with a CSRF token.

use crate::auth::csrf;
use crate::error::ManagerError;
use crate::rate_limit::LimitScope;
use crate::routes::organizations::{authenticated_user, require_csrf_for_browser};
use crate::server::AppState;
use axum::extract::{Form, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AcceptQuery {
    pub token: Option<String>,
}

/// `GET /invitations/accept?token=...` — render the acceptance page. Does not
/// accept anything.
pub async fn accept_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AcceptQuery>,
) -> Result<Response, ManagerError> {
    let services = state
        .services()
        .ok_or_else(|| ManagerError::api("SERVICE_UNAVAILABLE", "service unavailable"))?;
    let Some(raw_token) = q.token.clone() else {
        return Err(ManagerError::not_found("invitation"));
    };

    // Must be signed in as a browser user.
    let principal = authenticated_user(&headers, &services.session_manager).await?;
    if !principal.is_browser {
        return Err(ManagerError::api(
            "UNAUTHORIZED",
            "open the invitation link in your browser while signed in",
        ));
    }

    // Resolve the invitation's organization display name for the page.
    let token_hash = crate::auth::crypto::hash_token(&raw_token);
    let inv = services.org_repo.invitation_by_token(&token_hash).await?;
    let Some(inv) = inv else {
        return Err(ManagerError::not_found("invitation"));
    };
    let display: Option<(String,)> =
        sqlx::query_as("SELECT display_name FROM organizations WHERE id = $1")
            .bind(inv.organization_id.0)
            .fetch_one(&services.orgs_service.pool)
            .await
            .ok();

    // Issue a CSRF cookie and render the accept form.
    let csrf_token = csrf::new_token();
    let html = crate::pages::invitation_page(
        &display
            .map(|(d,)| d)
            .unwrap_or_else(|| "the organization".to_string()),
        &inv.role.to_string(),
        &csrf_token,
    );
    let mut response = axum::response::Html(html).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!(
            "{}={csrf_token}; Path=/; HttpOnly; SameSite=Lax",
            csrf::CSRF_COOKIE
        )
        .parse()
        .map_err(|_| ManagerError::Service(anyhow::anyhow!("invalid cookie")))?,
    );
    Ok(response)
}

#[derive(Deserialize)]
pub struct AcceptForm {
    pub token: Option<String>,
    pub _csrf: String,
}

/// `POST /invitations/accept` — accept an invitation (single-use).
pub async fn accept(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AcceptForm>,
) -> Result<Response, ManagerError> {
    let services = state
        .services()
        .ok_or_else(|| ManagerError::api("SERVICE_UNAVAILABLE", "service unavailable"))?;
    let principal = authenticated_user(&headers, &services.session_manager).await?;
    if !principal.is_browser {
        return Err(ManagerError::api(
            "UNAUTHORIZED",
            "accept the invitation in your browser while signed in",
        ));
    }
    require_csrf_for_browser(&principal, &headers).await?;

    let Some(raw_token) = form.token.clone() else {
        return Err(ManagerError::not_found("invitation"));
    };

    let bucket = client_ip(&headers);
    if !services
        .rate_limiter
        .allow(&bucket, LimitScope::Invitation, 30)
        .await?
    {
        return Err(ManagerError::api("RATE_LIMITED", "too many attempts").retryable(true));
    }

    // Resolve the caller's GitHub identity to verify the invitee.
    let github_user_id = services
        .users
        .github_subject(principal.user_id)
        .await?
        .ok_or_else(|| ManagerError::api("UNAUTHORIZED", "no GitHub identity on this account"))?;

    services
        .orgs_service
        .accept_invitation(
            principal.user_id,
            github_user_id,
            &raw_token,
            &request_id(&headers),
        )
        .await?;

    let (_, html) = crate::pages::device_result_page(
        "Invitation accepted. You can close this window.",
        StatusCode::OK.as_u16(),
    );
    Ok(axum::response::Html(html).into_response())
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
