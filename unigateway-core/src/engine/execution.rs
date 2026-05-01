use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::hooks::AttemptStartedEvent;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    RequestKind, ResponseStream, ResponsesEvent, ResponsesFinal, StreamKind,
};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::reporting::{
    SharedStreamState, StreamingAttemptContext, apply_retry_backoff, failed_attempt_event,
    failed_attempt_report, new_shared_stream_state, should_retry_error, success_attempt_event,
    success_attempt_report, with_completed_request_report, with_streaming_attempt_reports,
};
use super::{FailedRequestContext, UniGatewayEngine};

impl UniGatewayEngine {
    /// Dispatches a chat completion request to a specific endpoint or pool with fallbacks.
    /// Returns a session representing the lifecycle of the response stream or monolithic text.
    pub async fn proxy_chat(
        &self,
        mut request: ProxyChatRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();

        if let Some(hooks) = &self.inner.hooks {
            hooks.on_request(&mut request).await;
        }

        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let mut request_metadata = snapshot.metadata.clone();
        request_metadata.extend(request.metadata.clone());
        let mut attempts = Vec::new();

        self.emit_request_started(crate::hooks::RequestStartedEvent {
            request_id: request_id.clone(),
            correlation_id: request_id.clone(),
            pool_id: snapshot.pool_id.clone(),
            kind: RequestKind::Chat,
            streaming: request.stream,
            started_at: request_started_at,
            metadata: request_metadata,
        })
        .await;

        let mut skipped_due_to_aimd = 0;
        let mut last_error: Option<GatewayError> = None;
        let mut last_context: Option<(String, crate::pool::ProviderKind)> = None;

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();

            let aimd = self.aimd_for_endpoint(&endpoint_id).await;
            let aimd_guard = match aimd.acquire() {
                Some(guard) => guard,
                None => {
                    skipped_due_to_aimd += 1;
                    continue;
                }
            };

            let provider_kind = endpoint.provider_kind;
            last_context = Some((endpoint_id.clone(), provider_kind));
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();

            let attempt_record_index = attempts.len();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                correlation_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                provider_kind,
                attempt_index: attempt_record_index,
                metadata: attempt_metadata.clone(),
            })
            .await;
            let attempt_started_at_system_time = SystemTime::now();
            let started_at = Instant::now();

            let driver = match self.driver_for_endpoint(&endpoint) {
                Ok(driver) => driver,
                Err(error) => {
                    let latency = started_at.elapsed();
                    attempts.push(failed_attempt_report(&endpoint_id, latency, &error, false));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                        &error,
                    ))
                    .await;
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Chat,
                        )
                        .await);
                }
            };

            match execute_chat_attempt(
                driver,
                context,
                request.clone(),
                snapshot.retry_policy.per_attempt_timeout,
            )
            .await
            {
                Ok(ProxySession::Completed(result)) => {
                    let latency = Duration::from_millis(result.report.latency_ms);
                    attempts.push(success_attempt_report(&endpoint_id, latency));
                    self.emit_attempt_finished(success_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                    ))
                    .await;

                    let result = with_completed_request_report(
                        result,
                        &request_id,
                        attempts,
                        RequestKind::Chat,
                    );
                    self.emit_request_finished(result.report.clone()).await;
                    aimd.on_success();
                    return Ok(ProxySession::Completed(result));
                }
                Ok(ProxySession::Streaming(mut streaming)) => {
                    let streaming_context = StreamingAttemptContext {
                        request_id,
                        pool_id: snapshot.pool_id.clone(),
                        endpoint_id,
                        provider_kind,
                        request_kind: RequestKind::Chat,
                        stream_kind: StreamKind::Chat,
                        request_started_at,
                        attempt_started_at_system_time,
                        attempt_started_at: started_at,
                        metadata: attempt_metadata.clone(),
                        previous_attempts: attempts,
                        hooks: self.inner.hooks.clone(),
                        aimd,
                        aimd_guard: Some(aimd_guard),
                    };
                    let shared_stream_state = new_shared_stream_state(&streaming_context);
                    shared_stream_state.started().await;

                    // Wrap the stream so on_stream_chunk is called for each chunk.
                    if let Some(hooks) = &self.inner.hooks {
                        let hooks = hooks.clone();
                        let shared_stream_state = shared_stream_state.clone();
                        streaming.stream = observe_stream(
                            streaming.stream,
                            shared_stream_state.clone(),
                            move |chunk| {
                                let hooks = hooks.clone();
                                let shared_stream_state = shared_stream_state.clone();
                                let chunk = chunk.clone();
                                async move {
                                    shared_stream_state.record_chunk().await;
                                    hooks.on_stream_chunk(&chunk).await;
                                }
                            },
                        );
                    } else {
                        let shared_stream_state = shared_stream_state.clone();
                        streaming.stream = observe_stream(
                            streaming.stream,
                            shared_stream_state.clone(),
                            move |_chunk| {
                                let shared_stream_state = shared_stream_state.clone();
                                async move {
                                    shared_stream_state.record_chunk().await;
                                }
                            },
                        );
                    }

                    return Ok(ProxySession::Streaming(with_streaming_attempt_reports(
                        streaming,
                        streaming_context,
                        shared_stream_state,
                    )));
                }
                Err(error) => {
                    if super::reporting::is_saturation_error(&error) {
                        aimd.on_saturation();
                    }
                    let should_retry = attempt_index + 1 < total_attempts
                        && should_retry_error(
                            &snapshot.load_balancing,
                            &snapshot.retry_policy,
                            &error,
                        );
                    attempts.push(failed_attempt_report(
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                        should_retry,
                    ));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
                        last_error = Some(error);
                        continue;
                    }
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Chat,
                        )
                        .await);
                }
            }
        }

        if let Some(error) = last_error {
            let (endpoint_id, provider_kind) = last_context.unwrap();
            return Err(self
                .finalize_request_failure(
                    FailedRequestContext {
                        request_id: request_id.clone(),
                        pool_id: snapshot.pool_id.clone(),
                        endpoint_id,
                        provider_kind,
                        started_at: request_started_at,
                        metadata: std::collections::HashMap::new(),
                    },
                    attempts,
                    error,
                    RequestKind::Chat,
                )
                .await);
        }

        if attempts.is_empty() && skipped_due_to_aimd > 0 {
            Err(GatewayError::AllEndpointsSaturated {
                pool_id: snapshot.pool_id.clone(),
            })
        } else {
            Err(GatewayError::NoAvailableEndpoint {
                pool_id: snapshot.pool_id.clone(),
            })
        }
    }

    /// Dispatches a proxy responses stream request.
    pub async fn proxy_responses(
        &self,
        request: ProxyResponsesRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();
        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let mut request_metadata = snapshot.metadata.clone();
        request_metadata.extend(request.metadata.clone());
        let mut attempts = Vec::new();

        self.emit_request_started(crate::hooks::RequestStartedEvent {
            request_id: request_id.clone(),
            correlation_id: request_id.clone(),
            pool_id: snapshot.pool_id.clone(),
            kind: RequestKind::Responses,
            streaming: request.stream,
            started_at: request_started_at,
            metadata: request_metadata,
        })
        .await;

        let mut skipped_due_to_aimd = 0;
        let mut last_error: Option<GatewayError> = None;
        let mut last_context: Option<(String, crate::pool::ProviderKind)> = None;

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();

            let aimd = self.aimd_for_endpoint(&endpoint_id).await;
            let aimd_guard = match aimd.acquire() {
                Some(guard) => guard,
                None => {
                    skipped_due_to_aimd += 1;
                    continue;
                }
            };

            let provider_kind = endpoint.provider_kind;
            last_context = Some((endpoint_id.clone(), provider_kind));
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();

            let attempt_record_index = attempts.len();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                correlation_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                provider_kind,
                attempt_index: attempt_record_index,
                metadata: attempt_metadata.clone(),
            })
            .await;
            let attempt_started_at_system_time = SystemTime::now();
            let started_at = Instant::now();

            let driver = match self.driver_for_endpoint(&endpoint) {
                Ok(driver) => driver,
                Err(error) => {
                    let latency = started_at.elapsed();
                    attempts.push(failed_attempt_report(&endpoint_id, latency, &error, false));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                        &error,
                    ))
                    .await;
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Responses,
                        )
                        .await);
                }
            };

            match execute_responses_attempt(
                driver,
                context,
                request.clone(),
                snapshot.retry_policy.per_attempt_timeout,
            )
            .await
            {
                Ok(ProxySession::Completed(result)) => {
                    let latency = Duration::from_millis(result.report.latency_ms);
                    attempts.push(success_attempt_report(&endpoint_id, latency));
                    self.emit_attempt_finished(success_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                    ))
                    .await;

                    let result = with_completed_request_report(
                        result,
                        &request_id,
                        attempts,
                        RequestKind::Responses,
                    );
                    self.emit_request_finished(result.report.clone()).await;
                    aimd.on_success();
                    return Ok(ProxySession::Completed(result));
                }
                Ok(ProxySession::Streaming(streaming)) => {
                    let streaming_context = StreamingAttemptContext {
                        request_id,
                        pool_id: snapshot.pool_id.clone(),
                        endpoint_id,
                        provider_kind,
                        request_kind: RequestKind::Responses,
                        stream_kind: StreamKind::Responses,
                        request_started_at,
                        attempt_started_at_system_time,
                        attempt_started_at: started_at,
                        metadata: attempt_metadata.clone(),
                        previous_attempts: attempts,
                        hooks: self.inner.hooks.clone(),
                        aimd,
                        aimd_guard: Some(aimd_guard),
                    };
                    let shared_stream_state = new_shared_stream_state(&streaming_context);
                    shared_stream_state.started().await;
                    let mut streaming = streaming;
                    let shared_stream_state_for_chunks = shared_stream_state.clone();
                    streaming.stream = observe_stream(
                        streaming.stream,
                        shared_stream_state.clone(),
                        move |_chunk| {
                            let shared_stream_state = shared_stream_state_for_chunks.clone();
                            async move {
                                shared_stream_state.record_chunk().await;
                            }
                        },
                    );
                    return Ok(ProxySession::Streaming(with_streaming_attempt_reports(
                        streaming,
                        streaming_context,
                        shared_stream_state,
                    )));
                }
                Err(error) => {
                    if super::reporting::is_saturation_error(&error) {
                        aimd.on_saturation();
                    }
                    let should_retry = attempt_index + 1 < total_attempts
                        && should_retry_error(
                            &snapshot.load_balancing,
                            &snapshot.retry_policy,
                            &error,
                        );
                    attempts.push(failed_attempt_report(
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                        should_retry,
                    ));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
                        last_error = Some(error);
                        continue;
                    }
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Responses,
                        )
                        .await);
                }
            }
        }

        if let Some(error) = last_error {
            let (endpoint_id, provider_kind) = last_context.unwrap();
            return Err(self
                .finalize_request_failure(
                    FailedRequestContext {
                        request_id: request_id.clone(),
                        pool_id: snapshot.pool_id.clone(),
                        endpoint_id,
                        provider_kind,
                        started_at: request_started_at,
                        metadata: std::collections::HashMap::new(),
                    },
                    attempts,
                    error,
                    RequestKind::Responses,
                )
                .await);
        }

        if attempts.is_empty() && skipped_due_to_aimd > 0 {
            Err(GatewayError::AllEndpointsSaturated {
                pool_id: snapshot.pool_id.clone(),
            })
        } else {
            Err(GatewayError::NoAvailableEndpoint {
                pool_id: snapshot.pool_id.clone(),
            })
        }
    }

    /// Executes a stateless vector embeddings extraction.
    pub async fn proxy_embeddings(
        &self,
        request: ProxyEmbeddingsRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();
        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let mut request_metadata = snapshot.metadata.clone();
        request_metadata.extend(request.metadata.clone());
        let mut attempts = Vec::new();

        self.emit_request_started(crate::hooks::RequestStartedEvent {
            request_id: request_id.clone(),
            correlation_id: request_id.clone(),
            pool_id: snapshot.pool_id.clone(),
            kind: RequestKind::Embeddings,
            streaming: false,
            started_at: request_started_at,
            metadata: request_metadata,
        })
        .await;

        let mut skipped_due_to_aimd = 0;
        let mut last_error: Option<GatewayError> = None;
        let mut last_context: Option<(String, crate::pool::ProviderKind)> = None;

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();

            let aimd = self.aimd_for_endpoint(&endpoint_id).await;
            let _aimd_guard = match aimd.acquire() {
                Some(guard) => guard,
                None => {
                    skipped_due_to_aimd += 1;
                    continue;
                }
            };

            let provider_kind = endpoint.provider_kind;
            last_context = Some((endpoint_id.clone(), provider_kind));
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();

            let attempt_record_index = attempts.len();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                correlation_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                provider_kind,
                attempt_index: attempt_record_index,
                metadata: attempt_metadata.clone(),
            })
            .await;
            let started_at = Instant::now();

            let driver = match self.driver_for_endpoint(&endpoint) {
                Ok(driver) => driver,
                Err(error) => {
                    let latency = started_at.elapsed();
                    attempts.push(failed_attempt_report(&endpoint_id, latency, &error, false));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                        &error,
                    ))
                    .await;
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Embeddings,
                        )
                        .await);
                }
            };

            match execute_embeddings_attempt(
                driver,
                context,
                request.clone(),
                snapshot.retry_policy.per_attempt_timeout,
            )
            .await
            {
                Ok(response) => {
                    let latency = Duration::from_millis(response.report.latency_ms);
                    attempts.push(success_attempt_report(&endpoint_id, latency));
                    self.emit_attempt_finished(success_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                    ))
                    .await;

                    let response = with_completed_request_report(
                        response,
                        &request_id,
                        attempts,
                        RequestKind::Embeddings,
                    );
                    self.emit_request_finished(response.report.clone()).await;
                    aimd.on_success();
                    return Ok(response);
                }
                Err(error) => {
                    if super::reporting::is_saturation_error(&error) {
                        aimd.on_saturation();
                    }
                    let should_retry = attempt_index + 1 < total_attempts
                        && should_retry_error(
                            &snapshot.load_balancing,
                            &snapshot.retry_policy,
                            &error,
                        );
                    attempts.push(failed_attempt_report(
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                        should_retry,
                    ));
                    self.emit_attempt_finished(failed_attempt_event(
                        &request_id,
                        snapshot.pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
                        last_error = Some(error);
                        continue;
                    }
                    return Err(self
                        .finalize_request_failure(
                            FailedRequestContext {
                                request_id: request_id.clone(),
                                pool_id: snapshot.pool_id.clone(),
                                endpoint_id,
                                provider_kind,
                                started_at: request_started_at,
                                metadata: attempt_metadata.clone(),
                            },
                            attempts,
                            error,
                            RequestKind::Embeddings,
                        )
                        .await);
                }
            }
        }

        if let Some(error) = last_error {
            let (endpoint_id, provider_kind) = last_context.unwrap();
            return Err(self
                .finalize_request_failure(
                    FailedRequestContext {
                        request_id: request_id.clone(),
                        pool_id: snapshot.pool_id.clone(),
                        endpoint_id,
                        provider_kind,
                        started_at: request_started_at,
                        metadata: std::collections::HashMap::new(),
                    },
                    attempts,
                    error,
                    RequestKind::Embeddings,
                )
                .await);
        }

        if attempts.is_empty() && skipped_due_to_aimd > 0 {
            Err(GatewayError::AllEndpointsSaturated {
                pool_id: snapshot.pool_id.clone(),
            })
        } else {
            Err(GatewayError::NoAvailableEndpoint {
                pool_id: snapshot.pool_id.clone(),
            })
        }
    }
}

fn observe_stream<Chunk, Hook, HookFuture>(
    mut stream: ResponseStream<Chunk>,
    shared_stream_state: SharedStreamState,
    hook: Hook,
) -> ResponseStream<Chunk>
where
    Chunk: Send + 'static,
    Hook: Fn(&Chunk) -> HookFuture + Send + Sync + 'static,
    HookFuture: std::future::Future<Output = ()> + Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(item) = stream.next().await {
            if let Ok(ref chunk) = item {
                hook(chunk).await;
            }
            if sender.send(item).is_err() {
                break;
            }
        }
        shared_stream_state.mark_drained();
    });

    Box::pin(UnboundedReceiverStream::new(receiver))
}

async fn execute_chat_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyChatRequest,
    timeout: Option<Duration>,
) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_chat(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_chat(endpoint, request).await
    }
}

async fn execute_responses_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyResponsesRequest,
    timeout: Option<Duration>,
) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_responses(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_responses(endpoint, request).await
    }
}

async fn execute_embeddings_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyEmbeddingsRequest,
    timeout: Option<Duration>,
) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_embeddings(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_embeddings(endpoint, request).await
    }
}
