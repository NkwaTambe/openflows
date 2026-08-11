// crates/agent-nexus/src/a2a/tests.rs
//! Integration and unit tests for the A2A relay (task 8).

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use a2a_protocol::{VerifyCwd, VerifyExpect, VerifyKind, VerifyRequest};

    #[test]
    fn test_verify_request_idempotency_seed_is_stable() {
        let req = VerifyRequest {
            pair_id: "T-048".to_string(),
            kind: VerifyKind::Command,
            cwd: VerifyCwd::Repo,
            argv: vec!["cargo".into(), "test".into()],
            timeout_secs: 300,
            env_allowlist: vec![],
            expect: VerifyExpect {
                exit_code: Some(0),
                artifacts: vec![],
            },
        };

        let seed1 = req.idempotency_seed().unwrap();
        let seed2 = req.idempotency_seed().unwrap();

        // Seeds should be identical for identical requests
        assert_eq!(seed1, seed2);

        // Seed should be pair-scoped
        let mut req2 = req.clone();
        req2.pair_id = "T-049".to_string();
        let seed3 = req2.idempotency_seed().unwrap();

        assert_ne!(seed1, seed3);
        assert!(seed1.starts_with("T-048:"));
        assert!(seed3.starts_with("T-049:"));
    }

    #[test]
    fn test_allowlist_validation_passes_known_commands() {
        let valid_commands = vec![
            vec!["cargo", "test"],
            vec!["cargo", "test", "--package", "foo"],
            vec!["npm", "test"],
            vec!["make", "test"],
            vec!["bun", "test"],
        ];

        for cmd in valid_commands {
            let argv: Vec<String> = cmd.into_iter().map(|s| s.to_string()).collect();
            assert!(
                a2a_protocol::is_allowlisted(&argv),
                "Command {:?} should be allowlisted",
                argv
            );
        }
    }

    #[test]
    fn test_allowlist_validation_rejects_unknown_commands() {
        let invalid_commands = vec![
            vec!["rm", "-rf", "/"],
            vec!["cargo", "publish"],
            vec!["sh", "-c", "evil"],
            vec!["bash"],
            vec![],
        ];

        for cmd in invalid_commands {
            let argv: Vec<String> = cmd.into_iter().map(|s| s.to_string()).collect();
            assert!(
                !a2a_protocol::is_allowlisted(&argv),
                "Command {:?} should NOT be allowlisted",
                argv
            );
        }
    }

    #[test]
    fn test_verify_result_satisfies_checks_exit_code() {
        use a2a_protocol::{ExecutorInfo, VerifyResult};

        let result = VerifyResult {
            task_id: "t1".into(),
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 100,
            stdout_ref: "audit:a2a:t1:stdout".into(),
            stderr_ref: "audit:a2a:t1:stderr".into(),
            artifacts: vec![],
            executor: ExecutorInfo {
                role: "forge".into(),
                workspace: "forge-T-048".into(),
            },
        };

        let expect_pass = VerifyExpect {
            exit_code: Some(0),
            artifacts: vec![],
        };

        assert!(result.satisfies(&expect_pass));

        let expect_fail = VerifyExpect {
            exit_code: Some(1),
            artifacts: vec![],
        };

        assert!(!result.satisfies(&expect_fail));
    }

    #[test]
    fn test_verify_result_timeout_never_satisfies() {
        use a2a_protocol::{ExecutorInfo, VerifyResult};

        let result = VerifyResult {
            task_id: "t1".into(),
            exit_code: Some(0), // Even with exit 0
            timed_out: true,    // Timeout = failure
            duration_ms: 10000,
            stdout_ref: "audit:a2a:t1:stdout".into(),
            stderr_ref: "audit:a2a:t1:stderr".into(),
            artifacts: vec![],
            executor: ExecutorInfo {
                role: "forge".into(),
                workspace: "forge-T-048".into(),
            },
        };

        let any_expect = VerifyExpect {
            exit_code: Some(0),
            artifacts: vec![],
        };

        // Timeout always fails, regardless of exit code
        assert!(!result.satisfies(&any_expect));
    }

    #[test]
    fn test_redis_key_formatting() {
        use a2a_protocol::{audit_rejected_key, audit_task_key, verification_key};

        assert_eq!(verification_key("T-048"), "pair:T-048:verification");
        assert_eq!(audit_task_key("abc123"), "audit:a2a:abc123");
        assert_eq!(audit_rejected_key(), "audit:a2a:rejected");
    }
}
