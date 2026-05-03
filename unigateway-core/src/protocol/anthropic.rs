use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::request::{
    MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    RequestKind, ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};
use crate::transport::{HttpTransport, TransportByteStream, TransportRequest};

use super::{build_request_report, drain_sse_frames, next_request_id, parse_sse_frame};

pub const DRIVER_ID: &str = "anthropic";

pub struct AnthropicDriver {
    transport: Arc<dyn HttpTransport>,
}

impl AnthropicDriver {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }
}

impl ProviderDriver for AnthropicDriver {
    fn driver_id(&self) -> &str {
        DRIVER_ID
    }

    fn provider_kind(&self) -> crate::ProviderKind {
        crate::ProviderKind::Anthropic
    }

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>
    {
        let transport = self.transport.clone();

        Box::pin(async move {
            if request.stream {
                return start_chat_stream(transport, endpoint, request).await;
            }

            let started_at = SystemTime::now();
            let transport_request = build_chat_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_chat_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(ProxySession::Completed(CompletedResponse {
                response: response_body,
                report: build_request_report(
                    &endpoint,
                    started_at,
                    finished_at,
                    usage,
                    RequestKind::Chat,
                    None,
                ),
            }))
        })
    }

    fn execute_responses(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>
    {
        Box::pin(async { Err(GatewayError::not_implemented("anthropic responses")) })
    }

    fn execute_embeddings(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>> {
        Box::pin(async { Err(GatewayError::not_implemented("anthropic embeddings")) })
    }
}

async fn start_chat_stream(
    transport: Arc<dyn HttpTransport>,
    endpoint: DriverEndpointContext,
    request: ProxyChatRequest,
) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let request_id = next_request_id();
    let started_at = SystemTime::now();
    let transport_request = build_chat_request(&endpoint, &request)?;
    let transport_response = transport.send_stream(transport_request).await?;
    let (chunk_tx, chunk_rx) = mpsc::unbounded_channel();
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
    let completion_request_id = request_id.clone();

    tokio::spawn(async move {
        let completion = drive_chat_stream(
            transport_response.stream,
            chunk_tx,
            endpoint,
            started_at,
            completion_request_id,
        )
        .await;
        let _ = completion_tx.send(completion);
    });

    Ok(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(UnboundedReceiverStream::new(chunk_rx)),
        completion: completion_rx,
        request_id,
        request_metadata: request.metadata.clone(),
    }))
}

#[derive(Default)]
struct AnthropicChatStreamState {
    model: Option<String>,
    output_text: String,
    usage: Option<TokenUsage>,
    raw_events: Vec<Value>,
    done: bool,
    downstream_detached: bool,
}

async fn drive_chat_stream(
    mut upstream: TransportByteStream,
    chunk_tx: mpsc::UnboundedSender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
) -> Result<CompletedResponse<ChatResponseFinal>, GatewayError> {
    let mut buffer = Vec::new();
    let mut state = AnthropicChatStreamState::default();

    while let Some(item) = upstream.next().await {
        match item {
            Ok(bytes) => {
                buffer.extend_from_slice(&bytes);
                for frame in drain_sse_frames(&mut buffer) {
                    process_chat_frame(&chunk_tx, &endpoint, &mut state, frame).await?;
                    if state.done {
                        return finalize_chat_stream(endpoint, started_at, request_id, state);
                    }
                }
            }
            Err(error) => {
                return fail_stream::<ChatResponseChunk, ChatResponseFinal>(
                    &chunk_tx,
                    &endpoint.endpoint_id,
                    error.to_string(),
                )
                .await;
            }
        }
    }

    if !buffer.is_empty()
        && let Some(frame) = parse_sse_frame(&buffer)
    {
        process_chat_frame(&chunk_tx, &endpoint, &mut state, frame).await?;
    }

    finalize_chat_stream(endpoint, started_at, request_id, state)
}

async fn process_chat_frame(
    chunk_tx: &mpsc::UnboundedSender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: &DriverEndpointContext,
    state: &mut AnthropicChatStreamState,
    frame: super::ParsedSseEvent,
) -> Result<(), GatewayError> {
    if frame.data == "[DONE]" {
        state.done = true;
        return Ok(());
    }

    let mut raw: Value =
        serde_json::from_str(&frame.data).map_err(|error| GatewayError::Transport {
            message: format!("failed to parse anthropic chat stream frame: {error}"),
            endpoint_id: Some(endpoint.endpoint_id.clone()),
        })?;

    let event_type = frame
        .event
        .or_else(|| raw.get("type").and_then(Value::as_str).map(str::to_string));
    if let Some(event_type) = event_type.clone()
        && let Some(object) = raw.as_object_mut()
    {
        object
            .entry("type".to_string())
            .or_insert_with(|| Value::String(event_type));
    }

    if state.model.is_none() {
        state.model = raw.get("model").and_then(Value::as_str).map(str::to_string);
    }

    if let Some(usage) = raw.get("usage").map(|usage| TokenUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: match (
            usage.get("input_tokens").and_then(Value::as_u64),
            usage.get("output_tokens").and_then(Value::as_u64),
        ) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        },
    }) {
        state.usage = Some(usage);
    }

    let delta = raw
        .get("delta")
        .and_then(|delta| delta.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string);

    if let Some(text) = delta.as_deref() {
        state.output_text.push_str(text);
    }

    state.raw_events.push(raw.clone());

    if !state.downstream_detached && chunk_tx.send(Ok(ChatResponseChunk { delta, raw })).is_err() {
        state.downstream_detached = true;
    }

    Ok(())
}

fn finalize_chat_stream(
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
    state: AnthropicChatStreamState,
) -> Result<CompletedResponse<ChatResponseFinal>, GatewayError> {
    let finished_at = SystemTime::now();
    Ok(CompletedResponse {
        response: ChatResponseFinal {
            model: state.model,
            output_text: if state.output_text.is_empty() {
                None
            } else {
                Some(state.output_text)
            },
            raw: Value::Array(state.raw_events),
        },
        report: build_request_report(
            &endpoint,
            started_at,
            finished_at,
            state.usage,
            RequestKind::Chat,
            Some(request_id),
        ),
    })
}

async fn fail_stream<Item, Final>(
    sender: &mpsc::UnboundedSender<Result<Item, GatewayError>>,
    endpoint_id: &str,
    message: String,
) -> Result<CompletedResponse<Final>, GatewayError> {
    let stream_error = GatewayError::StreamAborted {
        message: message.clone(),
        endpoint_id: endpoint_id.to_string(),
    };
    let _ = sender.send(Err(stream_error));
    Err(GatewayError::StreamAborted {
        message,
        endpoint_id: endpoint_id.to_string(),
    })
}

pub fn build_chat_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyChatRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut system_parts = Vec::new();
    let fallback_messages = request
        .messages
        .iter()
        .filter_map(|message| match message.role {
            MessageRole::System => {
                system_parts.push(message.content.clone());
                None
            }
            MessageRole::User => Some(json!({
                "role": "user",
                "content": message.content,
            })),
            MessageRole::Assistant => Some(json!({
                "role": "assistant",
                "content": message.content,
            })),
            MessageRole::Tool => None,
        })
        .collect::<Vec<_>>();

    let system = request.system.clone().or_else(|| {
        if system_parts.is_empty() {
            None
        } else {
            Some(Value::String(system_parts.join("\n")))
        }
    });
    let messages = request
        .raw_messages
        .clone()
        .unwrap_or(Value::Array(fallback_messages));

    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        ("messages".to_string(), messages),
        (
            "max_tokens".to_string(),
            json!(request.max_tokens.unwrap_or(1024)),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ]);

    if let Some(system) = system {
        payload.insert("system".to_string(), system);
    }
    if let Some(temperature) = request.temperature {
        payload.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        payload.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = request.top_k {
        payload.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(stop_sequences) = request.stop_sequences.clone() {
        payload.insert("stop_sequences".to_string(), stop_sequences);
    }
    if let Some(toools) = request.tools.clone() {
        payload.insert("tools".to_string(), toools);
    }
    if let Some(tool_choice) = request.tool_choice.clone() {
        payload.insert("tool_choice".to_string(), tool_choice);
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "messages"),
        anthropic_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

pub fn parse_chat_response(
    body: &[u8],
) -> Result<(ChatResponseFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse anthropic chat response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty());

    let usage = raw.get("usage").map(|usage| TokenUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: match (
            usage.get("input_tokens").and_then(Value::as_u64),
            usage.get("output_tokens").and_then(Value::as_u64),
        ) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        },
    });

    Ok((
        ChatResponseFinal {
            model: raw.get("model").and_then(Value::as_str).map(str::to_string),
            output_text,
            raw,
        },
        usage,
    ))
}

fn anthropic_headers(endpoint: &DriverEndpointContext) -> HashMap<String, String> {
    HashMap::from([
        (
            "x-api-key".to_string(),
            endpoint.api_key.expose_secret().to_string(),
        ),
        ("anthropic-version".to_string(), "2023-06-01".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ])
}

fn resolved_model(endpoint: &DriverEndpointContext, requested_model: &str) -> String {
    endpoint
        .model_policy
        .model_mapping
        .get(requested_model)
        .cloned()
        .or_else(|| endpoint.model_policy.default_model.clone())
        .unwrap_or_else(|| requested_model.to_string())
}

fn join_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use futures_util::StreamExt;
    use futures_util::future::BoxFuture;
    use serde_json::json;

    use super::{AnthropicDriver, build_chat_request};
    use crate::GatewayError;
    use crate::drivers::{DriverEndpointContext, ProviderDriver};
    use crate::pool::{ModelPolicy, ProviderKind, SecretString};
    use crate::request::{Message, MessageRole, ProxyChatRequest};
    use crate::response::ProxySession;
    use crate::transport::{
        HttpTransport, StreamingTransportResponse, TransportRequest, TransportResponse,
    };

    struct MockTransport {
        response: Option<TransportResponse>,
        stream_chunks: Option<Vec<Vec<u8>>>,
        seen: Arc<Mutex<Vec<TransportRequest>>>,
    }

    impl HttpTransport for MockTransport {
        fn send(
            &self,
            request: TransportRequest,
        ) -> BoxFuture<'static, Result<TransportResponse, crate::GatewayError>> {
            let seen = self.seen.clone();
            let response = self.response.clone().expect("missing non-stream response");
            Box::pin(async move {
                seen.lock().expect("seen lock").push(request);
                Ok(response)
            })
        }

        fn send_stream(
            &self,
            request: TransportRequest,
        ) -> BoxFuture<'static, Result<StreamingTransportResponse, crate::GatewayError>> {
            let seen = self.seen.clone();
            let chunks = self.stream_chunks.clone().expect("missing stream chunks");

            Box::pin(async move {
                seen.lock().expect("seen lock").push(request);
                Ok(StreamingTransportResponse {
                    status: 200,
                    headers: HashMap::new(),
                    stream: Box::pin(futures_util::stream::iter(
                        chunks.into_iter().map(Ok::<Vec<u8>, GatewayError>),
                    )),
                })
            })
        }
    }

    fn endpoint() -> DriverEndpointContext {
        DriverEndpointContext {
            endpoint_id: "anth-1".to_string(),
            provider_kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com/v1/".to_string(),
            api_key: SecretString::new("sk-ant"),
            model_policy: ModelPolicy::default(),
            metadata: HashMap::from([("pool_id".to_string(), "beta".to_string())]),
        }
    }

    #[test]
    fn build_chat_request_moves_system_messages_to_top_level_field() {
        let request = build_chat_request(
            &endpoint(),
            &ProxyChatRequest {
                model: "claude-3-5-sonnet".to_string(),
                messages: vec![
                    Message {
                        role: MessageRole::System,
                        content: "be concise".to_string(),
                    },
                    Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    },
                ],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: Some(0.2),
                top_p: None,
                top_k: Some(8),
                max_tokens: None,
                stop_sequences: Some(json!(["DONE", "HALT"])),
                stream: false,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .expect("anthropic request");

        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            request.headers.get("x-api-key").map(String::as_str),
            Some("sk-ant")
        );

        let body: serde_json::Value =
            serde_json::from_slice(&request.body.expect("body")).expect("json body");
        assert_eq!(
            body.get("system").and_then(serde_json::Value::as_str),
            Some("be concise")
        );
        assert_eq!(
            body.get("max_tokens").and_then(serde_json::Value::as_u64),
            Some(1024)
        );
        assert_eq!(
            body.get("top_k").and_then(serde_json::Value::as_u64),
            Some(8)
        );
        assert_eq!(
            body.get("stop_sequences")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    #[tokio::test]
    async fn anthropic_driver_executes_non_streaming_chat() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(MockTransport {
            response: Some(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: serde_json::to_vec(&json!({
                    "model": "claude-3-5-sonnet",
                    "content": [{"type": "text", "text": "hello from claude"}],
                    "usage": {"input_tokens": 11, "output_tokens": 13}
                }))
                .expect("response body"),
            }),
            stream_chunks: None,
            seen: seen.clone(),
        });
        let driver = AnthropicDriver::new(transport);

        let session = driver
            .execute_chat(
                endpoint(),
                ProxyChatRequest {
                    model: "claude-3-5-sonnet".to_string(),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    system: None,
                    tools: None,
                    tool_choice: None,
                    raw_messages: None,
                    temperature: None,
                    top_p: None,
                    top_k: None,
                    max_tokens: Some(256),
                    stop_sequences: None,
                    stream: false,
                    extra: HashMap::new(),
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("chat result");

        match session {
            ProxySession::Completed(response) => {
                assert_eq!(
                    response.response.output_text.as_deref(),
                    Some("hello from claude")
                );
                assert_eq!(response.report.selected_endpoint_id, "anth-1");
                assert_eq!(
                    response
                        .report
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.total_tokens),
                    Some(24)
                );
            }
            ProxySession::Streaming(_) => panic!("expected completed response"),
        }

        assert_eq!(seen.lock().expect("seen lock").len(), 1);
    }

    #[tokio::test]
    async fn anthropic_driver_executes_streaming_chat() {
        let transport = Arc::new(MockTransport {
			response: None,
			stream_chunks: Some(vec![
				b"event: message_start\ndata: {\"type\":\"message_start\",\"model\":\"claude-3-5-sonnet\"}\n\n".to_vec(),
				b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_vec(),
				b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n".to_vec(),
				b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
			]),
			seen: Arc::new(Mutex::new(Vec::new())),
		});
        let driver = AnthropicDriver::new(transport);

        let session = driver
            .execute_chat(
                endpoint(),
                ProxyChatRequest {
                    model: "claude-3-5-sonnet".to_string(),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    system: None,
                    tools: None,
                    tool_choice: None,
                    raw_messages: None,
                    temperature: None,
                    top_p: None,
                    top_k: None,
                    max_tokens: Some(128),
                    stop_sequences: None,
                    stream: true,
                    extra: HashMap::new(),
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("streaming chat session");

        match session {
            ProxySession::Streaming(streaming) => {
                let chunks = streaming
                    .stream
                    .map(|item| item.expect("chunk"))
                    .collect::<Vec<_>>()
                    .await;
                assert_eq!(chunks.len(), 4);
                assert_eq!(chunks[1].delta.as_deref(), Some("hello"));

                let completion = streaming
                    .completion
                    .await
                    .expect("completion receiver")
                    .expect("completion result");
                assert_eq!(completion.report.request_id, streaming.request_id);
                assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
                assert_eq!(
                    completion
                        .report
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.total_tokens),
                    Some(15)
                );
            }
            ProxySession::Completed(_) => panic!("expected streaming response"),
        }
    }

    #[tokio::test]
    async fn anthropic_driver_streaming_chat_completion_survives_dropped_stream() {
        let transport = Arc::new(MockTransport {
			response: None,
			stream_chunks: Some(vec![
				b"event: message_start\ndata: {\"type\":\"message_start\",\"model\":\"claude-3-5-sonnet\"}\n\n".to_vec(),
				b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_vec(),
				b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n".to_vec(),
				b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
			]),
			seen: Arc::new(Mutex::new(Vec::new())),
		});
        let driver = AnthropicDriver::new(transport);

        let session = driver
            .execute_chat(
                endpoint(),
                ProxyChatRequest {
                    model: "claude-3-5-sonnet".to_string(),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    system: None,
                    tools: None,
                    tool_choice: None,
                    raw_messages: None,
                    temperature: None,
                    top_p: None,
                    top_k: None,
                    max_tokens: Some(128),
                    stop_sequences: None,
                    stream: true,
                    extra: HashMap::new(),
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("streaming chat session");

        match session {
            ProxySession::Streaming(streaming) => {
                let completion = streaming
                    .into_completion()
                    .await
                    .expect("completion result after dropped stream");
                assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
                assert_eq!(
                    completion
                        .report
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.total_tokens),
                    Some(15)
                );
            }
            ProxySession::Completed(_) => panic!("expected streaming response"),
        }
    }
}
