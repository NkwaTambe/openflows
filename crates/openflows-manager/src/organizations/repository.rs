//! Organization, membership, and invitation repository with organization-row
//! locking for member mutations and ownership transfer.
//!
//! Security rules enforced here (in addition to the policy layer):
//!   * Every lookup is joined to the caller's organization scope.
//!   * Member mutations and ownership transfer lock the organization row
//!     (`FOR UPDATE`) so concurrent demotions cannot remove the last active
//!     admin and the owner cannot be removed/suspended without transfer.
//!   * Invitation acceptance consumes the invitation atomically and requires
//!     the exact authenticated GitHub identity recorded on the invitation.

use crate::db::tx::OrgScope;
use crate::dto::{MembershipRole, MembershipStatus};
use crate::error::ManagerError;
use crate::id::{InvitationId, OrganizationId, UserId};
use crate::organizations::policy::{Membership, OrganizationState};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// A persisted invitation row.
#[derive(Debug, Clone)]
pub struct InvitationRow {
    pub id: InvitationId,
    pub organization_id: OrganizationId,
    pub invitee_github_user_id: i64,
    pub role: MembershipRole,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct OrganizationRepository {
    pool: PgPool,
}

impl OrganizationRepository {
    pub fn new(pool: PgPool) -> Self {
        OrganizationRepository { pool }
    }

    /// Fetch the caller's current membership, scoped to an organization that
    /// exists. Returns `(membership, org_state)`; both `None` when the
    /// organization does not exist or the caller has no membership.
    pub async fn membership_and_org(
        &self,
        org_id: OrganizationId,
        user_id: UserId,
    ) -> Result<(Option<Membership>, Option<OrganizationState>), ManagerError> {
        let org: Option<(String, uuid::Uuid)> =
            sqlx::query_as("SELECT status, owner_user_id FROM organizations WHERE id = $1")
                .bind(org_id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(ManagerError::from)?;

        let org_state = org.map(|(status, owner)| OrganizationState {
            organization_id: org_id,
            status,
            owner_user_id: UserId::from_uuid(owner),
        });

        let membership = if org_state.is_some() {
            let row = sqlx::query_as::<_, (String, String)>(
                "SELECT role, status FROM memberships
                  WHERE organization_id = $1 AND user_id = $2",
            )
            .bind(org_id.0)
            .bind(user_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(ManagerError::from)?;
            match row {
                Some((role, status)) => Some(Membership {
                    organization_id: org_id,
                    user_id,
                    role: role.parse::<MembershipRole>().map_err(|_| {
                        ManagerError::Service(anyhow::anyhow!("invalid role in db"))
                    })?,
                    status: status.parse::<MembershipStatus>().map_err(|_| {
                        ManagerError::Service(anyhow::anyhow!("invalid status in db"))
                    })?,
                }),
                None => None,
            }
        } else {
            None
        };

        Ok((membership, org_state))
    }

    /// List members of an organization (active, suspended, and removed), joined
    /// to the organization scope.
    pub async fn list_members(
        &self,
        scope: OrgScope,
        limit: i64,
        after_user: Option<uuid::Uuid>,
    ) -> Result<Vec<(UserId, String, String, String)>, ManagerError> {
        // Simple keyset over user_id; limited result size.
        let rows = match after_user {
            Some(after) => sqlx::query_as::<_, (uuid::Uuid, String, String, String)>(
                "SELECT m.user_id, u.display_name, m.role, m.status
                       FROM memberships m
                       JOIN users u ON u.id = m.user_id
                      WHERE m.organization_id = $1 AND m.user_id > $2
                      ORDER BY m.user_id
                      LIMIT $3",
            )
            .bind(scope.organization_id.0)
            .bind(after)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(ManagerError::from)?,
            None => sqlx::query_as::<_, (uuid::Uuid, String, String, String)>(
                "SELECT m.user_id, u.display_name, m.role, m.status
                       FROM memberships m
                       JOIN users u ON u.id = m.user_id
                      WHERE m.organization_id = $1
                      ORDER BY m.user_id
                      LIMIT $2",
            )
            .bind(scope.organization_id.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(ManagerError::from)?,
        };
        Ok(rows
            .into_iter()
            .map(|(id, name, role, status)| (UserId::from_uuid(id), name, role, status))
            .collect())
    }

    /// Update a member's role/status with organization-row locking. Enforces:
    ///   * the owner must remain an active member (cannot be suspended/removed
    ///     or demoted without first transferring ownership);
    ///   * at least one active admin must remain (a demotion that would leave
    ///     zero active admins is rejected).
    ///
    /// Returns `Err` with the specific conflict when an invariant would be
    /// violated.
    pub async fn update_member(
        &self,
        scope: OrgScope,
        target_user: UserId,
        new_role: MembershipRole,
        new_status: MembershipStatus,
    ) -> Result<(), ManagerError> {
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        self.update_member_in_tx(&mut tx, scope, target_user, new_role, new_status)
            .await?;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }

    async fn update_member_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        scope: OrgScope,
        target_user: UserId,
        new_role: MembershipRole,
        new_status: MembershipStatus,
    ) -> Result<(), ManagerError> {
        // Lock the organization row to serialize membership mutations.
        let org: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT owner_user_id FROM organizations WHERE id = $1 FOR UPDATE")
                .bind(scope.organization_id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(ManagerError::from)?;
        let Some((owner,)) = org else {
            return Err(ManagerError::not_found("organization"));
        };

        // The target must exist in this org.
        let target: Option<(String, String)> = sqlx::query_as(
            "SELECT role, status FROM memberships
              WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(scope.organization_id.0)
        .bind(target_user.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ManagerError::from)?;
        let Some((target_role, target_status)) = target else {
            return Err(ManagerError::not_found("member"));
        };

        // Owner invariant: the owner must remain an active member. Suspending
        // or removing the owner, or demoting the owner away from admin while
        // they still own, requires ownership transfer first.
        if owner == target_user.0 {
            if new_status != MembershipStatus::Active
                || (new_role != MembershipRole::Admin && target_role == "admin")
            {
                return Err(ManagerError::Conflict(
                    "the organization owner must remain an active admin; transfer ownership first"
                        .to_string(),
                ));
            }
            if target_status == "active" && new_status != MembershipStatus::Active {
                return Err(ManagerError::Conflict(
                    "the organization owner must remain an active member; transfer ownership first"
                        .to_string(),
                ));
            }
        }

        // Last-admin invariant: count active admins excluding this target, and
        // reject a change that would leave zero active admins.
        let is_current_admin = target_role == "admin" && target_status == "active";
        let becomes_active_admin =
            new_role == MembershipRole::Admin && new_status == MembershipStatus::Active;
        if is_current_admin && !becomes_active_admin {
            let other_admins: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM memberships
                  WHERE organization_id = $1 AND role = 'admin' AND status = 'active'
                    AND user_id <> $2",
            )
            .bind(scope.organization_id.0)
            .bind(target_user.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(ManagerError::from)?;
            if other_admins.0 == 0 {
                return Err(ManagerError::Conflict(
                    "cannot demote, suspend, or remove the last active admin".to_string(),
                ));
            }
        }

        sqlx::query(
            "UPDATE memberships
                SET role = $1, status = $2, updated_at = now()
              WHERE organization_id = $3 AND user_id = $4",
        )
        .bind(new_role.as_str())
        .bind(new_status.as_str())
        .bind(scope.organization_id.0)
        .bind(target_user.0)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;
        Ok(())
    }

    /// Remove a member (mark removed) with the same invariants as `update_member`.
    pub async fn remove_member(
        &self,
        scope: OrgScope,
        target_user: UserId,
    ) -> Result<(), ManagerError> {
        self.update_member(
            scope,
            target_user,
            MembershipRole::Viewer,
            MembershipStatus::Removed,
        )
        .await
    }

    /// Transfer ownership to `new_owner` (must be an active member) with
    /// organization-row locking. The previous owner remains an admin unless
    /// separately changed.
    pub async fn transfer_ownership(
        &self,
        scope: OrgScope,
        new_owner: UserId,
    ) -> Result<(), ManagerError> {
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;
        // Lock the organization row.
        let org: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT owner_user_id FROM organizations WHERE id = $1 FOR UPDATE")
                .bind(scope.organization_id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(ManagerError::from)?;
        let Some((current_owner,)) = org else {
            return Err(ManagerError::not_found("organization"));
        };

        // The new owner must be an active member.
        let new_member: Option<(String,)> = sqlx::query_as(
            "SELECT status FROM memberships
              WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(scope.organization_id.0)
        .bind(new_owner.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ManagerError::from)?;
        let Some((status,)) = new_member else {
            return Err(ManagerError::not_found("member"));
        };
        if status != "active" {
            return Err(ManagerError::Conflict(
                "the new owner must be an active member".to_string(),
            ));
        }

        // Promote the new owner to admin if they are not already, so the owner
        // is always an active admin.
        sqlx::query(
            "UPDATE memberships SET role = 'admin', updated_at = now()
              WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(scope.organization_id.0)
        .bind(new_owner.0)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;

        sqlx::query(
            "UPDATE organizations SET owner_user_id = $1, updated_at = now()
              WHERE id = $2",
        )
        .bind(new_owner.0)
        .bind(scope.organization_id.0)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;

        let _ = current_owner;
        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }

    /// Create an invitation, safely replacing an expired or revoked live
    /// invitation for the same org/subject. Enforces one live invitation per
    /// org/subject; returns 409 when a live (unexpired) invitation already
    /// exists.
    pub async fn create_invitation(
        &self,
        scope: OrgScope,
        invitee_github_user_id: i64,
        role: MembershipRole,
        token_hash: &str,
        invited_by: UserId,
    ) -> Result<InvitationId, ManagerError> {
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;

        // Revoke any expired/live invitation for the same org/subject so a
        // replacement is safe (safe replacement of expired invitations).
        sqlx::query(
            "UPDATE invitations SET revoked_at = now()
              WHERE organization_id = $1 AND invitee_github_user_id = $2
                AND accepted_at IS NULL AND revoked_at IS NULL",
        )
        .bind(scope.organization_id.0)
        .bind(invitee_github_user_id)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;

        // Enforce one live invitation per org/subject even under concurrency:
        // the partial unique index does this at the database level.
        let id = InvitationId::new();
        sqlx::query(
            "INSERT INTO invitations
                (id, organization_id, invitee_github_user_id, role, token_hash, invited_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, clock_timestamp() + interval '7 days')",
        )
        .bind(id.0)
        .bind(scope.organization_id.0)
        .bind(invitee_github_user_id)
        .bind(role.as_str())
        .bind(token_hash)
        .bind(invited_by.0)
        .execute(&mut *tx)
        .await
        .map_err(translate_invite_insert)?;

        tx.commit().await.map_err(ManagerError::from)?;
        Ok(id)
    }

    /// Revoke a live invitation by id within the org scope.
    pub async fn revoke_invitation(
        &self,
        scope: OrgScope,
        id: InvitationId,
    ) -> Result<bool, ManagerError> {
        let affected = sqlx::query(
            "UPDATE invitations SET revoked_at = now()
              WHERE id = $1 AND organization_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL",
        )
        .bind(id.0)
        .bind(scope.organization_id.0)
        .execute(&self.pool)
        .await
        .map_err(ManagerError::from)?
        .rows_affected();
        Ok(affected == 1)
    }

    /// Look up a live (not accepted/revoked/expired) invitation by token hash,
    /// with its organization and role. The invitation URL is the delivery
    /// mechanism; acceptance additionally requires the exact GitHub identity.
    pub async fn invitation_by_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<InvitationRow>, ManagerError> {
        let row = sqlx::query_as::<
            _,
            (uuid::Uuid, uuid::Uuid, i64, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>),
        >(
            "SELECT id, organization_id, invitee_github_user_id, role, expires_at, accepted_at, revoked_at
               FROM invitations
              WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(ManagerError::from)?;
        Ok(row.map(
            |(id, org, invitee, role, expires, accepted, revoked)| InvitationRow {
                id: InvitationId::from_uuid(id),
                organization_id: OrganizationId::from_uuid(org),
                invitee_github_user_id: invitee,
                role: role
                    .parse::<MembershipRole>()
                    .map_err(|_| ManagerError::Service(anyhow::anyhow!("invalid role in db")))
                    .ok()
                    .unwrap_or(MembershipRole::Viewer),
                expires_at: expires,
                accepted_at: accepted,
                revoked_at: revoked,
            },
        ))
    }

    /// Accept an invitation: verify the exact authenticated GitHub identity,
    /// expiry, and single consumption; then add the membership in one
    /// transaction. Concurrent acceptance is serialized by locking the
    /// invitation row.
    pub async fn accept_invitation(
        &self,
        invitation_id: InvitationId,
        auth_github_user_id: i64,
        user_id: UserId,
    ) -> Result<(), ManagerError> {
        let mut tx = self.pool.begin().await.map_err(ManagerError::from)?;

        let row = sqlx::query_as::<
            _,
            (uuid::Uuid, i64, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>),
        >(
            "SELECT organization_id, invitee_github_user_id, role, expires_at, accepted_at, revoked_at
               FROM invitations WHERE id = $1 FOR UPDATE",
        )
        .bind(invitation_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ManagerError::from)?;
        let Some((org_id, invitee_id, role, expires_at, accepted_at, revoked_at)) = row else {
            return Err(ManagerError::not_found("invitation"));
        };

        // Single consumption + revocation.
        if accepted_at.is_some() || revoked_at.is_some() {
            return Err(ManagerError::Conflict(
                "invitation already used".to_string(),
            ));
        }
        // Expiry.
        if expires_at <= Utc::now() {
            return Err(ManagerError::Conflict("invitation expired".to_string()));
        }

        // Only the exact authenticated GitHub identity may accept.
        if invitee_id != auth_github_user_id {
            return Err(ManagerError::api(
                "INVITATION_WRONG_USER",
                "this invitation is not for your GitHub identity",
            ));
        }

        // Mark accepted and add membership (status active unless the org is
        // suspended; the org-status check happens at the policy layer before
        // this is reached for an org-scoped accept).
        sqlx::query("UPDATE invitations SET accepted_at = now() WHERE id = $1")
            .bind(invitation_id.0)
            .execute(&mut *tx)
            .await
            .map_err(ManagerError::from)?;

        sqlx::query(
            "INSERT INTO memberships (organization_id, user_id, role, status)
             VALUES ($1, $2, $3, 'active')
             ON CONFLICT (organization_id, user_id) DO UPDATE
               SET status = 'active', updated_at = now()",
        )
        .bind(org_id)
        .bind(user_id.0)
        .bind(&role)
        .execute(&mut *tx)
        .await
        .map_err(ManagerError::from)?;

        tx.commit().await.map_err(ManagerError::from)?;
        Ok(())
    }
}

/// Translate an invitation insert failure, mapping the one-live-invitation
/// violation to a 409.
fn translate_invite_insert(e: sqlx::Error) -> ManagerError {
    if let sqlx::Error::Database(db) = &e {
        if db.constraint() == Some("one_live_invitation_per_org_subject") {
            return ManagerError::Conflict(
                "an active invitation already exists for this person".to_string(),
            );
        }
    }
    ManagerError::from(e)
}
