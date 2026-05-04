use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Metadata key for marking OpenAI raw messages in ProxyChatRequest.
pub const OPENAI_RAW_MESSAGES_KEY: &str = "unigateway.openai_raw_messages";
/// Metadata key for recording the source client protocol.
pub const CLIENT_PROTOCOL_KEY: &str = "unigateway.client_protocol";
/// Metadata key for recording whether thinking signatures are real placeholders or absent.
pub const THINKING_SIGNATURE_STATUS_KEY: &str = "unigateway.thinking_signature_status";
/// Placeholder thinking signature used only for downstream protocol-shape compatibility.
pub const THINKING_SIGNATURE_PLACEHOLDER_VALUE: &str = "EXTENDED_THINKING_PLACEHOLDER_SIG";

pub use crate::conversion::{
    anthropic_content_to_blocks, anthropic_messages_to_openai_messages,
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
    content_blocks_to_anthropic, content_blocks_to_anthropic_request,
    is_placeholder_thinking_signature, openai_message_to_content_blocks,
    openai_messages_to_anthropic_messages, openai_tool_choice_to_anthropic_tool_choice,
    openai_tools_to_anthropic_tools, validate_anthropic_request_messages,
};

/// Structured content block for protocol-preserving chat messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },
    /// Anthropic thinking content with an optional continuation signature.
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    /// Tool use content block.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool result content block.
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// Message role in a chat completion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub max_tokens: Option<u32>,
    pub stop_sequences: Option<Value>,
    pub stream: bool,
    pub system: Option<Value>,
    pub tools: Option<Value>,
    pub tool_choice: Option<Value>,
    pub raw_messages: Option<Value>,
    pub extra: HashMap<String, Value>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyResponsesRequest {
    pub model: String,
    pub input: Option<serde_json::Value>,
    pub instructions: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub stream: bool,
    pub tools: Option<serde_json::Value>,
    pub tool_choice: Option<serde_json::Value>,
    pub previous_response_id: Option<String>,
    pub request_metadata: Option<serde_json::Value>,
    pub extra: HashMap<String, serde_json::Value>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyEmbeddingsRequest {
    pub model: String,
    pub input: Vec<String>,
    pub encoding_format: Option<String>,
    pub metadata: HashMap<String, String>,
}
