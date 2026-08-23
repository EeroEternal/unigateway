//! Generic retry/fallback execution loop shared by all request kinds.
//!
//! `proxy_chat`, `proxy_responses`, and `proxy_embeddings` previously carried
//! three ~85% identical copies of the AIMD acquire → driver context → attempt
//! reporting → fallback loop. This module owns the single skeleton; each
//! public entry point only supplies its [`RequestKind`], stream metadata, and
//! a per-attempt execute closure, then maps the returned
//! [`EndpointAttemptOutput`] onto its own public return type.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::error::GatewayError;
use crate::hooks::AttemptStartedEvent;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{RequestKind, StreamKind};
use crate::routing::ExecutionSnapshot;

use super::super::reporting::{
    apply_retry_backoff, build_aimd_capacity_skipped_event, failed_attempt_event,
    failed_attempt_report, should_retry_error, success_attempt_event, success_attempt_report,
    with_completed_request_report,
};
use super::super::{FailedRequestContext, UniGatewayEngine};
pub(super) use super::support::EndpointAttemptOutput;
use super::support::observe_stream_outcome;

/// Per-request parameters that vary between request kinds.
#[derive(Debug, Clone, Copy)]
pub(super) struct RequestExecutionParams {
    /// Reported on every lifecycle event and failure finalization.
    pub kind: RequestKind,
    /// Set when the request kind can produce a streaming session; the
    /// embeddings kind has no streaming branch.
    pub stream_kind: Option<StreamKind>,
    /// Value reported as `streaming` on the request-started event.
    pub streaming: bool,
}

/// Read access to the request-kind metadata map merged into lifecycle events.
pub(super) trait ProxyRequestMetadata {
    fn proxy_request_metadata(&self) -> &HashMap<String, String>;
}

impl ProxyRequestMetadata for ProxyChatRequest {
    fn proxy_request_metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }
}

impl ProxyRequestMetadata for ProxyResponsesRequest {
    fn proxy_request_metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }
}

impl ProxyRequestMetadata for ProxyEmbeddingsRequest {
    fn proxy_request_metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }
}

impl UniGatewayEngine {
    /// Runs one logical request across the snapshot's ordered endpoints with
    /// AIMD gating, per-attempt reporting, retry backoff, and terminal
    /// failure finalization.
    pub(super) async fn execute_with_fallback<Req, Chunk, Final, Exec, OutFut>(
        &self,
        request: Req,
        target: crate::pool::ExecutionTarget,
        params: RequestExecutionParams,
        chunk_forwarder: Option<super::support::ChunkForwarder<Chunk>>,
        execute: Exec,
    ) -> Result<EndpointAttemptOutput<Chunk, Final>, GatewayError>
    where
        Req: Clone + Send + 'static + ProxyRequestMetadata,
        Chunk: Send + Clone + 'static,
        Final: Send + 'static,
        Exec: Fn(
            Arc<dyn crate::drivers::ProviderDriver>,
            crate::endpoint_context::DriverEndpointContext,
            Req,
            Option<Duration>,
        ) -> OutFut,
        OutFut: Future<Output = Result<EndpointAttemptOutput<Chunk, Final>, GatewayError>>,
    {
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();

        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let mut request_metadata = snapshot.metadata.clone();
        request_metadata.extend(request.proxy_request_metadata().clone());
        let mut attempts = Vec::new();

        self.emit_request_started(crate::hooks::RequestStartedEvent {
            request_id: request_id.clone(),
            correlation_id: request_id.clone(),
            pool_id: snapshot.pool_id.clone(),
            kind: params.kind,
            streaming: params.streaming,
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
            let aimd_guard = match aimd.acquire(endpoint.max_concurrency) {
                Some(guard) => guard,
                None => {
                    skipped_due_to_aimd += 1;
                    let metadata = self
                        .driver_context(
                            snapshot.pool_id.clone(),
                            endpoint.clone(),
                            snapshot.metadata.clone(),
                            request.proxy_request_metadata().clone(),
                            snapshot.forward_metadata_as_headers.clone(),
                        )
                        .metadata;
                    self.emit_attempt_skipped(build_aimd_capacity_skipped_event(
                        &request_id,
                        snapshot.pool_id.clone(),
                        &endpoint,
                        attempt_index,
                        &aimd,
                        metadata,
                    ))
                    .await;
                    continue;
                }
            };

            let provider_kind = endpoint.provider_kind;
            last_context = Some((endpoint_id.clone(), provider_kind));
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.proxy_request_metadata().clone(),
                snapshot.forward_metadata_as_headers.clone(),
            );
            let attempt_metadata = context.metadata.clone();

            let attempt_record_index = attempts.len();
            let active_attempts_at_start = aimd.snapshot().active_connections;
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                correlation_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                provider_kind,
                attempt_index: attempt_record_index,
                active_attempts_at_start,
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
                        .finalize_failure(
                            &snapshot,
                            &request_id,
                            request_started_at,
                            attempts,
                            endpoint_id,
                            provider_kind,
                            attempt_metadata,
                            error,
                            params.kind,
                        )
                        .await);
                }
            };

            match execute(
                driver,
                context,
                request.clone(),
                snapshot.retry_policy.per_attempt_timeout,
            )
            .await
            {
                Ok(EndpointAttemptOutput::Completed(result)) => {
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

                    let result =
                        with_completed_request_report(*result, &request_id, attempts, params.kind);
                    self.emit_request_finished(result.report.clone()).await;
                    aimd.on_success();
                    return Ok(EndpointAttemptOutput::Completed(Box::new(result)));
                }
                Ok(EndpointAttemptOutput::Streaming(streaming)) => {
                    let stream_kind = params
                        .stream_kind
                        .expect("streaming attempt output requires params.stream_kind to be set");
                    let outcome = observe_stream_outcome(
                        streaming,
                        super::super::reporting::StreamingAttemptContext {
                            request_id,
                            pool_id: snapshot.pool_id.clone(),
                            endpoint_id,
                            provider_kind,
                            request_kind: params.kind,
                            stream_kind,
                            request_started_at,
                            attempt_started_at_system_time,
                            attempt_started_at: started_at,
                            metadata: attempt_metadata.clone(),
                            previous_attempts: attempts,
                            hooks: self.inner.hooks.clone(),
                            aimd,
                            aimd_guard: Some(aimd_guard),
                        },
                        chunk_forwarder,
                    )
                    .await;
                    return Ok(EndpointAttemptOutput::Streaming(outcome));
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
                        .finalize_failure(
                            &snapshot,
                            &request_id,
                            request_started_at,
                            attempts,
                            endpoint_id,
                            provider_kind,
                            attempt_metadata,
                            error,
                            params.kind,
                        )
                        .await);
                }
            }
        }

        if let Some(error) = last_error {
            let (endpoint_id, provider_kind) = last_context.unwrap();
            return Err(self
                .finalize_failure(
                    &snapshot,
                    &request_id,
                    request_started_at,
                    attempts,
                    endpoint_id,
                    provider_kind,
                    std::collections::HashMap::new(),
                    error,
                    params.kind,
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

    /// Shared terminal-failure path: wraps the accumulated attempt reports
    /// into a `FailedRequestContext` and delegates to the engine's failure
    /// finalizer.
    #[allow(clippy::too_many_arguments)]
    async fn finalize_failure(
        &self,
        snapshot: &ExecutionSnapshot,
        request_id: &str,
        request_started_at: SystemTime,
        attempts: Vec<crate::response::AttemptReport>,
        endpoint_id: String,
        provider_kind: crate::pool::ProviderKind,
        metadata: std::collections::HashMap<String, String>,
        error: GatewayError,
        kind: RequestKind,
    ) -> GatewayError {
        self.finalize_request_failure(
            FailedRequestContext {
                request_id: request_id.to_string(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id,
                provider_kind,
                started_at: request_started_at,
                metadata,
            },
            attempts,
            error,
            kind,
        )
        .await
    }
}
