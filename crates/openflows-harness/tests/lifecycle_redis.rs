//! Run against a disposable Redis: TEST_REDIS_URL=redis://127.0.0.1:6379 cargo test
//! -p openflows-harness --test lifecycle_redis -- --ignored
use config::lifecycle::{Event, Phase};
use openflows_harness::HarnessStore;
use pocketflow_core::SharedStore;

#[tokio::test]
#[ignore = "requires disposable Redis in TEST_REDIS_URL"]
async fn real_redis_plan_round_trip_and_atomic_review_cycle() {
    let url = std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL");
    let tenant = format!("lifecycle-test-{}", uuid::Uuid::new_v4());
    let harness = HarnessStore::new(&url, &tenant).await.unwrap();
    let a = SharedStore::new_redis_with_tenant(&url, Some(tenant.clone()))
        .await
        .unwrap();
    let b = SharedStore::new_redis_with_tenant(&url, Some(tenant))
        .await
        .unwrap();
    let plan = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(plan.path(), "# Plan\n\nVerify the target workflow.").unwrap();
    harness.plan_write("T-1", plan.path()).await.unwrap();
    let state = a.lifecycle("T-1").await.unwrap();
    assert_eq!(state.plan, "# Plan\n\nVerify the target workflow.");
    assert_eq!(
        a.get_typed::<String>("pair:T-1:plan").await.as_deref(),
        Some(state.plan.as_str())
    );
    harness
        .status_set("T-1", "forge", "plan_ready")
        .await
        .unwrap();
    let state = a.lifecycle("T-1").await.unwrap();
    let decision = Event::Decide {
        round: state.review_round,
        phase: Phase::PlanReady,
        approved: true,
        report: "Reviewed".into(),
        revision: state.revision,
        head: None,
    };
    let (first, second) = tokio::join!(
        a.transition("T-1", state.version, "sentinel", decision.clone()),
        b.transition("T-1", state.version, "sentinel", decision)
    );
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "only one competing decision may commit"
    );
    harness
        .status_set("T-1", "forge", "building")
        .await
        .unwrap();
    assert!(harness.status_set("T-1", "forge", "submit").await.is_err());
    assert!(harness.plan_write("T-1", plan.path()).await.is_err());
    let state = a.lifecycle("T-1").await.unwrap();
    assert_eq!(state.phase, Phase::Building);
    assert_eq!(state.history.len(), 4);
    // Fault injection belongs only on this disposable Redis instance.
    // A rejected write must not consume approval or advance the phase.
    use fred::prelude::*;
    let client = Builder::from_config(Config::from_url(&url).unwrap())
        .build()
        .unwrap();
    client.init().await.unwrap();
    client
        .config_set("maxmemory-policy", "noeviction")
        .await
        .unwrap();
    client.config_set("maxmemory", "1").await.unwrap();
    let failed = harness.status_set("T-1", "forge", "blocked").await;
    client.config_set("maxmemory", "0").await.unwrap();
    let error = failed.expect_err("Redis must reject a write over maxmemory");
    assert!(
        format!("{error:#}").contains("OOM"),
        "unexpected error: {error:#}"
    );
    assert_eq!(a.lifecycle("T-1").await.unwrap(), state);
    a.del("ticket:T-1:status").await;
    a.del("pair:T-1:plan").await;
}
