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

/// The originating client protocol for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientProtocol {
    /// OpenAI Chat Completions-compatible request.
    OpenAiChat,
    /// Anthropic Messages-compatible request.
    AnthropicMessages,
    /// Neutral request constructed directly against UniGateway core types.
    Neutral,
}

impl ClientProtocol {
    /// Returns the metadata value used for this protocol.
    pub const fn as_metadata_value(self) -> &'static str {
        match self {
            Self::OpenAiChat => "openai_chat",
            Self::AnthropicMessages => "anthropic_messages",
            Self::Neutral => "neutral",
        }
    }

    /// Parses a protocol from a metadata value.
    pub fn from_metadata_value(value: &str) -> Option<Self> {
        match value {
            "openai_chat" => Some(Self::OpenAiChat),
            "anthropic_messages" => Some(Self::AnthropicMessages),
            "neutral" => Some(Self::Neutral),
            _ => None,
        }
    }
}

/// The semantic status of a thinking signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingSignatureStatus {
    /// No signature is present.
    Absent,
    /// A placeholder signature is present for protocol-shape compatibility only.
    Placeholder,
    /// A verbatim signature came from Anthropic-native content.
    Verbatim,
}

impl ThinkingSignatureStatus {
    /// Returns the metadata value used for this status.
    pub const fn as_metadata_value(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Placeholder => "placeholder",
            Self::Verbatim => "verbatim",
        }
    }

    /// Parses a thinking signature status from a metadata value.
    pub fn from_metadata_value(value: &str) -> Option<Self> {
        match value {
            "absent" => Some(Self::Absent),
            "placeholder" => Some(Self::Placeholder),
            "verbatim" => Some(Self::Verbatim),
            _ => None,
        }
    }
}

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
    /// Image content represented using an Anthropic-compatible source descriptor.
    Image {
        source: Value,
        detail: Option<String>,
    },
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
    ToolResult { tool_use_id: String, content: Value },
}

/// Message role in a chat completion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Creates a text-only message.
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
        }
    }

    /// Creates a message from ordered content blocks.
    pub fn from_blocks(role: MessageRole, content: Vec<ContentBlock>) -> Self {
        Self { role, content }
    }

    /// Returns the concatenated text content, ignoring non-text blocks.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Returns this block-first message using the migration alias.
    pub fn to_structured(&self) -> StructuredMessage {
        self.clone()
    }

    /// Returns this block-first message using the migration alias.
    pub fn into_structured(self) -> StructuredMessage {
        self
    }
}

/// Compatibility alias for the block-first chat message shape.
pub type StructuredMessage = Message;

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

impl ProxyChatRequest {
    /// Returns the typed client protocol semantics, if present.
    pub fn client_protocol(&self) -> Option<ClientProtocol> {
        self.metadata
            .get(CLIENT_PROTOCOL_KEY)
            .and_then(|value| ClientProtocol::from_metadata_value(value))
    }

    /// Writes the typed client protocol semantics to request metadata.
    pub fn set_client_protocol(&mut self, protocol: ClientProtocol) {
        self.metadata.insert(
            CLIENT_PROTOCOL_KEY.to_string(),
            protocol.as_metadata_value().to_string(),
        );
    }

    /// Returns whether this request preserves OpenAI raw messages.
    pub fn has_openai_raw_messages(&self) -> bool {
        self.metadata.contains_key(OPENAI_RAW_MESSAGES_KEY)
    }

    /// Marks this request as carrying OpenAI raw messages.
    pub fn mark_openai_raw_messages(&mut self) {
        self.metadata
            .insert(OPENAI_RAW_MESSAGES_KEY.to_string(), "true".to_string());
    }

    /// Returns the typed thinking signature status, if present.
    pub fn thinking_signature_status(&self) -> Option<ThinkingSignatureStatus> {
        self.metadata
            .get(THINKING_SIGNATURE_STATUS_KEY)
            .and_then(|value| ThinkingSignatureStatus::from_metadata_value(value))
    }

    /// Writes the typed thinking signature status to request metadata.
    pub fn set_thinking_signature_status(&mut self, status: ThinkingSignatureStatus) {
        self.metadata.insert(
            THINKING_SIGNATURE_STATUS_KEY.to_string(),
            status.as_metadata_value().to_string(),
        );
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        ClientProtocol, ContentBlock, Message, MessageRole, ProxyChatRequest, StructuredMessage,
        ThinkingSignatureStatus,
    };

    fn request() -> ProxyChatRequest {
        ProxyChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn proxy_chat_request_round_trips_client_protocol() {
        let mut request = request();
        request.set_client_protocol(ClientProtocol::OpenAiChat);

        assert_eq!(request.client_protocol(), Some(ClientProtocol::OpenAiChat));
    }

    #[test]
    fn legacy_message_converts_to_structured_text_message() {
        let message = Message::text(MessageRole::User, "hello");
        let structured = message.to_structured();

        assert_eq!(structured.role, MessageRole::User);
        assert_eq!(
            structured.content,
            vec![ContentBlock::Text {
                text: "hello".to_string()
            }]
        );
    }

    #[test]
    fn structured_message_preserves_ordered_blocks() {
        let structured = StructuredMessage::from_blocks(
            MessageRole::Assistant,
            vec![
                ContentBlock::Thinking {
                    thinking: "reasoning".to_string(),
                    signature: Some("real-signature".to_string()),
                },
                ContentBlock::Text {
                    text: "answer".to_string(),
                },
            ],
        );

        assert_eq!(structured.role, MessageRole::Assistant);
        assert_eq!(structured.text_content(), "answer");
        assert!(matches!(
            structured.content.first(),
            Some(ContentBlock::Thinking { signature: Some(signature), .. }) if signature == "real-signature"
        ));
    }

    #[test]
    fn proxy_chat_request_marks_openai_raw_messages() {
        let mut request = request();

        assert!(!request.has_openai_raw_messages());
        request.mark_openai_raw_messages();
        assert!(request.has_openai_raw_messages());
    }

    #[test]
    fn proxy_chat_request_round_trips_thinking_signature_status() {
        let mut request = request();
        request.set_thinking_signature_status(ThinkingSignatureStatus::Placeholder);

        assert_eq!(
            request.thinking_signature_status(),
            Some(ThinkingSignatureStatus::Placeholder)
        );
    }

    #[test]
    fn unknown_metadata_values_are_ignored_by_typed_helpers() {
        let mut request = request();
        request.metadata.insert(
            "unigateway.client_protocol".to_string(),
            "unknown".to_string(),
        );
        request.metadata.insert(
            "unigateway.thinking_signature_status".to_string(),
            "invalid".to_string(),
        );

        assert_eq!(request.client_protocol(), None);
        assert_eq!(request.thinking_signature_status(), None);
    }
}
