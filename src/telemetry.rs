use futures_util::future::BoxFuture;
use unigateway_core::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestReport};

#[derive(Clone)]
pub struct GatewayTelemetryHooks;

impl GatewayHooks for GatewayTelemetryHooks {
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            tracing::info!(
                request_id = %event.request_id,
                pool_id = ?event.pool_id,
                endpoint_id = %event.endpoint_id,
                attempt = event.attempt_index,
                "upstream attempt started"
            );
        })
    }

    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            if event.success {
                tracing::info!(
                    request_id = %event.request_id,
                    endpoint_id = %event.endpoint_id,
                    latency_ms = event.latency_ms,
                    "upstream attempt succeeded"
                );
            } else {
                tracing::warn!(
                    request_id = %event.request_id,
                    endpoint_id = %event.endpoint_id,
                    latency_ms = event.latency_ms,
                    status_code = ?event.status_code,
                    error = ?event.error,
                    "upstream attempt failed"
                );
            }
        })
    }

    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let error_count = report
                .attempts
                .iter()
                .filter(|a| !matches!(a.status, unigateway_core::AttemptStatus::Succeeded))
                .count();

            tracing::info!(
                request_id = %report.request_id,
                pool_id = ?report.pool_id,
                endpoint_id = %report.selected_endpoint_id,
                selected_provider = ?report.selected_provider,
                latency_ms = report.latency_ms,
                attempts = report.attempts.len(),
                errors = error_count,
                "proxy request finished"
            );
        })
    }
}
