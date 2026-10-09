//! CSRF protection for browser mutations.
//!
//! Browser session cookies are `Secure; HttpOnly; SameSite=Lax`. `SameSite=Lax`
//! already prevents cross-site POST cookies from being sent, but the spec
//! requires explicit CSRF tokens on mutations as defense-in-depth. We use a
//! double-submit pattern: a per-request CSRF token is set as a cookie when a
//! page is rendered, and every state-changing browser request must echo the
//! same token in the `X-CSRF-Token` header. A cross-site attacker cannot read
//! the cookie (HttpOnly) nor rely on it being sent with a POST (SameSite=Lax),
//! so a matching header proves the request came from the authenticated origin.

use crate::auth::crypto::{ct_eq, Secret};
use crate::error::ManagerError;

/// The header a browser mutation must carry with the CSRF token.
pub const CSRF_HEADER: &str = "x-csrf-token";
/// The name of the CSRF cookie.
pub const CSRF_COOKIE: &str = "of_csrf";

/// A fresh CSRF token (the raw value is set in the cookie; the same value must
/// be echoed in the header).
pub fn new_token() -> String {
    Secret::generate().encode()
}

/// Validate a presented header token against the cookie token in constant time.
pub fn validate(presented: &str, cookie: &str) -> Result<(), ManagerError> {
    if presented.trim().is_empty() || cookie.trim().is_empty() {
        return Err(ManagerError::api("CSRF_FAILED", "missing CSRF token"));
    }
    if !ct_eq(presented.as_bytes(), cookie.as_bytes()) {
        return Err(ManagerError::api("CSRF_FAILED", "CSRF token mismatch"));
    }
    Ok(())
}
