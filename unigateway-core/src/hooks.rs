use std::collections::HashMap;

use futures_util::future::BoxFuture;

use crate::pool::{EndpointId, PoolId, RequestId};
use crate::response::RequestReport;

/// Telemetry and lifecycle hooks for the gateway engine.
///
/// Implement this trait to intercept and log individual upstream attempts and
/// overall proxied request results.
///
/// # Example
/// ```rust
/// use futures_util::future::BoxFuture;
/// use unigateway_core::{GatewayHooks, AttemptStartedEvent, AttemptFinishedEvent, RequestReport};
///
/// pub struct MyHooks;
/// impl GatewayHooks for MyHooks {
///     fn on_attempt_started(&self, _event: AttemptStartedEvent) -> BoxFuture<'static, ()> {
///         Box::pin(async move { /* log attempt started */ })
///     }
///     fn on_attempt_finished(&self, _event: AttemptFinishedEvent) -> BoxFuture<'static, ()> {
///         Box::pin(async move { /* log attempt finished */ })
///     }
///     fn on_request_finished(&self, _report: RequestReport) -> BoxFuture<'static, ()> {
///         Box::pin(async move { /* log request finished */ })
///     }
/// }
/// ```
pub trait GatewayHooks: Send + Sync + 'static {
    /// Fired right before an upstream HTTP driver execution begins.
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()>;

    /// Fired immediately after an upstream driver execution returns, successfully or not.
    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()>;

    /// Fired when the proxy session completes, successfully or fatally.
    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()>;
}

/// Snapshot of context when a driver attempt begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptStartedEvent {
    /// The unique request transaction ID
    pub request_id: RequestId,
    /// The local pool ID serving this request
    pub pool_id: Option<PoolId>,
    /// The chosen target endpoint ID
    pub endpoint_id: EndpointId,
    /// The zero-indexed retry attempt number
    pub attempt_index: usize,
    /// Metadata inherited from the service/provider configurations
    pub metadata: HashMap<String, String>,
}

/// Snapshot of the final status of a single driver attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptFinishedEvent {
    /// The unique request transaction ID
    pub request_id: RequestId,
    /// The selected endpoint ID
    pub endpoint_id: EndpointId,
    /// Whether the attempt yielded a completed or streaming result
    pub success: bool,
    /// HTTP status code upstream returned (if an upstream error occurred)
    pub status_code: Option<u16>,
    /// Request latency in milliseconds
    pub latency_ms: u64,
    /// Error tracing string if the attempt failed
    pub error: Option<String>,
}
