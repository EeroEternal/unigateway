use std::collections::HashMap;
use std::sync::Arc;

use crate::InMemoryDriverRegistry;
use crate::pool::{EndpointRef, ExecutionPlan, ExecutionTarget};
use crate::response::{AttemptStatus, ProxySession};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

use super::super::UniGatewayEngine;
use super::support::{
    BehaviorDriver, HookRecorder, TestBehavior, chat_request, endpoint, pool, responses_request,
};

#[tokio::test]
async fn fallback_strategy_tries_next_endpoint_on_failure() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream500),
            ("b".to_string(), TestBehavior::Success),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::Fallback,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "b");
            assert_eq!(result.response.output_text.as_deref(), Some("b"));
            assert_eq!(result.report.attempts.len(), 2);
            assert_eq!(result.report.attempts[0].status, AttemptStatus::Retried);
            assert_eq!(result.report.attempts[1].status, AttemptStatus::Succeeded);
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn round_robin_retries_only_for_configured_conditions() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream429),
            ("b".to_string(), TestBehavior::Success),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "b");
            assert_eq!(result.report.attempts.len(), 2);
            assert_eq!(result.report.attempts[0].status, AttemptStatus::Retried);
            assert_eq!(result.report.attempts[1].status, AttemptStatus::Succeeded);
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn chat_failure_returns_aggregated_attempt_reports() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream429),
            ("b".to_string(), TestBehavior::Upstream500),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let error = match engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
    {
        Ok(_) => panic!("chat should fail after retries"),
        Err(error) => error,
    };

    match error {
        crate::error::GatewayError::AllAttemptsFailed {
            attempts,
            last_error,
        } => {
            assert_eq!(attempts.len(), 2);
            assert_eq!(attempts[0].status, AttemptStatus::Retried);
            assert_eq!(attempts[1].status, AttemptStatus::Failed);
            assert!(matches!(
                *last_error,
                crate::error::GatewayError::UpstreamHttp { status: 500, .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn responses_failure_returns_aggregated_attempt_reports() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::new(),
        responses: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream429),
            ("b".to_string(), TestBehavior::Upstream500),
        ]),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let error = match engine
        .proxy_responses(
            responses_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
    {
        Ok(_) => panic!("responses should fail after retries"),
        Err(error) => error,
    };

    match error {
        crate::error::GatewayError::AllAttemptsFailed {
            attempts,
            last_error,
        } => {
            assert_eq!(attempts.len(), 2);
            assert_eq!(attempts[0].status, AttemptStatus::Retried);
            assert_eq!(attempts[1].status, AttemptStatus::Failed);
            assert!(matches!(
                *last_error,
                crate::error::GatewayError::UpstreamHttp { status: 500, .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn hooks_receive_failed_attempts_and_failed_request_report() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream429),
            ("b".to_string(), TestBehavior::Upstream500),
        ]),
        responses: HashMap::new(),
    }));
    let hooks = HookRecorder::default();

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_hooks(Arc::new(hooks.clone()))
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    if engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .is_ok()
    {
        panic!("chat should fail after retries");
    }

    let started = hooks.state.started.lock().expect("started lock");
    let finished = hooks.state.finished.lock().expect("finished lock");
    let requests = hooks.state.requests.lock().expect("requests lock");

    assert_eq!(started.len(), 2);
    assert_eq!(finished.len(), 2);
    assert_eq!(requests.len(), 1);
    assert_eq!(finished[0].status_code, Some(429));
    assert_eq!(finished[1].status_code, Some(500));
    assert_eq!(requests[0].attempts.len(), 2);
    assert_eq!(requests[0].selected_endpoint_id, "b");
}

#[tokio::test]
async fn aimd_on_saturation_reduces_limit_for_429() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([("a".to_string(), TestBehavior::Upstream429)]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();

    let mut pool_def = pool(
        "alpha",
        LoadBalancingStrategy::RoundRobin,
        vec![endpoint("a")],
    );
    pool_def.retry_policy.retry_on = vec![];
    engine.upsert_pool(pool_def).await.expect("upsert pool");

    let _ = engine.aimd_for_endpoint("a").await;

    let aimd_before = engine.aimd_metrics().await;
    let initial_limit = aimd_before.get("a").unwrap().current_limit;

    let _ = engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await;

    let aimd_after = engine.aimd_metrics().await;
    let new_limit = aimd_after.get("a").unwrap().current_limit;

    assert!(
        new_limit < initial_limit,
        "AIMD limit should decrease after 429 response. before: {}, after: {}",
        initial_limit,
        new_limit
    );
}

#[tokio::test]
async fn aimd_saturation_yields_all_endpoints_saturated() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(super::support::TestDriver));

    let hook_recorder = HookRecorder::default();
    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_hooks(Arc::new(hook_recorder.clone()))
        .build()
        .unwrap();

    let pool_def = pool(
        "alpha",
        LoadBalancingStrategy::RoundRobin,
        vec![endpoint("alpha_only")],
    );
    engine.upsert_pool(pool_def).await.expect("upsert pool");

    let aimd = engine.aimd_for_endpoint("alpha_only").await;
    let mut guards = Vec::new();
    while let Some(guard) = aimd.acquire(None) {
        guards.push(guard);
    }

    let result = engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await;

    match result {
        Err(crate::error::GatewayError::AllEndpointsSaturated { pool_id }) => {
            assert_eq!(pool_id.as_deref(), Some("alpha"));
        }
        Err(error) => panic!("expected AllEndpointsSaturated, got error: {}", error),
        Ok(_) => panic!("expected AllEndpointsSaturated, got Ok"),
    }

    assert!(hook_recorder.state.started.lock().unwrap().is_empty());
    assert!(hook_recorder.state.finished.lock().unwrap().is_empty());
    assert!(hook_recorder.state.requests.lock().unwrap().is_empty());
}

fn plan_with_candidates(
    candidates: Vec<&str>,
    retry_policy_override: Option<RetryPolicy>,
) -> ExecutionTarget {
    ExecutionTarget::Plan(ExecutionPlan {
        pool_id: Some("alpha".to_string()),
        candidates: candidates
            .into_iter()
            .map(|endpoint_id| EndpointRef {
                endpoint_id: endpoint_id.to_string(),
            })
            .collect(),
        load_balancing_override: Some(LoadBalancingStrategy::Fallback),
        retry_policy_override,
        metadata: HashMap::new(),
    })
}

#[tokio::test]
async fn execution_plan_fallback_stays_within_candidates() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream500),
            ("b".to_string(), TestBehavior::Success),
            ("c".to_string(), TestBehavior::Success),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_routing_feedback_provider(Arc::new(super::support::StaticFeedbackProvider {
            by_pool: HashMap::from([(
                "alpha".to_string(),
                crate::feedback::RoutingFeedback {
                    endpoint_signals: HashMap::from([(
                        "c".to_string(),
                        crate::feedback::EndpointSignal {
                            score: Some(100.0),
                            excluded: false,
                            cooldown_until: None,
                            recent_error_rate: None,
                        },
                    )]),
                },
            )]),
        }))
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

    let session = engine
        .proxy_chat(
            chat_request(false),
            plan_with_candidates(vec!["a", "b"], None),
        )
        .await
        .expect("proxy chat");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "b");
            assert_eq!(result.report.attempts.len(), 2);
            assert!(
                result
                    .report
                    .attempts
                    .iter()
                    .all(|attempt| attempt.endpoint_id == "a" || attempt.endpoint_id == "b")
            );
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn execution_plan_excludes_endpoints_outside_plan() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream500),
            ("b".to_string(), TestBehavior::Success),
            ("c".to_string(), TestBehavior::Success),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
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

    let snapshot = engine
        .execution_snapshot(&plan_with_candidates(vec!["a", "b"], None))
        .await
        .expect("plan snapshot");

    assert_eq!(snapshot.endpoints.len(), 2);
    assert!(
        snapshot
            .endpoints
            .iter()
            .all(|endpoint| endpoint.endpoint_id == "a" || endpoint.endpoint_id == "b")
    );
}

#[tokio::test]
async fn execution_plan_retry_policy_override_limits_attempts() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(BehaviorDriver {
        chat: HashMap::from([
            ("a".to_string(), TestBehavior::Upstream500),
            ("b".to_string(), TestBehavior::Success),
        ]),
        responses: HashMap::new(),
    }));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::Fallback,
            vec![endpoint("a"), endpoint("b")],
        ))
        .await
        .expect("upsert pool");

    let limited = plan_with_candidates(
        vec!["a", "b"],
        Some(RetryPolicy {
            max_attempts: 1,
            per_attempt_timeout: None,
            retry_on: vec![RetryCondition::HttpStatus(500)],
            backoff: BackoffPolicy::None,
            stop_after_stream_started: true,
        }),
    );

    let result = engine.proxy_chat(chat_request(false), limited).await;
    match result {
        Err(crate::error::GatewayError::AllAttemptsFailed { attempts, .. }) => {
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].endpoint_id, "a");
        }
        Err(_) => panic!("expected AllAttemptsFailed"),
        Ok(_) => panic!("expected single failed attempt, got success"),
    }

    let session = engine
        .proxy_chat(
            chat_request(false),
            plan_with_candidates(
                vec!["a", "b"],
                Some(RetryPolicy {
                    max_attempts: 2,
                    per_attempt_timeout: None,
                    retry_on: vec![RetryCondition::HttpStatus(500)],
                    backoff: BackoffPolicy::None,
                    stop_after_stream_started: true,
                }),
            ),
        )
        .await
        .expect("proxy chat with override max_attempts=2");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "b");
            assert_eq!(result.report.attempts.len(), 2);
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}
