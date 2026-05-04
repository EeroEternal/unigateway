use std::collections::HashMap;
use std::sync::Arc;

use crate::InMemoryDriverRegistry;
use crate::feedback::{EndpointSignal, RoutingFeedback};
use crate::pool::{EndpointRef, ExecutionPlan, ExecutionTarget};
use crate::retry::LoadBalancingStrategy;

use super::super::UniGatewayEngine;
use super::support::{StaticFeedbackProvider, endpoint, engine_with_empty_registry, pool};

#[tokio::test]
async fn upsert_get_list_and_remove_pool() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert");

    let stored = engine.get_pool("alpha").await.expect("stored pool");
    assert_eq!(stored.pool_id, "alpha");

    let listed = engine.list_pools().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].pool_id, "alpha");

    engine.remove_pool("alpha").await.expect("remove pool");
    assert!(engine.get_pool("alpha").await.is_none());
}

#[tokio::test]
async fn snapshot_is_stable_after_pool_update() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert first pool");

    let snapshot = engine
        .execution_snapshot(&ExecutionTarget::Pool {
            pool_id: "alpha".to_string(),
        })
        .await
        .expect("first snapshot");

    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("b")],
        ))
        .await
        .expect("upsert second pool");

    let next_snapshot = engine
        .execution_snapshot(&ExecutionTarget::Pool {
            pool_id: "alpha".to_string(),
        })
        .await
        .expect("second snapshot");

    assert_eq!(snapshot.pool_id.as_deref(), Some("alpha"));
    assert_eq!(snapshot.retry_policy.max_attempts, 2);
    assert_eq!(snapshot.endpoints[0].endpoint_id, "a");
    assert_eq!(next_snapshot.endpoints[0].endpoint_id, "b");
}

#[tokio::test]
async fn round_robin_rotates_across_enabled_endpoints() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let (_, first) = engine
        .select_endpoint_for_target(&ExecutionTarget::Pool {
            pool_id: "alpha".to_string(),
        })
        .await
        .expect("first selection");
    let (_, second) = engine
        .select_endpoint_for_target(&ExecutionTarget::Pool {
            pool_id: "alpha".to_string(),
        })
        .await
        .expect("second selection");

    assert_eq!(first.endpoint_id, "a");
    assert_eq!(second.endpoint_id, "b");
}

#[tokio::test]
async fn execution_plan_uses_candidate_subset() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b"), endpoint("c")],
        ))
        .await
        .expect("upsert pool");

    let snapshot = engine
        .execution_snapshot(&ExecutionTarget::Plan(ExecutionPlan {
            pool_id: Some("alpha".to_string()),
            candidates: vec![
                EndpointRef {
                    endpoint_id: "b".to_string(),
                },
                EndpointRef {
                    endpoint_id: "c".to_string(),
                },
            ],
            load_balancing_override: Some(LoadBalancingStrategy::Random),
            retry_policy_override: None,
            metadata: HashMap::new(),
        }))
        .await
        .expect("plan snapshot");

    assert_eq!(snapshot.pool_id.as_deref(), Some("alpha"));
    assert!(snapshot.metadata.is_empty());
    assert_eq!(snapshot.endpoints.len(), 2);
    assert_eq!(snapshot.load_balancing, LoadBalancingStrategy::Random);
    assert!(
        snapshot
            .endpoints
            .iter()
            .all(|item| item.endpoint_id == "b" || item.endpoint_id == "c")
    );
}

#[tokio::test]
async fn routing_feedback_prioritizes_scored_endpoints() {
    let feedback_provider = StaticFeedbackProvider {
        by_pool: HashMap::from([(
            "alpha".to_string(),
            RoutingFeedback {
                endpoint_signals: HashMap::from([
                    (
                        "a".to_string(),
                        EndpointSignal {
                            score: Some(10.0),
                            excluded: true,
                            cooldown_until: None,
                            recent_error_rate: Some(1.0),
                        },
                    ),
                    (
                        "b".to_string(),
                        EndpointSignal {
                            score: Some(65.0),
                            excluded: false,
                            cooldown_until: None,
                            recent_error_rate: Some(0.2),
                        },
                    ),
                    (
                        "c".to_string(),
                        EndpointSignal {
                            score: Some(95.0),
                            excluded: false,
                            cooldown_until: None,
                            recent_error_rate: Some(0.0),
                        },
                    ),
                ]),
            },
        )]),
    };

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(Arc::new(InMemoryDriverRegistry::new()))
        .with_routing_feedback_provider(Arc::new(feedback_provider))
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::Fallback,
            vec![endpoint("a"), endpoint("b"), endpoint("c")],
        ))
        .await
        .expect("upsert pool");

    let (_snapshot, selected) = engine
        .select_endpoint_for_target(&ExecutionTarget::Pool {
            pool_id: "alpha".to_string(),
        })
        .await
        .expect("select endpoint");

    assert_eq!(selected.endpoint_id, "c");
}
