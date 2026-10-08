//! Idempotency-key handling for mutations and durable work creation.
//!
//! Per the shared API conventions, mutations that create resources or durable
//! work require an `Idempotency-Key`. A unique constraint on
//! `(actor_id, organization_id, route, key)` makes retries idempotent: a
//! repeated key with an identical body returns the original result reference;
//! the same key with a different body is rejected with a 409. Records are
//! retained at least 7 days and can be pruned after expiry.

use crate::error::ManagerError;
use crate::id::{OrganizationId, UserId};
use chrono::{Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Retention window for idempotency records.
pub const RETENTION: Duration = Duration::days(7);

/// The result of attempting to register an idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyOutcome {
    /// The key was newly created; the caller should perform the mutation.
    New,
    /// A prior identical request used this key; `response_reference` holds the
    /// original result the caller should return.
    Replay { response_reference: String },
}

/// A prepared idempotency-key record ready to be persisted atomically with the
/// mutation it guards.
#[derive(Debug, Clone)]
pub struct IdempotencyRequest {
    pub actor_id: UserId,
    pub organization_id: OrganizationId,
    pub route: String,
    pub key: String,
    pub request_hash: String,
}

impl IdempotencyRequest {
    /// Compute the SHA-256 request hash of the serialized request body.
    pub fn hash_request<T: Serialize>(body: &T) -> String {
        let json = serde_json::to_vec(body).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&json);
        format!("{:x}", hasher.finalize())
    }
}

/// Service for creating and reusing idempotency keys.
#[derive(Clone)]
pub struct IdempotencyService {
    pool: PgPool,
}

impl IdempotencyService {
    pub fn new(pool: PgPool) -> Self {
        IdempotencyService { pool }
    }

    /// Register an idempotency key for a request body.
    ///
    /// Returns `New` when the key is new, or `Replay` when an identical prior
    /// request used the same key. A reused key with a different request body is
    /// rejected with `Conflict`.
    pub async fn register(
        &self,
        req: &IdempotencyRequest,
        response_reference: Option<&str>,
    ) -> Result<IdempotencyOutcome, ManagerError> {
        // Expired rows still participate in the permanent uniqueness
        // constraint, so prune this key before attempting a new registration.
        // This makes the documented retention window meaningful while keeping
        // the operation safe under concurrent callers (the insert below still
        // arbitrates races).
        sqlx::query(
            "DELETE FROM idempotency_keys
             WHERE actor_id = $1 AND organization_id = $2 AND route = $3 AND key = $4
               AND expires_at <= now()",
        )
        .bind(req.actor_id.0)
        .bind(req.organization_id.0)
        .bind(&req.route)
        .bind(&req.key)
        .execute(&self.pool)
        .await
        .map_err(ManagerError::from)?;

        // Look for an existing, non-expired record first.
        let existing = sqlx::query_as::<_, (String, String)>(
            "SELECT request_hash, COALESCE(response_reference, '')
             FROM idempotency_keys
             WHERE actor_id = $1 AND organization_id = $2 AND route = $3 AND key = $4
               AND expires_at > now()",
        )
        .bind(req.actor_id.0)
        .bind(req.organization_id.0)
        .bind(&req.route)
        .bind(&req.key)
        .fetch_optional(&self.pool)
        .await
        .map_err(ManagerError::from)?;

        if let Some((stored_hash, stored_ref)) = existing {
            if stored_hash != req.request_hash {
                return Err(ManagerError::Conflict(
                    "idempotency key reused with a different request body".to_string(),
                ));
            }
            return Ok(IdempotencyOutcome::Replay {
                response_reference: stored_ref,
            });
        }

        // Insert a new record. A concurrent identical insert is handled by the
        // unique constraint; we re-check on conflict.
        let expires_at = Utc::now() + RETENTION;
        let result = sqlx::query(
            "INSERT INTO idempotency_keys
                (id, actor_id, organization_id, route, key, request_hash, response_reference, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (actor_id, organization_id, route, key) DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(req.actor_id.0)
        .bind(req.organization_id.0)
        .bind(&req.route)
        .bind(&req.key)
        .bind(&req.request_hash)
        .bind(response_reference)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(ManagerError::from)?;

        if result.rows_affected() == 1 {
            return Ok(IdempotencyOutcome::New);
        }

        // Lost a race: a concurrent insert won. Re-read and decide.
        let existing = sqlx::query_as::<_, (String, String)>(
            "SELECT request_hash, COALESCE(response_reference, '')
             FROM idempotency_keys
             WHERE actor_id = $1 AND organization_id = $2 AND route = $3 AND key = $4
               AND expires_at > now()",
        )
        .bind(req.actor_id.0)
        .bind(req.organization_id.0)
        .bind(&req.route)
        .bind(&req.key)
        .fetch_one(&self.pool)
        .await
        .map_err(ManagerError::from)?;

        if existing.0 != req.request_hash {
            return Err(ManagerError::Conflict(
                "idempotency key reused with a different request body".to_string(),
            ));
        }
        Ok(IdempotencyOutcome::Replay {
            response_reference: existing.1,
        })
    }

    /// Record the response reference for a newly-created key once the mutation
    /// it guarded has completed. This is a no-op when the reference is None.
    pub async fn record_response(
        &self,
        req: &IdempotencyRequest,
        response_reference: &str,
    ) -> Result<(), ManagerError> {
        sqlx::query(
            "UPDATE idempotency_keys
             SET response_reference = $1
             WHERE actor_id = $2 AND organization_id = $3 AND route = $4 AND key = $5",
        )
        .bind(response_reference)
        .bind(req.actor_id.0)
        .bind(req.organization_id.0)
        .bind(&req.route)
        .bind(&req.key)
        .execute(&self.pool)
        .await
        .map_err(ManagerError::from)?;
        Ok(())
    }
}
