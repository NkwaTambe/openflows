//! Redis key constants/helpers for A2A verification artifacts.
//!
//! These are *unnamespaced* keys — callers (currently
//! `openflows-harness::store::HarnessStore`) apply the existing
//! `ns:{tenant}:` prefix on top, consistent with `full_ticket_key` /
//! `full_ticket_key_flat` in `config::state`.

/// Key under which the terminal `VerifyResult` for a given pair's most
/// recent verification is mirrored, so Sentinel/Forge/humans can inspect
/// verification history without replaying A2A task state.
///
/// Note: a pair may run many verify tasks over its lifetime; this key holds
/// the latest one. Full history lives under `audit:a2a:{task_id}` keys,
/// enumerable via the nexus relay's task log (task 2 of the plan).
pub fn verification_key(pair_id: &str) -> String {
    format!("pair:{}:verification", pair_id)
}

/// Key prefix for a specific A2A task's audit trail (request, result,
/// stdout/stderr tails). Append `:stdout`, `:stderr`, `:request`, or
/// `:result` as needed.
pub fn audit_task_key(task_id: &str) -> String {
    format!("audit:a2a:{}", task_id)
}

/// Key for the append-only log of requests rejected by the nexus relay's
/// command allowlist / cwd validation (task 2).
pub fn audit_rejected_key() -> &'static str {
    "audit:a2a:rejected"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_key_is_pair_scoped() {
        assert_eq!(verification_key("T-048"), "pair:T-048:verification");
        assert_ne!(verification_key("T-048"), verification_key("T-049"));
    }

    #[test]
    fn audit_task_key_prefixes_task_id() {
        assert_eq!(audit_task_key("abc123"), "audit:a2a:abc123");
    }

    #[test]
    fn audit_rejected_key_is_fixed() {
        assert_eq!(audit_rejected_key(), "audit:a2a:rejected");
    }
}
