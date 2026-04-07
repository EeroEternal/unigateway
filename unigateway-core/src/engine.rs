use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::{Mutex, RwLock};

use crate::drivers::{DriverEndpointContext, DriverRegistry, ProviderDriver};
use crate::error::GatewayError;
use crate::hooks::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks};
use crate::pool::{
    Endpoint, ExecutionTarget, PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId,
};
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    AttemptReport, AttemptStatus, ChatResponseChunk, ChatResponseFinal, CompletedResponse,
    EmbeddingsResponse, ProxySession, RequestReport, ResponsesEvent, ResponsesFinal,
    StreamingResponse,
};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};
use crate::routing::ExecutionSnapshot;

struct EngineState {
    pools: RwLock<std::collections::HashMap<PoolId, ProviderPool>>,
    rr_counters: Mutex<std::collections::HashMap<String, usize>>,
    hooks: Option<Arc<dyn GatewayHooks>>,
    driver_registry: Option<Arc<dyn DriverRegistry>>,
    default_retry_policy: RetryPolicy,
    default_timeout: Option<Duration>,
}

pub struct UniGatewayEngine {
    inner: Arc<EngineState>,
}

struct FailedRequestContext {
    request_id: RequestId,
    pool_id: Option<PoolId>,
    endpoint_id: String,
    provider_kind: ProviderKind,
    started_at: SystemTime,
    metadata: std::collections::HashMap<String, String>,
}

#[derive(Default)]
pub struct UniGatewayEngineBuilder {
    pub hooks: Option<Arc<dyn GatewayHooks>>,
    pub driver_registry: Option<Arc<dyn DriverRegistry>>,
    pub default_retry_policy: RetryPolicy,
    pub default_timeout: Option<Duration>,
}

impl UniGatewayEngine {
    pub fn builder() -> UniGatewayEngineBuilder {
        UniGatewayEngineBuilder::default()
    }

    pub async fn upsert_pool(&self, pool: ProviderPool) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.insert(pool.pool_id.clone(), pool);
        Ok(())
    }

    pub async fn remove_pool(&self, pool_id: &str) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.remove(pool_id);

        let mut rr_guard = self.inner.rr_counters.lock().await;
        rr_guard.retain(|bucket, _| !bucket.starts_with(&format!("pool:{pool_id}:")));

        Ok(())
    }

    pub async fn get_pool(&self, pool_id: &str) -> Option<ProviderPool> {
        let guard = self.inner.pools.read().await;
        guard.get(pool_id).cloned()
    }

    pub async fn list_pools(&self) -> Vec<PoolSummary> {
        let guard = self.inner.pools.read().await;
        let mut pools: Vec<PoolSummary> = guard
            .values()
            .map(|pool| PoolSummary {
                pool_id: pool.pool_id.clone(),
                endpoint_count: pool.endpoints.len(),
                load_balancing: pool.load_balancing.clone(),
                metadata: pool.metadata.clone(),
            })
            .collect();
        pools.sort_by(|left, right| left.pool_id.cmp(&right.pool_id));
        pools
    }

    pub async fn proxy_chat(
        &self,
        request: ProxyChatRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();
        let mut attempts = Vec::new();

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();
            let provider_kind = endpoint.provider_kind;
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                attempt_index,
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
                        &endpoint_id,
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
                        &endpoint_id,
                        latency,
                    ))
                    .await;

                    let result = with_completed_request_report(result, &request_id, attempts);
                    self.emit_request_finished(result.report.clone()).await;
                    return Ok(ProxySession::Completed(result));
                }
                Ok(ProxySession::Streaming(streaming)) => {
                    return Ok(ProxySession::Streaming(with_streaming_attempt_reports(
                        streaming,
                        StreamingAttemptContext {
                            request_id,
                            pool_id: snapshot.pool_id.clone(),
                            endpoint_id,
                            provider_kind,
                            request_started_at,
                            attempt_started_at: started_at,
                            metadata: attempt_metadata.clone(),
                            previous_attempts: attempts,
                            hooks: self.inner.hooks.clone(),
                        },
                    )));
                }
                Err(error) => {
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
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
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
                        )
                        .await);
                }
            }
        }

        Err(GatewayError::NoAvailableEndpoint)
    }

    pub async fn proxy_responses(
        &self,
        request: ProxyResponsesRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();
        let mut attempts = Vec::new();

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();
            let provider_kind = endpoint.provider_kind;
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                attempt_index,
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
                        &endpoint_id,
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
                        &endpoint_id,
                        latency,
                    ))
                    .await;

                    let result = with_completed_request_report(result, &request_id, attempts);
                    self.emit_request_finished(result.report.clone()).await;
                    return Ok(ProxySession::Completed(result));
                }
                Ok(ProxySession::Streaming(streaming)) => {
                    return Ok(ProxySession::Streaming(with_streaming_attempt_reports(
                        streaming,
                        StreamingAttemptContext {
                            request_id,
                            pool_id: snapshot.pool_id.clone(),
                            endpoint_id,
                            provider_kind,
                            request_started_at,
                            attempt_started_at: started_at,
                            metadata: attempt_metadata.clone(),
                            previous_attempts: attempts,
                            hooks: self.inner.hooks.clone(),
                        },
                    )));
                }
                Err(error) => {
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
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
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
                        )
                        .await);
                }
            }
        }

        Err(GatewayError::NoAvailableEndpoint)
    }

    pub async fn proxy_embeddings(
        &self,
        request: ProxyEmbeddingsRequest,
        target: ExecutionTarget,
    ) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
        let snapshot = self.execution_snapshot(&target).await?;
        let endpoints = self.attempt_endpoints(&snapshot).await?;
        let total_attempts = endpoints.len();
        let request_id = crate::protocol::next_request_id();
        let request_started_at = SystemTime::now();
        let mut attempts = Vec::new();

        for (attempt_index, endpoint) in endpoints.into_iter().enumerate() {
            let endpoint_id = endpoint.endpoint_id.clone();
            let provider_kind = endpoint.provider_kind;
            let context = self.driver_context(
                snapshot.pool_id.clone(),
                endpoint.clone(),
                snapshot.metadata.clone(),
                request.metadata.clone(),
            );
            let attempt_metadata = context.metadata.clone();
            self.emit_attempt_started(AttemptStartedEvent {
                request_id: request_id.clone(),
                pool_id: snapshot.pool_id.clone(),
                endpoint_id: endpoint_id.clone(),
                attempt_index,
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
                        &endpoint_id,
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
                        &endpoint_id,
                        latency,
                    ))
                    .await;

                    let response = with_completed_request_report(response, &request_id, attempts);
                    self.emit_request_finished(response.report.clone()).await;
                    return Ok(response);
                }
                Err(error) => {
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
                        &endpoint_id,
                        started_at.elapsed(),
                        &error,
                    ))
                    .await;
                    if should_retry {
                        apply_retry_backoff(&snapshot.retry_policy.backoff, attempt_index).await;
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
                        )
                        .await);
                }
            }
        }

        Err(GatewayError::NoAvailableEndpoint)
    }

    pub(crate) async fn execution_snapshot(
        &self,
        target: &ExecutionTarget,
    ) -> Result<ExecutionSnapshot, GatewayError> {
        let guard = self.inner.pools.read().await;
        crate::routing::build_execution_snapshot(
            &guard,
            target,
            &self.inner.default_retry_policy,
            self.inner.default_timeout,
        )
    }

    #[cfg(test)]
    pub(crate) async fn select_endpoint_for_target(
        &self,
        target: &ExecutionTarget,
    ) -> Result<(ExecutionSnapshot, Endpoint), GatewayError> {
        let snapshot = self.execution_snapshot(target).await?;
        let mut rr_guard = self.inner.rr_counters.lock().await;
        let endpoint = snapshot.select_endpoint(&mut rr_guard)?;
        Ok((snapshot, endpoint))
    }

    async fn attempt_endpoints(
        &self,
        snapshot: &ExecutionSnapshot,
    ) -> Result<Vec<Endpoint>, GatewayError> {
        let mut rr_guard = self.inner.rr_counters.lock().await;
        snapshot.ordered_endpoints(&mut rr_guard, snapshot.retry_policy.max_attempts)
    }

    async fn emit_attempt_started(&self, event: AttemptStartedEvent) {
        emit_attempt_started_hook(self.inner.hooks.clone(), event).await;
    }

    async fn emit_attempt_finished(&self, event: AttemptFinishedEvent) {
        emit_attempt_finished_hook(self.inner.hooks.clone(), event).await;
    }

    async fn emit_request_finished(&self, report: RequestReport) {
        emit_request_finished_hook(self.inner.hooks.clone(), report).await;
    }

    async fn finalize_request_failure(
        &self,
        context: FailedRequestContext,
        attempts: Vec<AttemptReport>,
        error: GatewayError,
    ) -> GatewayError {
        if attempts.is_empty() {
            return error;
        }

        let report = build_failed_request_report(&context, attempts.clone(), SystemTime::now());
        self.emit_request_finished(report).await;

        GatewayError::AllAttemptsFailed {
            attempts,
            last_error: Box::new(error),
        }
    }

    fn driver_for_endpoint(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Arc<dyn ProviderDriver>, GatewayError> {
        let Some(registry) = self.inner.driver_registry.as_ref() else {
            return Err(GatewayError::InvalidRequest(
                "driver registry not configured".to_string(),
            ));
        };

        registry.get(&endpoint.driver_id).ok_or_else(|| {
            GatewayError::InvalidRequest(format!("driver not found: {}", endpoint.driver_id))
        })
    }

    fn driver_context(
        &self,
        pool_id: Option<PoolId>,
        endpoint: Endpoint,
        snapshot_metadata: std::collections::HashMap<String, String>,
        request_metadata: std::collections::HashMap<String, String>,
    ) -> DriverEndpointContext {
        let mut metadata = snapshot_metadata;
        metadata.extend(endpoint.metadata.clone());
        if let Some(pool_id) = pool_id {
            metadata.entry("pool_id".to_string()).or_insert(pool_id);
        }
        // Request-level metadata has highest priority so callers can attach
        // trace/tenant/user tags that propagate into RequestReport.metadata.
        metadata.extend(request_metadata);

        DriverEndpointContext {
            endpoint_id: endpoint.endpoint_id,
            provider_kind: endpoint.provider_kind,
            base_url: endpoint.base_url,
            api_key: endpoint.api_key,
            model_policy: endpoint.model_policy,
            metadata,
        }
    }
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

fn success_attempt_report(endpoint_id: &str, latency: Duration) -> AttemptReport {
    AttemptReport {
        endpoint_id: endpoint_id.to_string(),
        status: AttemptStatus::Succeeded,
        latency_ms: latency.as_millis() as u64,
        error: None,
    }
}

fn failed_attempt_report(
    endpoint_id: &str,
    latency: Duration,
    error: &GatewayError,
    retried: bool,
) -> AttemptReport {
    AttemptReport {
        endpoint_id: endpoint_id.to_string(),
        status: if retried {
            AttemptStatus::Retried
        } else {
            AttemptStatus::Failed
        },
        latency_ms: latency.as_millis() as u64,
        error: Some(error.to_string()),
    }
}

fn success_attempt_event(
    request_id: &str,
    endpoint_id: &str,
    latency: Duration,
) -> AttemptFinishedEvent {
    AttemptFinishedEvent {
        request_id: request_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
        success: true,
        status_code: None,
        latency_ms: latency.as_millis() as u64,
        error: None,
    }
}

fn failed_attempt_event(
    request_id: &str,
    endpoint_id: &str,
    latency: Duration,
    error: &GatewayError,
) -> AttemptFinishedEvent {
    AttemptFinishedEvent {
        request_id: request_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
        success: false,
        status_code: error.status_code(),
        latency_ms: latency.as_millis() as u64,
        error: Some(error.to_string()),
    }
}

fn build_failed_request_report(
    context: &FailedRequestContext,
    attempts: Vec<AttemptReport>,
    finished_at: SystemTime,
) -> RequestReport {
    let latency_ms = finished_at
        .duration_since(context.started_at)
        .unwrap_or_default()
        .as_millis() as u64;

    RequestReport {
        request_id: context.request_id.clone(),
        pool_id: context.pool_id.clone(),
        selected_endpoint_id: context.endpoint_id.clone(),
        selected_provider: context.provider_kind,
        attempts,
        usage: None,
        latency_ms,
        started_at: context.started_at,
        finished_at,
        metadata: context.metadata.clone(),
    }
}

fn should_retry_error(
    strategy: &LoadBalancingStrategy,
    retry_policy: &RetryPolicy,
    error: &GatewayError,
) -> bool {
    if matches!(strategy, LoadBalancingStrategy::Fallback) {
        return !matches!(
            error,
            GatewayError::PoolNotFound(_) | GatewayError::NoAvailableEndpoint
        );
    }

    retry_policy
        .retry_on
        .iter()
        .any(|condition| retry_condition_matches(condition, error))
}

fn retry_condition_matches(condition: &RetryCondition, error: &GatewayError) -> bool {
    match condition {
        RetryCondition::HttpStatus(status) => {
            matches!(error, GatewayError::UpstreamHttp { status: value, .. } if value == status)
        }
        RetryCondition::HttpStatusRange { start, end } => matches!(
            error,
            GatewayError::UpstreamHttp { status, .. } if status >= start && status <= end
        ),
        RetryCondition::Timeout => matches!(
            error,
            GatewayError::Transport { message, .. } if message == "attempt timed out"
        ),
        RetryCondition::TransportError => matches!(
            error,
            GatewayError::Transport { .. } | GatewayError::StreamAborted { .. }
        ),
    }
}

async fn apply_retry_backoff(policy: &BackoffPolicy, attempt_index: usize) {
    let delay = match policy {
        BackoffPolicy::None => None,
        BackoffPolicy::Fixed(delay) => Some(*delay),
        BackoffPolicy::Exponential { base, max, jitter } => {
            let factor = 1u32.checked_shl(attempt_index as u32).unwrap_or(u32::MAX);
            let mut delay = base.checked_mul(factor).unwrap_or(*max);
            if delay > *max {
                delay = *max;
            }
            if *jitter {
                use rand::Rng;

                let upper_ms = delay.as_millis().max(1) as u64;
                let jitter_ms = rand::thread_rng().gen_range(0..=upper_ms);
                delay = Duration::from_millis(jitter_ms);
            }
            Some(delay)
        }
    };

    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
}

async fn emit_attempt_started_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: AttemptStartedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_attempt_started(event).await;
    }
}

async fn emit_attempt_finished_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: AttemptFinishedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_attempt_finished(event).await;
    }
}

async fn emit_request_finished_hook(hooks: Option<Arc<dyn GatewayHooks>>, report: RequestReport) {
    if let Some(hooks) = hooks {
        hooks.on_request_finished(report).await;
    }
}

fn with_completed_request_report<T>(
    mut response: CompletedResponse<T>,
    request_id: &str,
    attempts: Vec<AttemptReport>,
) -> CompletedResponse<T> {
    response.report.request_id = request_id.to_string();
    response.report.attempts = attempts;
    response
}

struct StreamingAttemptContext {
    request_id: RequestId,
    pool_id: Option<PoolId>,
    endpoint_id: String,
    provider_kind: ProviderKind,
    request_started_at: SystemTime,
    attempt_started_at: Instant,
    metadata: std::collections::HashMap<String, String>,
    previous_attempts: Vec<AttemptReport>,
    hooks: Option<Arc<dyn GatewayHooks>>,
}

fn aggregate_attempt_failure(attempts: Vec<AttemptReport>, error: GatewayError) -> GatewayError {
    if attempts.is_empty() {
        error
    } else {
        GatewayError::AllAttemptsFailed {
            attempts,
            last_error: Box::new(error),
        }
    }
}

fn with_streaming_attempt_reports<Chunk, Final>(
    streaming: StreamingResponse<Chunk, Final>,
    context: StreamingAttemptContext,
) -> StreamingResponse<Chunk, Final>
where
    Chunk: Send + 'static,
    Final: Send + 'static,
{
    let StreamingResponse {
        stream,
        completion,
        request_id: _,
    } = streaming;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let response_request_id = context.request_id.clone();

    tokio::spawn(async move {
        let StreamingAttemptContext {
            request_id,
            pool_id,
            endpoint_id,
            provider_kind,
            request_started_at,
            attempt_started_at,
            metadata,
            previous_attempts,
            hooks,
        } = context;

        let completion_result = completion.await.unwrap_or_else(|_| {
            Err(GatewayError::Transport {
                message: "stream completion channel dropped".to_string(),
                endpoint_id: None,
            })
        });

        let result = match completion_result {
            Ok(mut completed) => {
                let latency = Duration::from_millis(completed.report.latency_ms);
                let mut attempts = previous_attempts;
                attempts.push(success_attempt_report(&endpoint_id, latency));
                completed.report.request_id = request_id.clone();
                completed.report.attempts = attempts;

                emit_attempt_finished_hook(
                    hooks.clone(),
                    success_attempt_event(&request_id, &endpoint_id, latency),
                )
                .await;
                emit_request_finished_hook(hooks, completed.report.clone()).await;

                Ok(completed)
            }
            Err(error) => {
                let latency = attempt_started_at.elapsed();
                let mut attempts = previous_attempts;
                attempts.push(failed_attempt_report(&endpoint_id, latency, &error, false));

                emit_attempt_finished_hook(
                    hooks.clone(),
                    failed_attempt_event(&request_id, &endpoint_id, latency, &error),
                )
                .await;
                emit_request_finished_hook(
                    hooks,
                    build_failed_request_report(
                        &FailedRequestContext {
                            request_id: request_id.clone(),
                            pool_id,
                            endpoint_id: endpoint_id.clone(),
                            provider_kind,
                            started_at: request_started_at,
                            metadata,
                        },
                        attempts.clone(),
                        SystemTime::now(),
                    ),
                )
                .await;

                Err(aggregate_attempt_failure(attempts, error))
            }
        };

        let _ = sender.send(result);
    });

    StreamingResponse {
        stream,
        completion: receiver,
        request_id: response_request_id,
    }
}

impl UniGatewayEngineBuilder {
    pub fn with_hooks(mut self, hooks: Arc<dyn GatewayHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn with_driver_registry(mut self, registry: Arc<dyn DriverRegistry>) -> Self {
        self.driver_registry = Some(registry);
        self
    }

    /// Registers the built-in OpenAI-compatible and Anthropic drivers backed by the default
    /// `reqwest` HTTP transport. This is the zero-boilerplate starting point for most callers.
    ///
    /// Equivalent to manually creating an [`InMemoryDriverRegistry`], instantiating
    /// [`ReqwestHttpTransport`], calling [`builtin_drivers`], and passing the registry via
    /// [`with_driver_registry`].
    ///
    /// [`InMemoryDriverRegistry`]: crate::registry::InMemoryDriverRegistry
    /// [`ReqwestHttpTransport`]: crate::transport::ReqwestHttpTransport
    /// [`builtin_drivers`]: crate::protocol::builtin_drivers
    /// [`with_driver_registry`]: Self::with_driver_registry
    pub fn with_builtin_http_drivers(self) -> Self {
        use crate::registry::InMemoryDriverRegistry;
        use crate::transport::ReqwestHttpTransport;

        let transport = Arc::new(ReqwestHttpTransport::default());
        let registry = Arc::new(InMemoryDriverRegistry::new());
        for driver in crate::protocol::builtin_drivers(transport) {
            registry.register(driver);
        }
        self.with_driver_registry(registry)
    }

    pub fn with_default_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.default_retry_policy = retry_policy;
        self
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    pub fn build(self) -> UniGatewayEngine {
        UniGatewayEngine {
            inner: Arc::new(EngineState {
                pools: RwLock::new(std::collections::HashMap::new()),
                rr_counters: Mutex::new(std::collections::HashMap::new()),
                hooks: self.hooks,
                driver_registry: self.driver_registry,
                default_retry_policy: self.default_retry_policy,
                default_timeout: self.default_timeout,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
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
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: format!("https://{endpoint_id}.example.com"),
            api_key: SecretString::new(format!("sk-{endpoint_id}")),
            model_policy: Default::default(),
            enabled: true,
            metadata: HashMap::new(),
        }
    }

    fn pool(
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

    #[tokio::test]
    async fn upsert_get_list_and_remove_pool() {
        let engine = UniGatewayEngine::builder().build();
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
        let engine = UniGatewayEngine::builder().build();
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
        let engine = UniGatewayEngine::builder().build();
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
        let engine = UniGatewayEngine::builder().build();
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
        ) -> BoxFuture<
            'static,
            Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>,
        > {
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
        ) -> BoxFuture<
            'static,
            Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>,
        > {
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
            .build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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
        let engine = UniGatewayEngine::builder().build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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

        assert!(error.to_string().contains("driver registry not configured"));
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
            .build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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
            .build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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
            .build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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
            .build();
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
            .build();
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
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
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
}
