#![warn(missing_docs)]
//! Core library for UniGateway.
//!
//! Provides the core abstraction for routing, retries, and provider execution.

#[allow(missing_docs)]
pub mod capabilities;
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
pub mod responses_retry;
#[allow(missing_docs)]
pub mod retry;
#[allow(missing_docs)]
pub mod routing;
#[allow(missing_docs)]
pub mod transport;

pub use capabilities::{
    AnthropicThinkingOutputPolicy, EndpointCapabilities,
    OPENAI_CHAT_TOOLS_WITH_REASONING_EFFORT_KEY, OPENAI_RESPONSES_OPTIONAL_TOOLS_RETRY_KEY,
    OPENAI_RESPONSES_TOOLS_WITH_REASONING_KEY, OpenAiApiSurfaceCapabilities, ReasoningCapabilities,
    ToolCallingCapabilities, ToolChoiceDowngradeTarget, ToolChoiceMode,
};
pub use conversion::{
    normalize_proxy_responses_request, proxy_responses_request_uses_tools_and_reasoning,
};
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
    ANTHROPIC_THINKING_OUTPUT_KEY, CLIENT_PROTOCOL_KEY, ClientProtocol, ContentBlock, Message,
    MessageRole, OPENAI_RAW_MESSAGES_KEY, ProxyChatRequest, ProxyEmbeddingsRequest,
    ProxyResponsesRequest, StructuredMessage, THINKING_SIGNATURE_PLACEHOLDER_VALUE,
    THINKING_SIGNATURE_STATUS_KEY, TOOL_CHOICE_FORCE_OVERRIDE_KEY, TOOL_CHOICE_NORMALIZED_KEY,
    TOOL_CHOICE_ORIGINAL_KEY, TOOL_CHOICE_REACTIVE_RETRY_KEY, TOOL_CHOICE_REASON_KEY,
    ThinkingSignatureStatus, ToolChoiceNormalization, UpstreamToolChoiceProtocol,
    anthropic_content_to_blocks, anthropic_messages_to_openai_messages,
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
    apply_reactive_tool_choice_override, classify_anthropic_tool_choice,
    classify_openai_tool_choice, content_blocks_to_anthropic, content_blocks_to_anthropic_request,
    is_placeholder_thinking_signature, is_tool_choice_upstream_rejection, normalize_tool_choice,
    openai_message_to_anthropic_content_blocks,
    openai_message_to_anthropic_content_blocks_with_policy, openai_message_to_content_blocks,
    openai_messages_to_anthropic_messages, openai_tool_choice_to_anthropic_tool_choice,
    openai_tools_to_anthropic_tools, reactive_tool_choice_fallback,
    record_tool_choice_normalization, resolve_upstream_tool_choice, sent_tool_choice_from_metadata,
    validate_anthropic_request_messages,
};
pub use response::{
    AttemptReport, AttemptStatus, ChatResponseChunk, ChatResponseFinal, CompletedResponse,
    CompletionHandle, EmbeddingsResponse, ProxySession, RequestKind, RequestReport, ResponseStream,
    ResponsesEvent, ResponsesFinal, StreamKind, StreamOutcome, StreamReport, StreamingResponse,
    TokenUsage,
};
pub use responses_retry::should_retry_responses_without_tools;
pub use retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};
