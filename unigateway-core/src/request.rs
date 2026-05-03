use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
