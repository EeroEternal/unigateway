#![warn(missing_docs)]
//! Core library for UniGateway.
//!
//! Provides the core abstraction for routing, retries, and provider execution.

#[allow(missing_docs)]
pub mod conversion;
/// Traits and types defining integration with external API providers.
pub mod drivers;
/// High-level core engine and execution context structs.
pub mod engine;
/// Error types specific to the gateway's execution and network layer.
pub mod error;
/// Neutral runtime feedback abstractions for endpoint ordering.
pub mod feedback;
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
pub use error::{GatewayError, GatewayErrorKind};
pub use feedback::{EndpointSignal, RoutingFeedback, RoutingFeedbackProvider};
pub use hooks::{
    AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestStartedEvent, StreamChunkEvent,
    StreamStartedEvent,
};
pub use pool::{
    DriverId, Endpoint, EndpointId, EndpointRef, ExecutionPlan, ExecutionTarget, ModelPolicy,
    PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId, SecretString,
};
pub use registry::InMemoryDriverRegistry;
pub use request::{
    CLIENT_PROTOCOL_KEY, ClientProtocol, ContentBlock, Message, MessageRole,
    OPENAI_RAW_MESSAGES_KEY, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
    StructuredMessage, THINKING_SIGNATURE_PLACEHOLDER_VALUE, THINKING_SIGNATURE_STATUS_KEY,
    ThinkingSignatureStatus, anthropic_content_to_blocks, anthropic_messages_to_openai_messages,
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
    content_blocks_to_anthropic, content_blocks_to_anthropic_request,
    is_placeholder_thinking_signature, openai_message_to_content_blocks,
    openai_messages_to_anthropic_messages, openai_tool_choice_to_anthropic_tool_choice,
    openai_tools_to_anthropic_tools, validate_anthropic_request_messages,
};
pub use response::{
    AttemptReport, AttemptStatus, ChatResponseChunk, ChatResponseFinal, CompletedResponse,
    CompletionHandle, EmbeddingsResponse, ProxySession, RequestKind, RequestReport, ResponseStream,
    ResponsesEvent, ResponsesFinal, StreamKind, StreamOutcome, StreamReport, StreamingResponse,
    TokenUsage,
};
pub use retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};
