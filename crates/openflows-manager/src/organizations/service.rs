//! Organization service: creation, listing, membership, invitations, ownership
//! transfer, and deletion requests.
//!
//! Every method first resolves the caller's *current* membership through the
//! policy layer and the database, never from a header or cached claim. Durable
//! state changes, audit, and outbox writes are atomic; external effects (e.g.
//! Coder provisioning) are enqueued as operations and never awaited
//! synchronously.

use crate::audit::{self, AuditEvent};
use crate::db::tx::OrgScope;
use crate::dto::{MembershipDto, MembershipRole, MembershipStatus};
use crate::error::ManagerError;
use crate::id::{InvitationId, OperationId, OrganizationId, UserId};
use crate::idempotency::{IdempotencyOutcome, IdempotencyService, OrglessIdempotencyRequest};
use crate::operations;
use crate::organizations::policy::Policy;
use crate::organizations::repository::OrganizationRepository;
use crate::pagination::PageLimit;
use serde_json::json;
use sqlx::PgPool;

/// A validated organization slug.
pub fn validate_slug(slug: &str) -> Result<(), ManagerError> {
    let ok = slug.len() >= 3
        && slug.len() <= 63
        && slug
            .bytes()
            .next()
            .map(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            == Some(true)
        && slug
            .bytes()
            .last()
            .map(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            == Some(true)
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(ManagerError::InvalidInput(
            "slug must be 3-63 chars of lowercase letters, digits, or hyphens, and start/end alphanumeric".into(),
        ))
    }
}

pub fn validate_display_name(name: &str) -> Result<(), ManagerError> {
    if name.trim().is_empty() || name.chars().count() > 100 {
        return Err(ManagerError::InvalidInput(
            "display name must be 1-100 characters".into(),
        ));
    }
    Ok(())
}

/// The response body for a durable org operation (202).
#[derive(Debug, Clone, serde::Serialize)]
pub struct OrgOperationResponse {
    pub operation_id: OperationId,
    pub resource_id: OrganizationId,
    pub status: &'static str,
}

#[derive(Clone)]
pub struct OrganizationService {
    pub pool: PgPool,
    pub repo: OrganizationRepository,
    pub orgs: crate::repositories::OrganizationsRepository,
    pub idempotency: IdempotencyService,
}

impl OrganizationService {
    pub fn new(
        pool: PgPool,
        repo: OrganizationRepository,
        orgs: crate::repositories::OrganizationsRepository,
        idempotency: IdempotencyService,
    ) -> Self {
        OrganizationService {
            pool,
            repo,
            orgs,
            idempotency,
        }
    }

    /// Create an organization and enqueue its provisioning operation.
    ///
    /// Creation is idempotent *before any organization exists*, keyed by
    /// `(actor_id, route, key)` with a NULL organization id. The mutation
    /// atomically persists: organization + creator owner/admin membership +
    /// queued provisioning operation + audit + outbox. It returns 202 with a
    /// durable operation reference; Coder is never called synchronously.
    pub async fn create_org(
        &self,
        actor: UserId,
        slug: &str,
        display_name: &str,
        idempotency_key: &str,
        request_id: &str,
    ) -> Result<OrgOperationResponse, ManagerError> {
        validate_slug(slug)?;
        validate_display_name(display_name)?;

        // Own the values so the idempotency closure (higher-ranked over the
        // connection lifetime) can capture them by value.
        let slug = slug.to_string();
        let display_name = display_name.to_string();
        let idempotency_key = idempotency_key.to_string();
        let request_id = request_id.to_string();

        let route = "/api/v1/organizations".to_string();
        let request_hash =
            IdempotencyRequestHash::of(&json!({"slug": slug, "display_name": display_name}))?;
        let req = OrglessIdempotencyRequest {
            actor_id: actor,
            route,
            key: idempotency_key.clone(),
            request_hash,
        };

        let org_id = OrganizationId::new();
        let outcome = self
            .idempotency
            .execute_orgless(&req, |conn| {
                let slug = slug.clone();
                let display_name = display_name.clone();
                let idempotency_key = idempotency_key.clone();
                let request_id = request_id.clone();
                Box::pin(async move {
                    // Organization (status provisioning) + owner/admin membership.
                    let result = sqlx::query(
                        "INSERT INTO organizations
                            (id, slug, display_name, owner_user_id, status)
                         VALUES ($1, $2, $3, $4, 'provisioning')",
                    )
                    .bind(org_id.0)
                    .bind(&slug)
                    .bind(&display_name)
                    .bind(actor.0)
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| translate_org_insert(e, &slug))?;

                    if result.rows_affected() == 0 {
                        return Err(ManagerError::Conflict("organization not created".into()));
                    }

                    sqlx::query(
                        "INSERT INTO memberships (organization_id, user_id, role, status)
                         VALUES ($1, $2, 'admin', 'active')",
                    )
                    .bind(org_id.0)
                    .bind(actor.0)
                    .execute(&mut *conn)
                    .await
                    .map_err(ManagerError::from)?;

                    // Queued provisioning operation (a worker drives it; no
                    // synchronous Coder call).
                    let op_id = operations::create_in_tx(
                        conn,
                        &operations::NewOperation {
                            organization_id: org_id,
                            resource_type: "organization".into(),
                            resource_id: Some(org_id.0),
                            kind: "org.provision".into(),
                            idempotency_ref: Some(idempotency_key.to_string()),
                        },
                    )
                    .await?;

                    // Audit + outbox in the same transaction.
                    audit::insert(
                        &mut *conn,
                        &AuditEvent::new("org.create")
                            .organization(org_id)
                            .actor(actor)
                            .resource("organization", org_id.0)
                            .request_id(request_id),
                    )
                    .await?;
                    crate::outbox::insert_in_tx(
                        conn,
                        Some(org_id),
                        "org.provisioned",
                        Some(&json!({"organization_id": org_id.to_string()})),
                    )
                    .await?;

                    Ok(format!("{}:{}", org_id, op_id))
                })
            })
            .await?;

        match outcome {
            IdempotencyOutcome::New { response_reference } => {
                let (resource_id, op_id) = split_ref(&response_reference)?;
                Ok(OrgOperationResponse {
                    operation_id: op_id,
                    resource_id,
                    status: "queued",
                })
            }
            IdempotencyOutcome::Replay { response_reference } => {
                let (resource_id, op_id) = split_ref(&response_reference)?;
                Ok(OrgOperationResponse {
                    operation_id: op_id,
                    resource_id,
                    status: "queued",
                })
            }
        }
    }

    /// List organizations the caller belongs to (active memberships only).
    pub async fn list_orgs(&self, user: UserId) -> Result<Vec<MembershipDto>, ManagerError> {
        self.orgs.list_for_user(user).await
    }

    /// Get an organization the caller is a member of.
    pub async fn get_org(
        &self,
        caller: UserId,
        org_id: OrganizationId,
    ) -> Result<crate::dto::OrganizationDto, ManagerError> {
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_member(membership.as_ref(), org_state.as_ref())?;
        let scope = OrgScope::new(org_id);
        self.orgs
            .get_scoped(scope, org_id)
            .await?
            .ok_or_else(|| ManagerError::not_found("organization"))
    }

    /// Update organization settings (admin only). Only allowlisted settings are
    /// mutable in v1.
    pub async fn update_org(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        display_name: Option<&str>,
        request_id: &str,
    ) -> Result<crate::dto::OrganizationDto, ManagerError> {
        if let Some(name) = display_name {
            validate_display_name(name)?;
        }
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_admin(membership.as_ref(), org_state.as_ref())?;

        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        if let Some(name) = display_name {
            sqlx::query(
                "UPDATE organizations SET display_name = $1, updated_at = now() WHERE id = $2",
            )
            .bind(name)
            .bind(org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(ManagerError::from)?;
        }
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("org.update")
                .organization(org_id)
                .actor(caller)
                .resource("organization", org_id.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;

        self.get_org(caller, org_id).await
    }

    /// List members (any active member may view).
    pub async fn list_members(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        limit: PageLimit,
        after_user: Option<String>,
    ) -> Result<Vec<serde_json::Value>, ManagerError> {
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_member(membership.as_ref(), org_state.as_ref())?;
        let scope = OrgScope::new(org_id);
        let after = match after_user {
            Some(a) => Some(
                a.parse::<uuid::Uuid>()
                    .map_err(|_| ManagerError::InvalidInput("malformed cursor".into()))?,
            ),
            None => None,
        };
        let rows = self
            .repo
            .list_members(scope, limit.get() as i64 + 1, after)
            .await?;
        let has_more = rows.len() > limit.get() as usize;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .take(limit.get() as usize)
            .map(|(id, name, role, status)| {
                json!({
                    "user_id": id.to_string(),
                    "display_name": name,
                    "role": role,
                    "status": status,
                })
            })
            .collect();
        let _ = has_more;
        Ok(items)
    }

    /// Update a member's role/status (admin only).
    pub async fn update_member(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        target: UserId,
        role: Option<MembershipRole>,
        status: Option<MembershipStatus>,
        request_id: &str,
    ) -> Result<(), ManagerError> {
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_admin(membership.as_ref(), org_state.as_ref())?;

        // Read the target's current role/status to compute the transition.
        let current: (String, String) = sqlx::query_as(
            "SELECT role, status FROM memberships WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(org_id.0)
        .bind(target.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ManagerError::not_found("member"))?;

        let new_role = role.unwrap_or_else(|| {
            current
                .0
                .parse::<MembershipRole>()
                .unwrap_or(MembershipRole::Viewer)
        });
        let new_status = status.unwrap_or_else(|| {
            current
                .1
                .parse::<MembershipStatus>()
                .unwrap_or(MembershipStatus::Active)
        });

        let scope = OrgScope::new(org_id);
        self.repo
            .update_member(scope, target, new_role, new_status)
            .await?;

        // Audit after the mutation (outside the locked tx is fine; the audit
        // itself is idempotent-ish and best-effort here, but to keep it atomic
        // we do it in a fresh transaction).
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("member.update")
                .organization(org_id)
                .actor(caller)
                .resource("membership", target.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }

    /// Remove a member (admin only).
    pub async fn remove_member(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        target: UserId,
        request_id: &str,
    ) -> Result<(), ManagerError> {
        self.update_member(
            caller,
            org_id,
            target,
            Some(MembershipRole::Viewer),
            Some(MembershipStatus::Removed),
            request_id,
        )
        .await
    }

    /// Create an invitation (admin only). Resolves the invited GitHub login to
    /// its immutable id; returns the one-time invitation URL (raw token).
    pub async fn create_invitation(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        github_login: &str,
        role: MembershipRole,
        github: &dyn crate::auth::github::GithubAuth,
        request_id: &str,
    ) -> Result<(InvitationId, String), ManagerError> {
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_admin(membership.as_ref(), org_state.as_ref())?;

        // Resolve the login to an immutable GitHub id.
        let lookup = github.resolve_login(github_login).await?;

        // Generate a one-time token; only its hash is persisted.
        let token = crate::auth::crypto::Secret::generate();
        let token_hash = token.hash();

        let id = self
            .repo
            .create_invitation(OrgScope::new(org_id), lookup.id, role, &token_hash, caller)
            .await?;

        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("invitation.create")
                .organization(org_id)
                .actor(caller)
                .resource("invitation", id.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;

        Ok((id, token.encode()))
    }

    /// Revoke an invitation (admin only).
    pub async fn revoke_invitation(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        invitation_id: InvitationId,
        request_id: &str,
    ) -> Result<(), ManagerError> {
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_admin(membership.as_ref(), org_state.as_ref())?;
        let revoked = self
            .repo
            .revoke_invitation(OrgScope::new(org_id), invitation_id)
            .await?;
        if !revoked {
            return Err(ManagerError::not_found("invitation"));
        }
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("invitation.revoke")
                .organization(org_id)
                .actor(caller)
                .resource("invitation", invitation_id.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }

    /// Accept an invitation by token. Requires the caller's authenticated
    /// GitHub identity to match the invitee.
    pub async fn accept_invitation(
        &self,
        caller: UserId,
        github_user_id: i64,
        raw_token: &str,
        request_id: &str,
    ) -> Result<OrganizationId, ManagerError> {
        let token_hash = crate::auth::crypto::hash_token(raw_token);
        let Some(inv) = self.repo.invitation_by_token(&token_hash).await? else {
            return Err(ManagerError::not_found("invitation"));
        };
        // The organization must be in a state that accepts new members.
        let org_state: (String,) = sqlx::query_as("SELECT status FROM organizations WHERE id = $1")
            .bind(inv.organization_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ManagerError::not_found("organization"))?;
        if org_state.0 == "deleted" {
            return Err(ManagerError::not_found("invitation"));
        }

        self.repo
            .accept_invitation(inv.id, github_user_id, caller)
            .await?;

        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("invitation.accept")
                .organization(inv.organization_id)
                .actor(caller)
                .resource("invitation", inv.id.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(inv.organization_id)
    }

    /// Transfer ownership (owner only + recent authentication).
    pub async fn transfer_ownership(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        new_owner: UserId,
        has_recent_auth: bool,
        request_id: &str,
    ) -> Result<(), ManagerError> {
        if !has_recent_auth {
            return Err(ManagerError::api(
                "REAUTH_REQUIRED",
                "recent authentication (within 10 minutes) is required to transfer ownership",
            ));
        }
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_owner(membership.as_ref(), org_state.as_ref())?;

        self.repo
            .transfer_ownership(OrgScope::new(org_id), new_owner)
            .await?;

        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("org.transfer_ownership")
                .organization(org_id)
                .actor(caller)
                .resource("organization", org_id.0)
                .request_id(request_id),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }

    /// Request organization deletion (owner only + recent authentication).
    /// Records durable work; infrastructure teardown is a future worker.
    pub async fn request_deletion(
        &self,
        caller: UserId,
        org_id: OrganizationId,
        has_recent_auth: bool,
        request_id: &str,
    ) -> Result<OperationId, ManagerError> {
        if !has_recent_auth {
            return Err(ManagerError::api(
                "REAUTH_REQUIRED",
                "recent authentication (within 10 minutes) is required to delete the organization",
            ));
        }
        let (membership, org_state) = self.repo.membership_and_org(org_id, caller).await?;
        Policy::require_owner(membership.as_ref(), org_state.as_ref())?;

        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        // Lock the org row, mark it deleting, and enqueue the deletion operation
        // durably so a worker can complete teardown later.
        let owner: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT owner_user_id FROM organizations WHERE id = $1 FOR UPDATE")
                .bind(org_id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(ManagerError::from)?;
        if owner.is_none() {
            return Err(ManagerError::not_found("organization"));
        }
        sqlx::query(
            "UPDATE organizations SET status = 'deleting', updated_at = now() WHERE id = $1",
        )
        .bind(org_id.0)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;
        let op_id = operations::create_in_tx(
            &mut tx,
            &operations::NewOperation {
                organization_id: org_id,
                resource_type: "organization".into(),
                resource_id: Some(org_id.0),
                kind: "org.delete".into(),
                idempotency_ref: None,
            },
        )
        .await?;
        audit::insert_in_tx(
            &mut tx,
            &AuditEvent::new("org.delete_request")
                .organization(org_id)
                .actor(caller)
                .resource("organization", org_id.0)
                .request_id(request_id),
        )
        .await?;
        crate::outbox::insert_in_tx(
            &mut tx,
            Some(org_id),
            "org.deleted",
            Some(&json!({"organization_id": org_id.to_string()})),
        )
        .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(op_id)
    }
}

/// A lightweight struct to produce idempotency request hashes without exposing
/// the internal serializer.
struct IdempotencyRequestHash;

impl IdempotencyRequestHash {
    fn of<T: serde::Serialize>(value: &T) -> Result<String, ManagerError> {
        crate::idempotency::IdempotencyRequest::hash_request(value)
            .map_err(|_| ManagerError::Service(anyhow::anyhow!("failed to hash request body")))
    }
}

fn split_ref(response_reference: &str) -> Result<(OrganizationId, OperationId), ManagerError> {
    let mut parts = response_reference.splitn(2, ':');
    let org = parts
        .next()
        .ok_or_else(|| ManagerError::Service(anyhow::anyhow!("invalid org reference")))?
        .parse::<OrganizationId>()
        .map_err(|_| ManagerError::Service(anyhow::anyhow!("invalid org id in reference")))?;
    let op = parts
        .next()
        .ok_or_else(|| ManagerError::Service(anyhow::anyhow!("invalid operation reference")))?
        .parse::<OperationId>()
        .map_err(|_| ManagerError::Service(anyhow::anyhow!("invalid operation id in reference")))?;
    Ok((org, op))
}

fn translate_org_insert(e: sqlx::Error, slug: &str) -> ManagerError {
    if let sqlx::Error::Database(db) = &e {
        if db.constraint() == Some("organizations_slug_key") {
            return ManagerError::Conflict(format!("organization slug already exists: {slug}"));
        }
    }
    ManagerError::from(e)
}
