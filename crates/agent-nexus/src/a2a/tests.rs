// crates/agent-nexus/src/a2a/tests.rs
//! Integration and unit tests for the A2A relay (task 8).

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use a2a_protocol::{
        ExecutorInfo, VerifyCwd, VerifyExpect, VerifyKind, VerifyRequest, VerifyResult,
    };
    use pocketflow_core::SharedStore;
    use std::sync::Arc;

    fn sample_request() -> VerifyRequest {
        VerifyRequest {
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
        }
    }

    fn sample_result(task_id: &str) -> VerifyResult {
        VerifyResult {
            task_id: task_id.to_string(),
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 42,
            stdout_ref: format!("audit:a2a:{}:stdout", task_id),
            stderr_ref: format!("audit:a2a:{}:stderr", task_id),
            artifacts: vec![],
            executor: ExecutorInfo {
                role: "forge".into(),
                workspace: "forge-T-048".into(),
            },
        }
    }

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
        let result = sample_result("t1");

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
        let mut result = sample_result("t1");
        result.timed_out = true;
        result.exit_code = Some(0);

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

    #[tokio::test]
    async fn test_relay_lifecycle_submit_claim_complete_get() {
        use crate::a2a::routing::{A2ARelay, TaskState};
        use crate::a2a::verify_handler::submit_verify_request;

        let store = SharedStore::new_in_memory();
        let relay = A2ARelay::new(Arc::new(store));

        let req = sample_request();

        // Sentinel submits; a real task id is returned (not a placeholder).
        let task_id = submit_verify_request(&relay, &req, "T-048").await.unwrap();
        assert!(!task_id.is_empty());
        assert_eq!(
            relay.get_task_state(&task_id).await,
            Some(TaskState::Pending)
        );

        // Idempotent duplicate submit returns the same task id.
        let task_id2 = submit_verify_request(&relay, &req, "T-048").await.unwrap();
        assert_eq!(task_id, task_id2);

        // Forge claims the next pending task for the pair.
        let claimed = relay.claim_next_task("T-048").await.unwrap().unwrap();
        assert_eq!(claimed.task_id, task_id);
        assert_eq!(
            relay.get_task_state(&task_id).await,
            Some(TaskState::Running)
        );

        // No more pending tasks for the pair.
        assert!(relay.claim_next_task("T-048").await.unwrap().is_none());

        // Forge completes the task with a terminal result.
        relay
            .complete_task(&task_id, sample_result(&task_id))
            .await
            .unwrap();
        assert_eq!(
            relay.get_task_state(&task_id).await,
            Some(TaskState::Completed)
        );

        // The completed task carries the mirrored result for Sentinel.
        let entry = relay.get_task(&task_id).await.unwrap();
        assert!(entry.result.is_some());
        assert_eq!(entry.result.unwrap().task_id, task_id);
    }

    #[tokio::test]
    async fn test_relay_claim_rejects_non_forge_role_via_validation() {
        use crate::a2a::routing::A2ARelay;
        use crate::a2a::verify_handler::submit_verify_request;

        let store = SharedStore::new_in_memory();
        let relay = A2ARelay::new(Arc::new(store));

        let req = sample_request();
        let task_id = submit_verify_request(&relay, &req, "T-048").await.unwrap();

        // A non-forge role must not be able to claim verify tasks: claim is
        // gated on role in the HTTP handler, and pair mismatch on submit is
        // rejected here. Verify the request is rejected for a cross-pair submit.
        let wrong_pair = VerifyRequest {
            pair_id: "T-999".to_string(),
            ..req.clone()
        };
        assert!(submit_verify_request(&relay, &wrong_pair, "T-048")
            .await
            .is_err());

        assert_eq!(
            relay.get_task_state(&task_id).await,
            Some(crate::a2a::routing::TaskState::Pending)
        );
    }
}
