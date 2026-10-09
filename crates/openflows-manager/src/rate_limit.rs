//! Independent rate limiting for authentication entry points.
//!
//! Start, code verification (device approve), and polling are bounded
//! independently so an attacker cannot exhaust one endpoint to starve another.
//! Counters are windowed and stored in PostgreSQL so they survive restarts and
//! are shared across the process.

use crate::error::ManagerError;
use chrono::{Duration, Utc};
use sqlx::PgPool;

/// Rate-limit scopes, each bounded independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitScope {
    /// `POST /auth/github/start` and `POST /auth/cli/start`.
    Start,
    /// `POST /auth/cli/approve` (code verification).
    Verify,
    /// `POST /auth/cli/token` (polling).
    Poll,
    /// `POST /invitations/accept` and other token-verification entry points.
    Invitation,
}

impl LimitScope {
    fn as_str(&self) -> &'static str {
        match self {
            LimitScope::Start => "start",
            LimitScope::Verify => "verify",
            LimitScope::Poll => "poll",
            LimitScope::Invitation => "invitation",
        }
    }
}

#[derive(Clone)]
pub struct RateLimiter {
    pool: PgPool,
}

/// The window length for counting attempts.
const WINDOW: Duration = Duration::seconds(60);

impl RateLimiter {
    pub fn new(pool: PgPool) -> Self {
        RateLimiter { pool }
    }

    /// Check-and-increment an attempt for `bucket_key` in `scope`. Returns
    /// `true` when within `limit` (allowed), `false` when over (rate-limited).
    ///
    /// This is not a precise sliding window; a coarse fixed window is sufficient
    /// for abuse bounding and is cheap to maintain.
    pub async fn allow(
        &self,
        bucket_key: &str,
        scope: LimitScope,
        limit: u32,
    ) -> Result<bool, ManagerError> {
        let window_start = Utc::now() - WINDOW;
        let count: i64 = sqlx::query_scalar(
            "INSERT INTO rate_limit_ledger (bucket_key, scope, window_start, count)
             VALUES ($1, $2, $3, 1)
             ON CONFLICT (bucket_key, scope, window_start) DO UPDATE
               SET count = rate_limit_ledger.count + 1,
                   updated_at = now()
             RETURNING count",
        )
        .bind(bucket_key)
        .bind(scope.as_str())
        .bind(window_start)
        .fetch_one(&self.pool)
        .await
        .map_err(ManagerError::from)?;
        Ok(count <= limit as i64)
    }
}
