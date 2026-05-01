use std::collections::HashMap;
use std::pin::Pin;
use std::time::SystemTime;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::error::GatewayError;
use crate::error::GatewayErrorKind;
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

#[allow(clippy::large_enum_variant)]
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

impl<Chunk, Final> StreamingResponse<Chunk, Final> {
    /// Drops the output stream and waits for the terminal completion result.
    ///
    /// Use this when the caller no longer intends to read additional chunks but still needs the
    /// final response payload or request report.
    ///
    /// This is also the preferred way to stop consuming a streaming response early. Leaving the
    /// receiver alive without draining it can keep buffering upstream output until the stream
    /// finishes.
    pub async fn into_completion(self) -> Result<CompletedResponse<Final>, GatewayError> {
        let Self {
            stream: _stream,
            completion,
            request_id: _,
            request_metadata: _,
        } = self;

        completion.await.map_err(|_| GatewayError::Transport {
            message: "stream completion channel dropped".to_string(),
            endpoint_id: None,
        })?
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestKind {
    Chat,
    Responses,
    Embeddings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    Chat,
    Responses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamOutcome {
    Completed,
    Aborted,
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
    pub error_kind: Option<GatewayErrorKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamReport {
    pub request_id: RequestId,
    pub correlation_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub kind: StreamKind,
    pub started_at: SystemTime,
    pub first_chunk_at: Option<SystemTime>,
    pub finished_at: SystemTime,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub max_inter_chunk_ms: Option<u64>,
    pub chunk_count: u64,
    pub outcome: StreamOutcome,
    pub error: Option<String>,
    pub error_kind: Option<GatewayErrorKind>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestReport {
    pub request_id: RequestId,
    pub correlation_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub selected_endpoint_id: EndpointId,
    pub selected_provider: ProviderKind,
    pub kind: RequestKind,
    pub attempts: Vec<AttemptReport>,
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub error_kind: Option<GatewayErrorKind>,
    pub stream: Option<StreamReport>,
    pub metadata: HashMap<String, String>,
}
