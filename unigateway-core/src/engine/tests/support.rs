use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::future::BoxFuture;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::InMemoryDriverRegistry;
use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::feedback::{RoutingFeedback, RoutingFeedbackProvider};
use crate::hooks::{
    AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestStartedEvent, StreamChunkEvent,
    StreamStartedEvent,
};
use crate::pool::{Endpoint, ProviderKind, ProviderPool, SecretString};
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    RequestKind, RequestReport, ResponsesEvent, ResponsesFinal, StreamReport, StreamingResponse,
};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

use super::super::UniGatewayEngine;

#[derive(Clone)]
pub(super) enum TestBehavior {
    Success,
    Upstream429,
    Upstream500,
}

pub(super) fn endpoint(endpoint_id: &str) -> Endpoint {
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

pub(super) fn engine_with_empty_registry() -> UniGatewayEngine {
    UniGatewayEngine::builder()
        .with_driver_registry(Arc::new(InMemoryDriverRegistry::new()))
        .build()
        .unwrap()
}

pub(super) fn pool(
    pool_id: &str,
    strategy: LoadBalancingStrategy,
    endpoints: Vec<Endpoint>,
) -> ProviderPool {
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

pub(super) fn chat_request(stream: bool) -> ProxyChatRequest {
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
        stream,
        extra: HashMap::new(),
        metadata: HashMap::new(),
    }
}

pub(super) fn responses_request(stream: bool) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        model: "gpt-4.1-mini".to_string(),
        input: Some(json!([{"role": "user", "content": "hello"}])),
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stream,
        tools: None,
        tool_choice: None,
        previous_response_id: None,
        request_metadata: None,
        extra: HashMap::new(),
        metadata: HashMap::new(),
    }
}

pub(super) struct TestDriver;

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
                    correlation_id: "req-1".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
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
                    correlation_id: "req-2".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    kind: RequestKind::Responses,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
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
                    correlation_id: "req-3".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    kind: RequestKind::Embeddings,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: endpoint.metadata,
                },
            })
        })
    }
}

pub(super) struct BehaviorDriver {
    pub(super) chat: HashMap<String, TestBehavior>,
    pub(super) responses: HashMap<String, TestBehavior>,
}

pub(super) struct HookState {
    pub(super) request_started: std::sync::Mutex<Vec<RequestStartedEvent>>,
    pub(super) started: std::sync::Mutex<Vec<AttemptStartedEvent>>,
    pub(super) finished: std::sync::Mutex<Vec<AttemptFinishedEvent>>,
    pub(super) stream_started: std::sync::Mutex<Vec<StreamStartedEvent>>,
    pub(super) stream_chunks: std::sync::Mutex<Vec<StreamChunkEvent>>,
    pub(super) stream_completed: std::sync::Mutex<Vec<StreamReport>>,
    pub(super) stream_aborted: std::sync::Mutex<Vec<StreamReport>>,
    pub(super) requests: std::sync::Mutex<Vec<RequestReport>>,
}

impl Default for HookState {
    fn default() -> Self {
        Self {
            request_started: std::sync::Mutex::new(Vec::new()),
            started: std::sync::Mutex::new(Vec::new()),
            finished: std::sync::Mutex::new(Vec::new()),
            stream_started: std::sync::Mutex::new(Vec::new()),
            stream_chunks: std::sync::Mutex::new(Vec::new()),
            stream_completed: std::sync::Mutex::new(Vec::new()),
            stream_aborted: std::sync::Mutex::new(Vec::new()),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct HookRecorder {
    pub(super) state: Arc<HookState>,
}

impl GatewayHooks for HookRecorder {
    fn on_request_started(&self, event: RequestStartedEvent) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state
                .request_started
                .lock()
                .expect("request_started lock")
                .push(event);
        })
    }

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

    fn on_stream_started(&self, event: StreamStartedEvent) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state
                .stream_started
                .lock()
                .expect("stream_started lock")
                .push(event);
        })
    }

    fn on_stream_chunk_event(&self, event: StreamChunkEvent) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state
                .stream_chunks
                .lock()
                .expect("stream_chunks lock")
                .push(event);
        })
    }

    fn on_stream_completed(&self, report: StreamReport) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state
                .stream_completed
                .lock()
                .expect("stream_completed lock")
                .push(report);
        })
    }

    fn on_stream_aborted(&self, report: StreamReport) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state
                .stream_aborted
                .lock()
                .expect("stream_aborted lock")
                .push(report);
        })
    }

    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()> {
        let state = self.state.clone();
        Box::pin(async move {
            state.requests.lock().expect("requests lock").push(report);
        })
    }
}

pub(super) struct StreamingDriver;

impl ProviderDriver for StreamingDriver {
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
            if !request.stream {
                return Err(crate::error::GatewayError::InvalidRequest(
                    "streaming driver expects stream=true".to_string(),
                ));
            }

            let request_metadata = endpoint.metadata.clone();
            let endpoint_id = endpoint.endpoint_id.clone();
            let provider_kind = endpoint.provider_kind;
            let pool_id = request_metadata.get("pool_id").cloned();
            let (chunk_tx, chunk_rx) = mpsc::unbounded_channel();
            let (completion_tx, completion_rx) = oneshot::channel();

            tokio::spawn(async move {
                chunk_tx
                    .send(Ok(ChatResponseChunk {
                        delta: Some("hel".to_string()),
                        raw: json!({"chunk": 1}),
                    }))
                    .expect("first chunk send");
                chunk_tx
                    .send(Ok(ChatResponseChunk {
                        delta: Some("lo".to_string()),
                        raw: json!({"chunk": 2}),
                    }))
                    .expect("second chunk send");

                let _ = completion_tx.send(Ok(CompletedResponse {
                    response: ChatResponseFinal {
                        model: Some("gpt-4o-mini".to_string()),
                        output_text: Some("hello".to_string()),
                        raw: json!({"endpoint_id": endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "driver-stream".to_string(),
                        correlation_id: "driver-stream".to_string(),
                        pool_id,
                        selected_endpoint_id: endpoint_id,
                        selected_provider: provider_kind,
                        kind: RequestKind::Chat,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        error_kind: None,
                        stream: None,
                        metadata: request_metadata,
                    },
                }));
            });

            Ok(ProxySession::Streaming(StreamingResponse {
                stream: Box::pin(UnboundedReceiverStream::new(chunk_rx)),
                completion: completion_rx,
                request_id: "driver-stream".to_string(),
                request_metadata: request.metadata,
            }))
        })
    }

    fn execute_responses(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyResponsesRequest,
    ) -> BoxFuture<
        'static,
        Result<ProxySession<ResponsesEvent, ResponsesFinal>, crate::error::GatewayError>,
    > {
        Box::pin(async {
            Err(crate::error::GatewayError::not_implemented(
                "streaming driver responses",
            ))
        })
    }

    fn execute_embeddings(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>>
    {
        Box::pin(async {
            Err(crate::error::GatewayError::not_implemented(
                "streaming driver embeddings",
            ))
        })
    }
}

#[derive(Default)]
pub(super) struct StaticFeedbackProvider {
    pub(super) by_pool: HashMap<String, RoutingFeedback>,
}

impl RoutingFeedbackProvider for StaticFeedbackProvider {
    fn feedback(&self, pool_id: &str) -> RoutingFeedback {
        self.by_pool.get(pool_id).cloned().unwrap_or_default()
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
                        correlation_id: "req-test".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        kind: RequestKind::Chat,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        error_kind: None,
                        stream: None,
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
                        correlation_id: "req-resp".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        kind: RequestKind::Responses,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        error_kind: None,
                        stream: None,
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
                    correlation_id: "req-embed".to_string(),
                    pool_id: endpoint.metadata.get("pool_id").cloned(),
                    selected_endpoint_id: endpoint.endpoint_id,
                    selected_provider: endpoint.provider_kind,
                    kind: RequestKind::Embeddings,
                    attempts: Vec::new(),
                    usage: None,
                    latency_ms: 1,
                    started_at: SystemTime::UNIX_EPOCH,
                    finished_at: SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: endpoint.metadata,
                },
            })
        })
    }
}
