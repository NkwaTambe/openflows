//! Shared A2A protocol types for delegated verification between SENTINEL and
//! FORGE (tracking issue: The-AgenticFlow/openflows#143).
//!
//! Scope (task 1 of `.kilo/plans/1785948146715-sentinel-forge-a2a-verify.md`):
//! serde types + Redis key helpers for the `verify` A2A task type, shared by
//! the nexus relay (crates/agent-nexus) and the harness CLI
//! (crates/openflows-harness). This crate defines the wire contract only; it
//! does not implement the relay, the executor sandboxing, or the harness
//! subcommands — those are later tasks in the plan.

mod keys;
mod verify;

pub use keys::{audit_rejected_key, audit_task_key, verification_key};
pub use verify::{
    ExecutorInfo, VerifyArtifact, VerifyExpect, VerifyKind, VerifyProgressEvent, VerifyRequest,
    VerifyResult,
};

/// The only `task_type` value this crate currently defines. Reserved so the
/// wire format can add other task types later without a breaking change to
/// this constant's meaning.
pub const TASK_TYPE_VERIFY: &str = "verify";

/// Command allowlist enforced by the nexus relay before dispatching a
/// `verify` request (task 2 of the plan). Kept here so both the relay and
/// any future client-side pre-validation share one definition.
pub const DEFAULT_COMMAND_ALLOWLIST: &[&[&str]] = &[
    &["cargo", "test"],
    &["cargo", "build"],
    &["cargo", "clippy"],
    &["npm", "test"],
    &["pnpm", "test"],
    &["make", "test"],
    &["bun", "test"],
];

/// True if `argv` starts with one of the allowlisted command prefixes.
/// Empty `argv` is always rejected.
pub fn is_allowlisted(argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    DEFAULT_COMMAND_ALLOWLIST.iter().any(|prefix| {
        prefix.len() <= argv.len() && prefix.iter().zip(argv.iter()).all(|(p, a)| p == a)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_known_prefixes() {
        assert!(is_allowlisted(&[
            "cargo".into(),
            "test".into(),
            "--package".into(),
            "foo".into()
        ]));
        assert!(is_allowlisted(&["npm".into(), "test".into()]));
    }

    #[test]
    fn allowlist_rejects_unknown_or_empty() {
        assert!(!is_allowlisted(&["rm".into(), "-rf".into(), "/".into()]));
        assert!(!is_allowlisted(&[]));
        assert!(!is_allowlisted(&["cargo".into(), "publish".into()]));
    }
}
