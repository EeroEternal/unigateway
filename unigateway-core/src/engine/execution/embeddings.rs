use std::time::{Duration, Instant, SystemTime};

use crate::error::GatewayError;
use crate::hooks::AttemptStartedEvent;
use crate::request::ProxyEmbeddingsRequest;
use crate::response::{CompletedResponse, EmbeddingsResponse, RequestKind};

use super::super::reporting::{
    apply_retry_backoff, failed_attempt_event, failed_attempt_report, should_retry_error,
    success_attempt_event, success_attempt_report, with_completed_request_report,
};
use super::super::{FailedRequestContext, UniGatewayEngine};
use super::support::execute_embeddings_attempt;

impl UniGatewayEngine {
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
