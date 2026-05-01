use std::collections::HashMap;
use std::time::SystemTime;

use futures_util::future::BoxFuture;

use crate::ProviderKind;
use crate::error::GatewayErrorKind;
use crate::pool::{EndpointId, PoolId, RequestId};
use crate::request::ProxyChatRequest;
use crate::response::{ChatResponseChunk, RequestKind, RequestReport, StreamKind, StreamReport};

/// Telemetry and lifecycle hooks for the gateway engine.
///
/// Implement this trait to intercept and log individual upstream attempts and
/// overall proxied request results.
///
/// # Example
/// ```rust
/// use futures_util::future::BoxFuture;
/// use unigateway_core::{
///     AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestReport,
/// };
///
/// pub struct MyHooks;
/// impl GatewayHooks for MyHooks {
///     fn on_request_started(&self, _event: unigateway_core::RequestStartedEvent) -> BoxFuture<'static, ()> {
///         Box::pin(async move { /* log request started */ })
///     }
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
    /// Fired after the request receives a correlation ID and resolves its initial execution snapshot.
    ///
    /// `started_at` captures the engine-entry timestamp, which may be slightly earlier than the
    /// moment this hook fires.
    fn on_request_started(&self, _event: RequestStartedEvent) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Fired right before an upstream HTTP driver execution begins.
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()>;

    /// Fired immediately after an upstream driver execution returns, successfully or not.
    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()>;

    /// Fired when the proxy session completes, successfully or fatally.
    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()>;

    /// Fired after the engine enters streaming mode for an upstream attempt.
    fn on_stream_started(&self, _event: StreamStartedEvent) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Fired for each emitted streaming chunk with stable context.
    fn on_stream_chunk_event(&self, _event: StreamChunkEvent) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Fired when a streaming attempt completes normally.
    fn on_stream_completed(&self, _report: StreamReport) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Fired when a streaming attempt aborts before normal completion.
    fn on_stream_aborted(&self, _report: StreamReport) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Called before a chat request is executed.
    ///
    /// Allows modifying the request (e.g. injecting headers, rewriting the model name,
    /// or attaching trace metadata) before it is sent to the upstream driver.
    ///
    /// The default implementation is a no-op.
    fn on_request(&self, _req: &mut ProxyChatRequest) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    /// Called for each chunk in a streaming chat response.
    ///
    /// Useful for streaming observability, metrics collection, or auditing
    /// without modifying the chunk itself.
    ///
    /// The default implementation is a no-op.
    fn on_stream_chunk(&self, _chunk: &ChatResponseChunk) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
}

/// Snapshot of context when a driver attempt begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptStartedEvent {
    /// The unique request transaction ID
    pub request_id: RequestId,
    /// Stable correlation identifier; currently identical to `request_id`.
    pub correlation_id: RequestId,
    /// The local pool ID serving this request
    pub pool_id: Option<PoolId>,
    /// The chosen target endpoint ID
    pub endpoint_id: EndpointId,
    /// Provider kind backing the selected endpoint.
    pub provider_kind: ProviderKind,
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
    /// Stable correlation identifier; currently identical to `request_id`.
    pub correlation_id: RequestId,
    /// The local pool ID serving this request.
    pub pool_id: Option<PoolId>,
    /// The selected endpoint ID
    pub endpoint_id: EndpointId,
    /// Provider kind backing the selected endpoint.
    pub provider_kind: ProviderKind,
    /// Whether the attempt yielded a completed or streaming result
    pub success: bool,
    /// HTTP status code upstream returned (if an upstream error occurred)
    pub status_code: Option<u16>,
    /// Request latency in milliseconds
    pub latency_ms: u64,
    /// Error tracing string if the attempt failed
    pub error: Option<String>,
    /// Stable classification of the terminal attempt failure.
    pub error_kind: Option<GatewayErrorKind>,
}

/// Snapshot of request-scoped context after correlation ID assignment and routing resolution.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestStartedEvent {
    pub request_id: RequestId,
    pub correlation_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub kind: RequestKind,
    pub streaming: bool,
    pub started_at: SystemTime,
    pub metadata: HashMap<String, String>,
}

/// Snapshot emitted when a request enters streaming mode for an upstream attempt.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamStartedEvent {
    pub request_id: RequestId,
    pub correlation_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub kind: StreamKind,
    pub started_at: SystemTime,
    pub metadata: HashMap<String, String>,
}

/// Snapshot emitted for every streaming chunk with stable context and derived latency metrics.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamChunkEvent {
    pub request_id: RequestId,
    pub correlation_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub kind: StreamKind,
    pub chunk_index: u64,
    pub first_chunk: bool,
    pub chunk_at: SystemTime,
    pub ttft_ms: Option<u64>,
    pub max_inter_chunk_ms: Option<u64>,
    pub metadata: HashMap<String, String>,
}
