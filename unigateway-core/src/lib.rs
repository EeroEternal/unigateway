#![warn(missing_docs)]
//! Core library for UniGateway.
//!
//! Provides the core abstraction for routing, retries, and provider execution.
//!
//! With `--no-default-features`, only the conversion surface is linked: request/response
//! types, protocol builders/parsers, and [`DriverEndpointContext`]. The in-process engine,
//! drivers, routing, and hooks require the `engine` feature (enabled by default).

#[allow(missing_docs)]
pub mod capabilities;
#[allow(missing_docs)]
pub mod conversion;
/// Endpoint context passed to request builders (always available).
pub mod endpoint_context;
/// Error types specific to the gateway's execution and network layer.
pub mod error;
#[allow(missing_docs)]
pub mod metadata_headers;
#[allow(missing_docs)]
pub mod pool;
#[allow(missing_docs)]
pub mod protocol;
#[allow(missing_docs)]
pub mod request;
#[allow(missing_docs)]
pub mod response;
#[allow(missing_docs)]
pub mod responses_retry;
#[allow(missing_docs)]
pub mod retry;
#[allow(missing_docs)]
pub mod transport;

#[cfg(feature = "drivers")]
/// Traits and types defining integration with external API providers.
pub mod drivers;

#[cfg(feature = "engine")]
/// High-level core engine and execution context structs.
pub mod engine;
#[cfg(feature = "engine")]
/// Neutral runtime feedback abstractions for endpoint ordering.
pub mod feedback;
#[cfg(feature = "engine")]
/// Hooks and telemetry definitions for capturing application lifecycle events.
pub mod hooks;
#[cfg(feature = "engine")]
#[allow(missing_docs)]
pub mod registry;
#[cfg(feature = "engine")]
#[allow(missing_docs)]
pub mod routing;

pub use capabilities::{
    AnthropicThinkingOutputPolicy, EndpointCapabilities,
    OPENAI_CHAT_TOOLS_WITH_REASONING_EFFORT_KEY, OPENAI_RESPONSES_OPTIONAL_TOOLS_RETRY_KEY,
    OPENAI_RESPONSES_TOOLS_WITH_REASONING_KEY, OpenAiApiSurfaceCapabilities, ReasoningCapabilities,
    ToolCallingCapabilities, ToolChoiceDowngradeTarget, ToolChoiceMode,
};
pub use conversion::{
    normalize_proxy_responses_request, proxy_responses_request_uses_tools_and_reasoning,
};
pub use endpoint_context::DriverEndpointContext;
pub use error::{GatewayError, GatewayErrorKind};
pub use metadata_headers::{
    forward_metadata_as_http_headers, is_internal_metadata_key, is_valid_http_header_value,
    merge_forward_allowlists, metadata_key_matches_allowlist,
};
pub use pool::{
    DriverId, Endpoint, EndpointId, EndpointRef, ExecutionPlan, ExecutionTarget, ModelPolicy,
    PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId, SecretString,
};
pub use request::{
    ANTHROPIC_THINKING_OUTPUT_KEY, CLIENT_PROTOCOL_KEY, ClientProtocol, ContentBlock,
    LOCAL_INFERENCE_PREFIX_CACHING_KEY, Message, MessageRole, OPENAI_RAW_MESSAGES_KEY,
    ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest, StructuredMessage,
    THINKING_SIGNATURE_PLACEHOLDER_VALUE, THINKING_SIGNATURE_STATUS_KEY,
    TOOL_CHOICE_FORCE_OVERRIDE_KEY, TOOL_CHOICE_NORMALIZED_KEY, TOOL_CHOICE_ORIGINAL_KEY,
    TOOL_CHOICE_REACTIVE_RETRY_KEY, TOOL_CHOICE_REASON_KEY, ThinkingSignatureStatus,
    ToolChoiceNormalization, UpstreamToolChoiceProtocol, anthropic_content_to_blocks,
    anthropic_messages_to_openai_messages, anthropic_tool_choice_to_openai_tool_choice,
    anthropic_tools_to_openai_tools, apply_reactive_tool_choice_override,
    classify_anthropic_tool_choice, classify_openai_tool_choice, content_blocks_to_anthropic,
    content_blocks_to_anthropic_request, is_gateway_only_field_key,
    is_placeholder_thinking_signature, is_tool_choice_upstream_rejection, merge_forwardable_extra,
    normalize_tool_choice, openai_message_to_anthropic_content_blocks,
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

/// Stable re-export of the Anthropic chat request builder (conversion surface).
pub use protocol::anthropic::build_chat_request as build_anthropic_chat_request;
/// Stable re-export of the OpenAI-compatible chat request builder (conversion surface).
pub use protocol::openai::build_chat_request as build_openai_chat_request;

#[cfg(feature = "drivers")]
pub use drivers::{DriverRegistry, ProviderDriver};

#[cfg(feature = "engine")]
pub use engine::{UniGatewayEngine, UniGatewayEngineBuilder};
#[cfg(feature = "engine")]
pub use feedback::{EndpointSignal, RoutingFeedback, RoutingFeedbackProvider};
#[cfg(feature = "engine")]
pub use hooks::{
    AttemptFinishedEvent, AttemptSkipReason, AttemptSkippedEvent, AttemptStartedEvent,
    GatewayHooks, RequestStartedEvent, StreamChunkEvent, StreamStartedEvent,
};
#[cfg(feature = "engine")]
pub use registry::InMemoryDriverRegistry;

#[cfg(feature = "sglang-lite")]
pub use protocol::sglang_lite::{
    BACKEND_MODE_KEY, DEVICE_KEY, DRIVER_ID, MAX_BATCH_SIZE_KEY, MODEL_PATH_KEY, PYTHON_ENV_KEY,
    SUBPROCESS_ARGS_KEY, SUBPROCESS_COMMAND_KEY, SUBPROCESS_HEALTH_PATH_KEY,
    SUBPROCESS_STARTUP_TIMEOUT_MS_KEY, SglangLiteBackend, SglangLiteDriver,
    SglangLiteSubprocessConfig,
};
