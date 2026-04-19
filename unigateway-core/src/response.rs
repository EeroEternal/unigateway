use std::collections::HashMap;
use std::pin::Pin;
use std::time::SystemTime;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::error::GatewayError;
use crate::pool::{EndpointId, PoolId, ProviderKind, RequestId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponseChunk {
    pub delta: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponseFinal {
    pub model: Option<String>,
    pub output_text: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesFinal {
    pub output_text: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingsResponse {
    pub raw: serde_json::Value,
}

pub type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, GatewayError>> + Send + 'static>>;

pub type CompletionHandle<T> =
    tokio::sync::oneshot::Receiver<Result<CompletedResponse<T>, GatewayError>>;

pub enum ProxySession<Chunk, Final> {
    Completed(CompletedResponse<Final>),
    Streaming(StreamingResponse<Chunk, Final>),
}

pub struct CompletedResponse<T> {
    pub response: T,
    pub report: RequestReport,
}

pub struct StreamingResponse<Chunk, Final> {
    pub stream: ResponseStream<Chunk>,
    pub completion: CompletionHandle<Final>,
    pub request_id: RequestId,
    pub request_metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttemptStatus {
    Succeeded,
    Failed,
    Retried,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptReport {
    pub endpoint_id: EndpointId,
    pub status: AttemptStatus,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReport {
    pub request_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub selected_endpoint_id: EndpointId,
    pub selected_provider: ProviderKind,
    pub attempts: Vec<AttemptReport>,
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub metadata: HashMap<String, String>,
}
