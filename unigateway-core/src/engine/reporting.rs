use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::error::GatewayError;
use crate::hooks::{AttemptFinishedEvent, GatewayHooks};
use crate::pool::{PoolId, ProviderKind, RequestId};
use crate::response::{
    AttemptReport, AttemptStatus, CompletedResponse, RequestReport, StreamingResponse,
};
use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

use super::FailedRequestContext;

pub(super) fn success_attempt_report(endpoint_id: &str, latency: Duration) -> AttemptReport {
    AttemptReport {
        endpoint_id: endpoint_id.to_string(),
        status: AttemptStatus::Succeeded,
        latency_ms: latency.as_millis() as u64,
        error: None,
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
    }
}

pub(super) fn success_attempt_event(
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

pub(super) fn failed_attempt_event(
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

pub(super) fn build_failed_request_report(
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

pub(super) fn with_completed_request_report<T>(
    mut response: CompletedResponse<T>,
    request_id: &str,
    attempts: Vec<AttemptReport>,
) -> CompletedResponse<T> {
    response.report.request_id = request_id.to_string();
    response.report.attempts = attempts;
    response
}

pub(super) struct StreamingAttemptContext {
    pub(super) request_id: RequestId,
    pub(super) pool_id: Option<PoolId>,
    pub(super) endpoint_id: String,
    pub(super) provider_kind: ProviderKind,
    pub(super) request_started_at: SystemTime,
    pub(super) attempt_started_at: Instant,
    pub(super) metadata: std::collections::HashMap<String, String>,
    pub(super) previous_attempts: Vec<AttemptReport>,
    pub(super) hooks: Option<Arc<dyn GatewayHooks>>,
    pub(super) aimd: Arc<crate::engine::AdaptiveConcurrency>,
    pub(super) aimd_guard: Option<crate::engine::aimd::AimdGuard>,
}

pub(super) fn is_saturation_error(error: &GatewayError) -> bool {
    let terminal = error.terminal_error();
    match terminal {
        GatewayError::UpstreamHttp { status, .. } if *status == 429 || *status >= 502 && *status <= 504 => true,
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

                aimd.on_success();

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
    }
}
