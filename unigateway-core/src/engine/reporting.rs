use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::error::GatewayError;
use crate::hooks::{
    AttemptFinishedEvent, GatewayHooks, RequestStartedEvent, StreamChunkEvent, StreamStartedEvent,
};
use crate::pool::{PoolId, ProviderKind, RequestId};
use crate::response::{
    AttemptReport, AttemptStatus, CompletedResponse, RequestKind, RequestReport, StreamKind,
    StreamOutcome, StreamReport, StreamingResponse,
};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

use super::FailedRequestContext;

pub(super) fn success_attempt_report(endpoint_id: &str, latency: Duration) -> AttemptReport {
    AttemptReport {
        endpoint_id: endpoint_id.to_string(),
        status: AttemptStatus::Succeeded,
        latency_ms: latency.as_millis() as u64,
        error: None,
        error_kind: None,
    }
}

pub(super) fn failed_attempt_report(
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
        error_kind: Some(error.kind()),
    }
}

pub(super) fn success_attempt_event(
    request_id: &str,
    pool_id: Option<&str>,
    endpoint_id: &str,
    provider_kind: ProviderKind,
    latency: Duration,
) -> AttemptFinishedEvent {
    AttemptFinishedEvent {
        request_id: request_id.to_string(),
        correlation_id: request_id.to_string(),
        pool_id: pool_id.map(str::to_string),
        endpoint_id: endpoint_id.to_string(),
        provider_kind,
        success: true,
        status_code: None,
        latency_ms: latency.as_millis() as u64,
        error: None,
        error_kind: None,
    }
}

pub(super) fn failed_attempt_event(
    request_id: &str,
    pool_id: Option<&str>,
    endpoint_id: &str,
    provider_kind: ProviderKind,
    latency: Duration,
    error: &GatewayError,
) -> AttemptFinishedEvent {
    AttemptFinishedEvent {
        request_id: request_id.to_string(),
        correlation_id: request_id.to_string(),
        pool_id: pool_id.map(str::to_string),
        endpoint_id: endpoint_id.to_string(),
        provider_kind,
        success: false,
        status_code: error.status_code(),
        latency_ms: latency.as_millis() as u64,
        error: Some(error.to_string()),
        error_kind: Some(error.kind()),
    }
}

pub(super) fn build_failed_request_report(
    context: &FailedRequestContext,
    attempts: Vec<AttemptReport>,
    finished_at: SystemTime,
    kind: RequestKind,
    stream: Option<StreamReport>,
    error_kind: Option<crate::error::GatewayErrorKind>,
) -> RequestReport {
    let latency_ms = finished_at
        .duration_since(context.started_at)
        .unwrap_or_default()
        .as_millis() as u64;

    RequestReport {
        request_id: context.request_id.clone(),
        correlation_id: context.request_id.clone(),
        pool_id: context.pool_id.clone(),
        selected_endpoint_id: context.endpoint_id.clone(),
        selected_provider: context.provider_kind,
        kind,
        attempts,
        usage: None,
        latency_ms,
        started_at: context.started_at,
        finished_at,
        error_kind,
        stream,
        metadata: context.metadata.clone(),
    }
}

pub(super) fn should_retry_error(
    strategy: &LoadBalancingStrategy,
    retry_policy: &RetryPolicy,
    error: &GatewayError,
) -> bool {
    if matches!(strategy, LoadBalancingStrategy::Fallback) {
        return !matches!(
            error,
            GatewayError::PoolNotFound(_)
                | GatewayError::NoAvailableEndpoint { .. }
                | GatewayError::AllEndpointsSaturated { .. }
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

pub(super) async fn apply_retry_backoff(policy: &BackoffPolicy, attempt_index: usize) {
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

pub(super) async fn emit_attempt_started_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: crate::hooks::AttemptStartedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_attempt_started(event).await;
    }
}

pub(super) async fn emit_request_started_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: RequestStartedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_request_started(event).await;
    }
}

pub(super) async fn emit_attempt_finished_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: AttemptFinishedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_attempt_finished(event).await;
    }
}

pub(super) async fn emit_request_finished_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    report: RequestReport,
) {
    if let Some(hooks) = hooks {
        hooks.on_request_finished(report).await;
    }
}

pub(super) async fn emit_stream_started_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: StreamStartedEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_stream_started(event).await;
    }
}

pub(super) async fn emit_stream_chunk_event_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    event: StreamChunkEvent,
) {
    if let Some(hooks) = hooks {
        hooks.on_stream_chunk_event(event).await;
    }
}

pub(super) async fn emit_stream_completed_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    report: StreamReport,
) {
    if let Some(hooks) = hooks {
        hooks.on_stream_completed(report).await;
    }
}

pub(super) async fn emit_stream_aborted_hook(
    hooks: Option<Arc<dyn GatewayHooks>>,
    report: StreamReport,
) {
    if let Some(hooks) = hooks {
        hooks.on_stream_aborted(report).await;
    }
}

pub(super) fn with_completed_request_report<T>(
    mut response: CompletedResponse<T>,
    request_id: &str,
    attempts: Vec<AttemptReport>,
    kind: RequestKind,
) -> CompletedResponse<T> {
    response.report.request_id = request_id.to_string();
    response.report.correlation_id = request_id.to_string();
    response.report.attempts = attempts;
    response.report.kind = kind;
    response
}

pub(super) struct StreamingAttemptContext {
    pub(super) request_id: RequestId,
    pub(super) pool_id: Option<PoolId>,
    pub(super) endpoint_id: String,
    pub(super) provider_kind: ProviderKind,
    pub(super) request_kind: RequestKind,
    pub(super) stream_kind: StreamKind,
    pub(super) request_started_at: SystemTime,
    pub(super) attempt_started_at_system_time: SystemTime,
    pub(super) attempt_started_at: Instant,
    pub(super) metadata: std::collections::HashMap<String, String>,
    pub(super) previous_attempts: Vec<AttemptReport>,
    pub(super) hooks: Option<Arc<dyn GatewayHooks>>,
    pub(super) aimd: Arc<crate::engine::AdaptiveConcurrency>,
    pub(super) aimd_guard: Option<crate::engine::aimd::AimdGuard>,
}

#[derive(Debug, Default)]
struct StreamObservationState {
    first_chunk_at: Option<SystemTime>,
    first_chunk_ttft_ms: Option<u64>,
    last_chunk_at: Option<SystemTime>,
    max_inter_chunk_ms: Option<u64>,
    chunk_count: u64,
}

#[derive(Clone)]
pub(super) struct SharedStreamState {
    request_id: RequestId,
    pool_id: Option<PoolId>,
    endpoint_id: String,
    provider_kind: ProviderKind,
    kind: StreamKind,
    started_at: SystemTime,
    attempt_started_at: Instant,
    metadata: std::collections::HashMap<String, String>,
    hooks: Option<Arc<dyn GatewayHooks>>,
    inner: Arc<Mutex<StreamObservationState>>,
    drained: Arc<tokio::sync::Notify>,
    drained_flag: Arc<AtomicBool>,
}

impl SharedStreamState {
    pub(super) async fn started(&self) {
        emit_stream_started_hook(
            self.hooks.clone(),
            StreamStartedEvent {
                request_id: self.request_id.clone(),
                correlation_id: self.request_id.clone(),
                pool_id: self.pool_id.clone(),
                endpoint_id: self.endpoint_id.clone(),
                provider_kind: self.provider_kind,
                kind: self.kind,
                started_at: self.started_at,
                metadata: self.metadata.clone(),
            },
        )
        .await;
    }

    pub(super) async fn record_chunk(&self) {
        let chunk_at = SystemTime::now();
        let (chunk_index, first_chunk, ttft_ms, max_inter_chunk_ms) = {
            let mut state = self.inner.lock().expect("stream observation lock");
            let previous_chunk_at = state.last_chunk_at;
            let ttft_ms = if state.first_chunk_at.is_none() {
                let ttft_ms = self.attempt_started_at.elapsed().as_millis() as u64;
                state.first_chunk_at = Some(chunk_at);
                state.first_chunk_ttft_ms = Some(ttft_ms);
                Some(ttft_ms)
            } else {
                state.first_chunk_ttft_ms
            };
            if let Some(previous_chunk_at) = previous_chunk_at {
                let inter_chunk_ms = chunk_at
                    .duration_since(previous_chunk_at)
                    .unwrap_or_default()
                    .as_millis() as u64;
                state.max_inter_chunk_ms = Some(
                    state
                        .max_inter_chunk_ms
                        .map(|current| current.max(inter_chunk_ms))
                        .unwrap_or(inter_chunk_ms),
                );
            }
            state.last_chunk_at = Some(chunk_at);
            state.chunk_count += 1;
            (
                state.chunk_count - 1,
                state.chunk_count == 1,
                ttft_ms,
                state.max_inter_chunk_ms,
            )
        };

        emit_stream_chunk_event_hook(
            self.hooks.clone(),
            StreamChunkEvent {
                request_id: self.request_id.clone(),
                correlation_id: self.request_id.clone(),
                pool_id: self.pool_id.clone(),
                endpoint_id: self.endpoint_id.clone(),
                provider_kind: self.provider_kind,
                kind: self.kind,
                chunk_index,
                first_chunk,
                chunk_at,
                ttft_ms,
                max_inter_chunk_ms,
                metadata: self.metadata.clone(),
            },
        )
        .await;
    }

    fn build_report(
        &self,
        finished_at: SystemTime,
        outcome: StreamOutcome,
        error: Option<&GatewayError>,
    ) -> StreamReport {
        let state = self.inner.lock().expect("stream observation lock");
        StreamReport {
            request_id: self.request_id.clone(),
            correlation_id: self.request_id.clone(),
            pool_id: self.pool_id.clone(),
            endpoint_id: self.endpoint_id.clone(),
            provider_kind: self.provider_kind,
            kind: self.kind,
            started_at: self.started_at,
            first_chunk_at: state.first_chunk_at,
            finished_at,
            latency_ms: finished_at
                .duration_since(self.started_at)
                .unwrap_or_default()
                .as_millis() as u64,
            ttft_ms: state.first_chunk_ttft_ms,
            max_inter_chunk_ms: state.max_inter_chunk_ms,
            chunk_count: state.chunk_count,
            outcome,
            error: error.map(ToString::to_string),
            error_kind: error.map(GatewayError::kind),
            metadata: self.metadata.clone(),
        }
    }

    pub(super) fn mark_drained(&self) {
        self.drained_flag.store(true, Ordering::Release);
        self.drained.notify_waiters();
    }

    pub(super) async fn wait_until_drained(&self) {
        if self.drained_flag.load(Ordering::Acquire) {
            return;
        }
        self.drained.notified().await;
    }
}

pub(super) fn new_shared_stream_state(context: &StreamingAttemptContext) -> SharedStreamState {
    SharedStreamState {
        request_id: context.request_id.clone(),
        pool_id: context.pool_id.clone(),
        endpoint_id: context.endpoint_id.clone(),
        provider_kind: context.provider_kind,
        kind: context.stream_kind,
        started_at: context.attempt_started_at_system_time,
        attempt_started_at: context.attempt_started_at,
        metadata: context.metadata.clone(),
        hooks: context.hooks.clone(),
        inner: Arc::new(Mutex::new(StreamObservationState::default())),
        drained: Arc::new(tokio::sync::Notify::new()),
        drained_flag: Arc::new(AtomicBool::new(false)),
    }
}

pub(super) fn is_saturation_error(error: &GatewayError) -> bool {
    let terminal = error.terminal_error();
    match terminal {
        GatewayError::UpstreamHttp { status, .. }
            if *status == 429 || *status >= 502 && *status <= 504 =>
        {
            true
        }
        GatewayError::Transport { .. } | GatewayError::StreamAborted { .. } => true,
        _ => false,
    }
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

pub(super) fn with_streaming_attempt_reports<Chunk, Final>(
    streaming: StreamingResponse<Chunk, Final>,
    context: StreamingAttemptContext,
    shared_stream_state: SharedStreamState,
) -> StreamingResponse<Chunk, Final>
where
    Chunk: Send + 'static,
    Final: Send + 'static,
{
    let StreamingResponse {
        stream,
        completion,
        request_id: _,
        request_metadata,
    } = streaming;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let response_request_id = context.request_id.clone();

    tokio::spawn(async move {
        let StreamingAttemptContext {
            request_id,
            pool_id,
            endpoint_id,
            provider_kind,
            request_kind,
            stream_kind: _,
            request_started_at,
            attempt_started_at_system_time: _,
            attempt_started_at,
            metadata,
            previous_attempts,
            hooks,
            aimd,
            aimd_guard: _aimd_guard,
        } = context;

        let completion_result = completion.await.unwrap_or_else(|_| {
            Err(GatewayError::Transport {
                message: "stream completion channel dropped".to_string(),
                endpoint_id: None,
            })
        });

        let result = match completion_result {
            Ok(mut completed) => {
                shared_stream_state.wait_until_drained().await;
                let latency = Duration::from_millis(completed.report.latency_ms);
                let mut attempts = previous_attempts;
                attempts.push(success_attempt_report(&endpoint_id, latency));
                completed.report.request_id = request_id.clone();
                completed.report.correlation_id = request_id.clone();
                completed.report.attempts = attempts;
                completed.report.kind = request_kind;

                let stream_report = shared_stream_state.build_report(
                    completed.report.finished_at,
                    StreamOutcome::Completed,
                    None,
                );
                completed.report.stream = Some(stream_report.clone());

                emit_attempt_finished_hook(
                    hooks.clone(),
                    success_attempt_event(
                        &request_id,
                        pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                    ),
                )
                .await;
                emit_stream_completed_hook(hooks.clone(), stream_report).await;
                emit_request_finished_hook(hooks, completed.report.clone()).await;

                aimd.on_success();

                Ok(completed)
            }
            Err(error) => {
                shared_stream_state.wait_until_drained().await;
                let latency = attempt_started_at.elapsed();
                let mut attempts = previous_attempts;
                attempts.push(failed_attempt_report(&endpoint_id, latency, &error, false));

                let finished_at = SystemTime::now();
                let stream_report = shared_stream_state.build_report(
                    finished_at,
                    StreamOutcome::Aborted,
                    Some(&error),
                );

                emit_attempt_finished_hook(
                    hooks.clone(),
                    failed_attempt_event(
                        &request_id,
                        pool_id.as_deref(),
                        &endpoint_id,
                        provider_kind,
                        latency,
                        &error,
                    ),
                )
                .await;
                emit_stream_aborted_hook(hooks.clone(), stream_report.clone()).await;
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
                        finished_at,
                        request_kind,
                        Some(stream_report),
                        Some(error.kind()),
                    ),
                )
                .await;

                if is_saturation_error(&error) {
                    aimd.on_saturation();
                }

                Err(aggregate_attempt_failure(attempts, error))
            }
        };

        let _ = sender.send(result);
    });

    StreamingResponse {
        stream,
        completion: receiver,
        request_id: response_request_id,
        request_metadata,
    }
}
