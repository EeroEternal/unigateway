#![warn(missing_docs)]
//! Core library for UniGateway.
//!
//! Provides the core abstraction for routing, retries, and provider execution.

/// Traits and types defining integration with external API providers.
pub mod drivers;
/// High-level core engine and execution context structs.
pub mod engine;
/// Error types specific to the gateway's execution and network layer.
pub mod error;
/// Hooks and telemetry definitions for capturing application lifecycle events.
pub mod hooks;
#[allow(missing_docs)]
pub mod pool;
#[allow(missing_docs)]
pub mod protocol;
#[allow(missing_docs)]
pub mod registry;
#[allow(missing_docs)]
pub mod request;
#[allow(missing_docs)]
pub mod response;
#[allow(missing_docs)]
pub mod retry;
#[allow(missing_docs)]
pub mod routing;
#[allow(missing_docs)]
pub mod transport;

pub use drivers::{DriverEndpointContext, DriverRegistry, ProviderDriver};
pub use engine::{UniGatewayEngine, UniGatewayEngineBuilder};
pub use error::GatewayError;
pub use hooks::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks};
pub use pool::{
    DriverId, Endpoint, EndpointId, EndpointRef, ExecutionPlan, ExecutionTarget, ModelPolicy,
    PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId, SecretString,
};
pub use registry::InMemoryDriverRegistry;
pub use request::{
    Message, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
pub use response::{
    AttemptReport, AttemptStatus, ChatResponseChunk, ChatResponseFinal, CompletedResponse,
    CompletionHandle, EmbeddingsResponse, ProxySession, RequestReport, ResponseStream,
    ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};
pub use retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};
