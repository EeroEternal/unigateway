use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::future::BoxFuture;
use serde_json::json;

use crate::InMemoryDriverRegistry;
use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::hooks::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks};
use crate::pool::{
    Endpoint, ExecutionPlan, ExecutionTarget, ProviderKind, ProviderPool, SecretString,
};
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    RequestReport, ResponsesEvent, ResponsesFinal,
};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

use super::UniGatewayEngine;

#[derive(Clone)]
enum TestBehavior {
    Success,
    Upstream429,
    Upstream500,
}

fn endpoint(endpoint_id: &str) -> Endpoint {
    Endpoint {
        endpoint_id: endpoint_id.to_string(),
        provider_name: Some(endpoint_id.to_string()),
        source_endpoint_id: None,
        provider_family: None,
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url: format!("https://{endpoint_id}.example.com"),
        api_key: SecretString::new(format!("sk-{endpoint_id}")),
        model_policy: Default::default(),
        enabled: true,
        metadata: HashMap::new(),
    }
}

fn engine_with_empty_registry() -> UniGatewayEngine {
    UniGatewayEngine::builder()
        .with_driver_registry(Arc::new(InMemoryDriverRegistry::new()))
        .build()
        .unwrap()
}

fn pool(pool_id: &str, strategy: LoadBalancingStrategy, endpoints: Vec<Endpoint>) -> ProviderPool {
    ProviderPool {
        pool_id: pool_id.to_string(),
        endpoints,
        load_balancing: strategy,
        retry_policy: RetryPolicy {
            max_attempts: 2,
            per_attempt_timeout: None,
            retry_on: vec![RetryCondition::HttpStatus(429)],
            backoff: BackoffPolicy::None,
            stop_after_stream_started: true,
        },
        metadata: HashMap::new(),
    }
}

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
                crate::pool::EndpointRef {
                    endpoint_id: "b".to_string(),
                },
                crate::pool::EndpointRef {
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

struct TestDriver;

impl ProviderDriver for TestDriver {
    fn driver_id(&self) -> &str {
        "openai-compatible"
    }

    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<
        'static,
        Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, crate::error::GatewayError>,
    > {
        Box::pin(async move {
            Ok(ProxySession::Completed(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some(request.model),
                    output_text: Some(endpoint.endpoint_id.clone()),
                    raw: json!({"endpoint_id": endpoint.endpoint_id}),
                },
                report: RequestReport {
                    request_id: "req-1".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    metadata: endpoint.metadata,
                },
            }))
        })
    }

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        _request: ProxyResponsesRequest,
    ) -> BoxFuture<
        'static,
        Result<ProxySession<ResponsesEvent, ResponsesFinal>, crate::error::GatewayError>,
    > {
        Box::pin(async move {
            Ok(ProxySession::Completed(CompletedResponse {
                response: ResponsesFinal {
                    output_text: Some(endpoint.endpoint_id.clone()),
                    raw: json!({"endpoint_id": endpoint.endpoint_id}),
                },
                report: RequestReport {
                    request_id: "req-2".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    metadata: endpoint.metadata,
                },
            }))
        })
    }

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        _request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>>
    {
        Box::pin(async move {
            Ok(CompletedResponse {
                response: EmbeddingsResponse {
                    raw: json!({"endpoint_id": endpoint.endpoint_id}),
                },
                report: RequestReport {
                    request_id: "req-3".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    metadata: endpoint.metadata,
                },
            })
        })
    }
}

struct BehaviorDriver {
    chat: HashMap<String, TestBehavior>,
    responses: HashMap<String, TestBehavior>,
}

#[derive(Default)]
struct HookState {
    started: std::sync::Mutex<Vec<AttemptStartedEvent>>,
    finished: std::sync::Mutex<Vec<AttemptFinishedEvent>>,
    requests: std::sync::Mutex<Vec<RequestReport>>,
}

#[derive(Clone, Default)]
struct HookRecorder {
    state: Arc<HookState>,
}

impl GatewayHooks for HookRecorder {
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state.started.lock().expect("started lock").push(event);
        })
    }

    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state.finished.lock().expect("finished lock").push(event);
        })
    }

    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state.requests.lock().expect("requests lock").push(report);
        })
    }
}

impl ProviderDriver for BehaviorDriver {
    fn driver_id(&self) -> &str {
        "openai-compatible"
    }

    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<
        'static,
        Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, crate::error::GatewayError>,
    > {
        let behavior = self
            .chat
            .get(&endpoint.endpoint_id)
            .cloned()
            .unwrap_or(TestBehavior::Success);
        Box::pin(async move {
            match behavior {
                TestBehavior::Success => Ok(ProxySession::Completed(CompletedResponse {
                    response: ChatResponseFinal {
                        model: Some(request.model),
                        output_text: Some(endpoint.endpoint_id.clone()),
                        raw: json!({"endpoint_id": endpoint.endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "req-test".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        metadata: endpoint.metadata,
                    },
                })),
                TestBehavior::Upstream429 => Err(crate::error::GatewayError::UpstreamHttp {
                    status: 429,
                    body: Some("rate limited".to_string()),
                    endpoint_id: endpoint.endpoint_id,
                }),
                TestBehavior::Upstream500 => Err(crate::error::GatewayError::UpstreamHttp {
                    status: 500,
                    body: Some("boom".to_string()),
                    endpoint_id: endpoint.endpoint_id,
                }),
            }
        })
    }

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        _request: ProxyResponsesRequest,
    ) -> BoxFuture<
        'static,
        Result<ProxySession<ResponsesEvent, ResponsesFinal>, crate::error::GatewayError>,
    > {
        let behavior = self
            .responses
            .get(&endpoint.endpoint_id)
            .cloned()
            .unwrap_or(TestBehavior::Success);
        Box::pin(async move {
            match behavior {
                TestBehavior::Success => Ok(ProxySession::Completed(CompletedResponse {
                    response: ResponsesFinal {
                        output_text: Some(endpoint.endpoint_id.clone()),
                        raw: json!({"endpoint_id": endpoint.endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "req-resp".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        metadata: endpoint.metadata,
                    },
                })),
                TestBehavior::Upstream429 => Err(crate::error::GatewayError::UpstreamHttp {
                    status: 429,
                    body: Some("rate limited".to_string()),
                    endpoint_id: endpoint.endpoint_id,
                }),
                TestBehavior::Upstream500 => Err(crate::error::GatewayError::UpstreamHttp {
                    status: 500,
                    body: Some("boom".to_string()),
                    endpoint_id: endpoint.endpoint_id,
                }),
            }
        })
    }

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        _request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>>
    {
        Box::pin(async move {
            Ok(CompletedResponse {
                response: EmbeddingsResponse {
                    raw: json!({"endpoint_id": endpoint.endpoint_id}),
                },
                report: RequestReport {
                    request_id: "req-embed".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    metadata: endpoint.metadata,
                },
            })
        })
    }
}

#[tokio::test]
async fn proxy_chat_delegates_to_registered_driver() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(TestDriver));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "a");
            assert_eq!(result.report.pool_id.as_deref(), Some("alpha"));
            assert_eq!(result.response.output_text.as_deref(), Some("a"));
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn proxy_chat_fails_when_driver_missing() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert pool");

    let error = match engine
        .proxy_chat(
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
    {
        Ok(_) => panic!("missing driver registry should fail"),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("driver not found: openai-compatible")
    );
}

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
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
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
            assert_eq!(
                result.report.attempts[0].status,
                crate::response::AttemptStatus::Retried
            );
            assert_eq!(
                result.report.attempts[1].status,
                crate::response::AttemptStatus::Succeeded
            );
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
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
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
            assert_eq!(
                result.report.attempts[0].status,
                crate::response::AttemptStatus::Retried
            );
            assert_eq!(
                result.report.attempts[1].status,
                crate::response::AttemptStatus::Succeeded
            );
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
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
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
            assert_eq!(attempts[0].status, crate::response::AttemptStatus::Retried);
            assert_eq!(attempts[1].status, crate::response::AttemptStatus::Failed);
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
            ProxyResponsesRequest {
                model: "gpt-4.1-mini".to_string(),
                input: Some(json!([{"role": "user", "content": "hello"}])),
                instructions: None,
                temperature: None,
                top_p: None,
                max_output_tokens: None,
                stream: false,
                tools: None,
                tool_choice: None,
                previous_response_id: None,
                request_metadata: None,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
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
            assert_eq!(attempts[0].status, crate::response::AttemptStatus::Retried);
            assert_eq!(attempts[1].status, crate::response::AttemptStatus::Failed);
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
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
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
    // don't retry 429 so it fails immediately
    pool_def.retry_policy.retry_on = vec![];
    engine.upsert_pool(pool_def).await.expect("upsert pool");

    // pre-populate AIMD state
    let _ = engine.aimd_for_endpoint("a").await;

    // initial AIMD limit
    let aimd_before = engine.aimd_metrics().await;
    let initial_limit = aimd_before.get("a").unwrap().current_limit;

    let _ = engine
        .proxy_chat(
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
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
    registry.register(Arc::new(TestDriver));

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
    // Drain all active connections to saturate it
    let mut guards = Vec::new();
    while let Some(guard) = aimd.acquire() {
        guards.push(guard);
    }

    // Now execute a request, it should fail immediately with AllEndpointsSaturated
    let result = engine
        .proxy_chat(
            ProxyChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: Vec::new(),
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                metadata: HashMap::new(),
            },
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await;

    match result {
        Err(crate::error::GatewayError::AllEndpointsSaturated { pool_id }) => {
            assert_eq!(pool_id.as_deref(), Some("alpha"));
        }
        Err(e) => panic!("expected AllEndpointsSaturated, got error: {}", e),
        Ok(_) => panic!("expected AllEndpointsSaturated, got Ok"),
    }

    assert!(hook_recorder.state.started.lock().unwrap().is_empty());
    assert!(hook_recorder.state.finished.lock().unwrap().is_empty());
    assert!(hook_recorder.state.requests.lock().unwrap().is_empty());
}
