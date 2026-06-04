mod blocks;
mod messages;
mod responses_fields;
mod tool_calls;
mod tool_choice_normalize;
mod tools;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tool_choice_normalize_tests;

pub use blocks::{
    anthropic_content_to_blocks, content_blocks_to_anthropic, content_blocks_to_anthropic_request,
    is_placeholder_thinking_signature, openai_message_to_anthropic_content_blocks,
    openai_message_to_anthropic_content_blocks_with_policy, openai_message_to_content_blocks,
};
pub use messages::{
    anthropic_messages_to_openai_messages, openai_messages_to_anthropic_messages,
    validate_anthropic_request_messages,
};
pub use responses_fields::{
    normalize_proxy_responses_request, proxy_responses_request_uses_tools_and_reasoning,
};
pub use tool_calls::{
    AnthropicInputJsonDelta, AnthropicToolUseStart, OpenAiToolCallDeltaUpdate,
    OpenAiToolCallStopUpdate, PendingOpenAiToolCall, apply_openai_tool_call_delta_update,
    flush_openai_tool_call_stop_update,
};
pub use tool_choice_normalize::{
    ToolChoiceNormalization, UpstreamToolChoiceProtocol, apply_reactive_tool_choice_override,
    classify_anthropic_tool_choice, classify_openai_tool_choice, is_tool_choice_upstream_rejection,
    normalize_tool_choice, reactive_tool_choice_fallback, record_tool_choice_normalization,
    resolve_upstream_tool_choice, sent_tool_choice_from_metadata,
};
pub use tools::{
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
    openai_tool_choice_to_anthropic_tool_choice, openai_tools_to_anthropic_tools,
};
