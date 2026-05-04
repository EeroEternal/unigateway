use std::time::{Duration, Instant, SystemTime};

use crate::error::GatewayError;
use crate::hooks::AttemptStartedEvent;
use crate::request::ProxyChatRequest;
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, ProxySession, RequestKind, StreamKind,
};

use super::super::reporting::{
    StreamingAttemptContext, apply_retry_backoff, failed_attempt_event, failed_attempt_report,
    new_shared_stream_state, should_retry_error, success_attempt_event, success_attempt_report,
    with_completed_request_report, with_streaming_attempt_reports,
};
use super::super::{FailedRequestContext, UniGatewayEngine};
use super::support::{execute_chat_attempt, observe_stream};

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
                    if super::super::reporting::is_saturation_error(&error) {
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
}
