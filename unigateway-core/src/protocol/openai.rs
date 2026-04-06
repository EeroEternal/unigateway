use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::request::{
    MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};
use crate::transport::{HttpTransport, TransportByteStream, TransportRequest};

use super::{
    build_request_report, drain_sse_frames, next_request_id, output_text_from_openai_message,
    parse_sse_frame,
};

pub const DRIVER_ID: &str = "openai-compatible";

pub struct OpenAiCompatibleDriver {
    transport: Arc<dyn HttpTransport>,
}

impl OpenAiCompatibleDriver {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }
}

impl ProviderDriver for OpenAiCompatibleDriver {
    fn driver_id(&self) -> &str {
        DRIVER_ID
    }

    fn provider_kind(&self) -> crate::ProviderKind {
        crate::ProviderKind::OpenAiCompatible
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
                report: build_request_report(&endpoint, started_at, finished_at, usage, None),
            }))
        })
    }

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>
    {
        let transport = self.transport.clone();

        Box::pin(async move {
            if request.stream {
                return start_responses_stream(transport, endpoint, request).await;
            }

            let started_at = SystemTime::now();
            let transport_request = build_responses_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_responses_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(ProxySession::Completed(CompletedResponse {
                response: response_body,
                report: build_request_report(&endpoint, started_at, finished_at, usage, None),
            }))
        })
    }

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>> {
        let transport = self.transport.clone();

        Box::pin(async move {
            let started_at = SystemTime::now();
            let transport_request = build_embeddings_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_embeddings_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(CompletedResponse {
                response: response_body,
                report: build_request_report(&endpoint, started_at, finished_at, usage, None),
            })
        })
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
    let (chunk_tx, chunk_rx) = mpsc::channel(16);
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
        stream: Box::pin(ReceiverStream::new(chunk_rx)),
        completion: completion_rx,
        request_id,
    }))
}

async fn start_responses_stream(
    transport: Arc<dyn HttpTransport>,
    endpoint: DriverEndpointContext,
    request: ProxyResponsesRequest,
) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
    let request_id = next_request_id();
    let started_at = SystemTime::now();
    let transport_request = build_responses_request(&endpoint, &request)?;
    let transport_response = transport.send_stream(transport_request).await?;
    let (event_tx, event_rx) = mpsc::channel(16);
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
    let completion_request_id = request_id.clone();

    tokio::spawn(async move {
        let completion = drive_responses_stream(
            transport_response.stream,
            event_tx,
            endpoint,
            started_at,
            completion_request_id,
        )
        .await;
        let _ = completion_tx.send(completion);
    });

    Ok(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(ReceiverStream::new(event_rx)),
        completion: completion_rx,
        request_id,
    }))
}

#[derive(Default)]
struct OpenAiChatStreamState {
    model: Option<String>,
    output_text: String,
    usage: Option<TokenUsage>,
    raw_events: Vec<Value>,
    done: bool,
}

#[derive(Default)]
struct OpenAiResponsesStreamState {
    output_text: String,
    usage: Option<TokenUsage>,
    raw_events: Vec<Value>,
    done: bool,
}

async fn drive_chat_stream(
    mut upstream: TransportByteStream,
    chunk_tx: mpsc::Sender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
) -> Result<CompletedResponse<ChatResponseFinal>, GatewayError> {
    let mut buffer = Vec::new();
    let mut state = OpenAiChatStreamState::default();

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

async fn drive_responses_stream(
    mut upstream: TransportByteStream,
    event_tx: mpsc::Sender<Result<ResponsesEvent, GatewayError>>,
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
) -> Result<CompletedResponse<ResponsesFinal>, GatewayError> {
    let mut buffer = Vec::new();
    let mut state = OpenAiResponsesStreamState::default();

    while let Some(item) = upstream.next().await {
        match item {
            Ok(bytes) => {
                buffer.extend_from_slice(&bytes);
                for frame in drain_sse_frames(&mut buffer) {
                    process_responses_frame(&event_tx, &endpoint, &mut state, frame).await?;
                    if state.done {
                        return finalize_responses_stream(endpoint, started_at, request_id, state);
                    }
                }
            }
            Err(error) => {
                return fail_stream::<ResponsesEvent, ResponsesFinal>(
                    &event_tx,
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
        process_responses_frame(&event_tx, &endpoint, &mut state, frame).await?;
    }

    finalize_responses_stream(endpoint, started_at, request_id, state)
}

async fn process_chat_frame(
    chunk_tx: &mpsc::Sender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: &DriverEndpointContext,
    state: &mut OpenAiChatStreamState,
    frame: super::ParsedSseEvent,
) -> Result<(), GatewayError> {
    if frame.data == "[DONE]" {
        state.done = true;
        return Ok(());
    }

    let raw: Value =
        serde_json::from_str(&frame.data).map_err(|error| GatewayError::Transport {
            message: format!("failed to parse openai chat stream frame: {error}"),
            endpoint_id: Some(endpoint.endpoint_id.clone()),
        })?;

    if state.model.is_none() {
        state.model = raw.get("model").and_then(Value::as_str).map(str::to_string);
    }
    if let Some(usage) = parse_openai_usage(&raw) {
        state.usage = Some(usage);
    }

    let delta = raw
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(output_text_from_openai_message);

    if let Some(text) = delta.as_deref() {
        state.output_text.push_str(text);
    }

    state.raw_events.push(raw.clone());

    chunk_tx
        .send(Ok(ChatResponseChunk { delta, raw }))
        .await
        .map_err(|_| downstream_closed(&endpoint.endpoint_id))
}

async fn process_responses_frame(
    event_tx: &mpsc::Sender<Result<ResponsesEvent, GatewayError>>,
    endpoint: &DriverEndpointContext,
    state: &mut OpenAiResponsesStreamState,
    frame: super::ParsedSseEvent,
) -> Result<(), GatewayError> {
    if frame.data == "[DONE]" {
        state.done = true;
        return Ok(());
    }

    let mut raw: Value =
        serde_json::from_str(&frame.data).map_err(|error| GatewayError::Transport {
            message: format!("failed to parse openai responses stream frame: {error}"),
            endpoint_id: Some(endpoint.endpoint_id.clone()),
        })?;

    let event_type = frame
        .event
        .or_else(|| raw.get("type").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "message".to_string());

    if let Some(object) = raw.as_object_mut() {
        object
            .entry("type".to_string())
            .or_insert_with(|| Value::String(event_type.clone()));
    }

    if event_type == "response.output_text.delta"
        && let Some(delta) = raw.get("delta").and_then(Value::as_str)
    {
        state.output_text.push_str(delta);
    }

    if let Some(usage) = parse_responses_usage(&raw) {
        state.usage = Some(usage);
    }

    state.raw_events.push(raw.clone());

    event_tx
        .send(Ok(ResponsesEvent {
            event_type,
            data: raw,
        }))
        .await
        .map_err(|_| downstream_closed(&endpoint.endpoint_id))
}

fn finalize_chat_stream(
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
    state: OpenAiChatStreamState,
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
            Some(request_id),
        ),
    })
}

fn finalize_responses_stream(
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
    state: OpenAiResponsesStreamState,
) -> Result<CompletedResponse<ResponsesFinal>, GatewayError> {
    let finished_at = SystemTime::now();
    Ok(CompletedResponse {
        response: ResponsesFinal {
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
            Some(request_id),
        ),
    })
}

async fn fail_stream<Item, Final>(
    sender: &mpsc::Sender<Result<Item, GatewayError>>,
    endpoint_id: &str,
    message: String,
) -> Result<CompletedResponse<Final>, GatewayError> {
    let stream_error = GatewayError::StreamAborted {
        message: message.clone(),
        endpoint_id: endpoint_id.to_string(),
    };
    let _ = sender.send(Err(stream_error)).await;
    Err(GatewayError::StreamAborted {
        message,
        endpoint_id: endpoint_id.to_string(),
    })
}

fn downstream_closed(endpoint_id: &str) -> GatewayError {
    GatewayError::StreamAborted {
        message: "downstream stream receiver dropped".to_string(),
        endpoint_id: endpoint_id.to_string(),
    }
}

pub fn build_chat_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyChatRequest,
) -> Result<TransportRequest, GatewayError> {
    let payload = json!({
        "model": resolved_model(endpoint, &request.model),
        "messages": request
            .messages
            .iter()
            .map(|message| json!({
                "role": openai_role(message.role),
                "content": message.content,
            }))
            .collect::<Vec<_>>(),
        "temperature": request.temperature,
        "top_p": request.top_p,
        "max_tokens": request.max_tokens,
        "stream": request.stream,
    });

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "chat/completions"),
        openai_headers(endpoint),
        &payload,
        None,
    )
}

pub fn build_responses_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyResponsesRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ]);

    if let Some(input) = request.input.clone() {
        payload.insert("input".to_string(), input);
    }
    if let Some(instructions) = request.instructions.clone() {
        payload.insert("instructions".to_string(), Value::String(instructions));
    }
    if let Some(temperature) = request.temperature {
        payload.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        payload.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(max_output_tokens) = request.max_output_tokens {
        payload.insert("max_output_tokens".to_string(), json!(max_output_tokens));
    }
    if let Some(previous_response_id) = request.previous_response_id.clone() {
        payload.insert(
            "previous_response_id".to_string(),
            Value::String(previous_response_id),
        );
    }
    if let Some(request_metadata) = request.request_metadata.clone() {
        payload.insert("metadata".to_string(), request_metadata);
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "responses"),
        openai_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

pub fn build_embeddings_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyEmbeddingsRequest,
) -> Result<TransportRequest, GatewayError> {
    let payload = json!({
        "model": resolved_model(endpoint, &request.model),
        "input": request.input,
    });

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "embeddings"),
        openai_headers(endpoint),
        &payload,
        None,
    )
}

pub fn parse_chat_response(
    body: &[u8],
) -> Result<(ChatResponseFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai chat response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(output_text_from_openai_message);

    let usage = parse_openai_usage(&raw);

    Ok((
        ChatResponseFinal {
            model: raw.get("model").and_then(Value::as_str).map(str::to_string),
            output_text,
            raw,
        },
        usage,
    ))
}

pub fn parse_responses_response(
    body: &[u8],
) -> Result<(ResponsesFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai responses response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_responses_output_text(&raw));

    let usage = parse_responses_usage(&raw);

    Ok((ResponsesFinal { output_text, raw }, usage))
}

pub fn parse_embeddings_response(
    body: &[u8],
) -> Result<(EmbeddingsResponse, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai embeddings response: {error}"),
        endpoint_id: None,
    })?;
    let usage = parse_openai_usage(&raw);
    Ok((EmbeddingsResponse { raw }, usage))
}

fn parse_openai_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
}

fn extract_responses_output_text(raw: &Value) -> Option<String> {
    raw.get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            let texts = items
                .iter()
                .flat_map(|item| {
                    item.get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(|content| content.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();

            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        })
}

fn parse_responses_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw
        .get("response")
        .and_then(|response| response.get("usage"))
        .or_else(|| raw.get("usage"))?;

    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
}

fn openai_headers(endpoint: &DriverEndpointContext) -> HashMap<String, String> {
    HashMap::from([
        (
            "authorization".to_string(),
            format!("Bearer {}", endpoint.api_key.expose_secret()),
        ),
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

fn openai_role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
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
    use serde_json::{Value, json};

    use super::{
        OpenAiCompatibleDriver, build_chat_request, build_responses_request,
        parse_responses_response,
    };
    use crate::GatewayError;
    use crate::drivers::{DriverEndpointContext, ProviderDriver};
    use crate::pool::{ModelPolicy, ProviderKind, SecretString};
    use crate::request::{
        Message, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
    };
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
            endpoint_id: "ep-1".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            base_url: "https://api.example.com/v1/".to_string(),
            api_key: SecretString::new("sk-test"),
            model_policy: ModelPolicy {
                default_model: Some("gpt-4o-mini".to_string()),
                model_mapping: HashMap::from([("alias".to_string(), "mapped-model".to_string())]),
            },
            metadata: HashMap::from([("pool_id".to_string(), "alpha".to_string())]),
        }
    }

    #[test]
    fn build_chat_request_maps_model_and_url() {
        let request = build_chat_request(
            &endpoint(),
            &ProxyChatRequest {
                model: "alias".to_string(),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                temperature: Some(0.3),
                top_p: None,
                max_tokens: Some(32),
                stream: false,
                metadata: HashMap::new(),
            },
        )
        .expect("chat request");

        assert_eq!(request.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer sk-test")
        );

        let body: serde_json::Value =
            serde_json::from_slice(&request.body.expect("body")).expect("json body");
        assert_eq!(
            body.get("model").and_then(serde_json::Value::as_str),
            Some("mapped-model")
        );
    }

    #[test]
    fn build_responses_request_forwards_supported_optional_fields() {
        let request = build_responses_request(
            &endpoint(),
            &ProxyResponsesRequest {
                model: "alias".to_string(),
                input: Some(json!([{"role": "user", "content": "hello"}])),
                instructions: Some("be terse".to_string()),
                temperature: Some(0.2),
                top_p: Some(0.9),
                max_output_tokens: Some(128),
                stream: true,
                previous_response_id: Some("resp_prev".to_string()),
                request_metadata: Some(json!({"trace_id": "abc"})),
                metadata: HashMap::new(),
            },
        )
        .expect("responses request");

        let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
        assert_eq!(
            body.get("model").and_then(Value::as_str),
            Some("mapped-model")
        );
        assert_eq!(
            body.get("instructions").and_then(Value::as_str),
            Some("be terse")
        );
        assert_eq!(
            body.get("max_output_tokens").and_then(Value::as_u64),
            Some(128)
        );
        assert_eq!(
            body.get("previous_response_id").and_then(Value::as_str),
            Some("resp_prev")
        );
        assert_eq!(
            body.get("metadata")
                .and_then(|value| value.get("trace_id"))
                .and_then(Value::as_str),
            Some("abc")
        );
    }

    #[test]
    fn parse_responses_response_reads_responses_usage_shape() {
        let (response, usage) = parse_responses_response(
            &serde_json::to_vec(&json!({
                "id": "resp_1",
                "object": "response",
                "output_text": "hello",
                "usage": {
                    "input_tokens": 7,
                    "output_tokens": 5,
                    "total_tokens": 12
                }
            }))
            .expect("response body"),
        )
        .expect("parse response");

        assert_eq!(response.output_text.as_deref(), Some("hello"));
        assert_eq!(usage.and_then(|usage| usage.total_tokens), Some(12));
    }

    #[tokio::test]
    async fn openai_driver_executes_non_streaming_operations() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(MockTransport {
            response: Some(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: serde_json::to_vec(&json!({
                    "id": "chatcmpl-1",
                    "model": "gpt-4o-mini",
                    "choices": [{"message": {"content": "hello back"}}],
                    "usage": {
                        "prompt_tokens": 5,
                        "completion_tokens": 7,
                        "total_tokens": 12
                    }
                }))
                .expect("response body"),
            }),
            stream_chunks: None,
            seen: seen.clone(),
        });
        let driver = OpenAiCompatibleDriver::new(transport);

        let session = driver
            .execute_chat(
                endpoint(),
                ProxyChatRequest {
                    model: "alias".to_string(),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    stream: false,
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("chat result");

        match session {
            ProxySession::Completed(response) => {
                assert_eq!(response.response.output_text.as_deref(), Some("hello back"));
                assert_eq!(response.report.selected_endpoint_id, "ep-1");
                assert_eq!(response.report.pool_id.as_deref(), Some("alpha"));
                assert_eq!(
                    response
                        .report
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.total_tokens),
                    Some(12)
                );
            }
            ProxySession::Streaming(_) => panic!("expected completed response"),
        }

        assert_eq!(seen.lock().expect("seen lock").len(), 1);

        let embeddings_transport = Arc::new(MockTransport {
            response: Some(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: serde_json::to_vec(&json!({
                    "data": [{"embedding": [0.1, 0.2], "index": 0}],
                    "usage": {"prompt_tokens": 3, "total_tokens": 3}
                }))
                .expect("embeddings body"),
            }),
            stream_chunks: None,
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let embeddings_driver = OpenAiCompatibleDriver::new(embeddings_transport);
        let embeddings = embeddings_driver
            .execute_embeddings(
                endpoint(),
                ProxyEmbeddingsRequest {
                    model: "text-embedding-3-small".to_string(),
                    input: vec!["hello".to_string()],
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("embeddings result");
        assert!(embeddings.response.raw.get("data").is_some());

        let responses_transport = Arc::new(MockTransport {
            response: Some(TransportResponse {
                status: 200,
                headers: HashMap::new(),
                body: serde_json::to_vec(&json!({
                    "output": [
                        {"content": [{"type": "output_text", "text": "response text"}]}
                    ]
                }))
                .expect("responses body"),
            }),
            stream_chunks: None,
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let responses_driver = OpenAiCompatibleDriver::new(responses_transport);
        let responses = responses_driver
            .execute_responses(
                endpoint(),
                ProxyResponsesRequest {
                    model: "gpt-4.1-mini".to_string(),
                    input: Some(json!([{"role": "user", "content": "hello"}])),
                    instructions: None,
                    temperature: None,
                    top_p: None,
                    max_output_tokens: None,
                    stream: false,
                    previous_response_id: None,
                    request_metadata: None,
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("responses result");

        match responses {
            ProxySession::Completed(response) => {
                assert_eq!(
                    response.response.output_text.as_deref(),
                    Some("response text")
                );
            }
            ProxySession::Streaming(_) => panic!("expected completed response"),
        }
    }

    #[tokio::test]
    async fn openai_driver_executes_streaming_chat() {
        let transport = Arc::new(MockTransport {
			response: None,
			stream_chunks: Some(vec![
				b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_vec(),
				b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n".to_vec(),
				b"data: [DONE]\n\n".to_vec(),
			]),
			seen: Arc::new(Mutex::new(Vec::new())),
		});
        let driver = OpenAiCompatibleDriver::new(transport);

        let session = driver
            .execute_chat(
                endpoint(),
                ProxyChatRequest {
                    model: "alias".to_string(),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    stream: true,
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("chat stream session");

        match session {
            ProxySession::Streaming(streaming) => {
                let chunks = streaming
                    .stream
                    .map(|item| item.expect("chunk"))
                    .collect::<Vec<_>>()
                    .await;
                assert_eq!(chunks.len(), 2);
                assert_eq!(chunks[0].delta.as_deref(), Some("hel"));
                assert_eq!(chunks[1].delta.as_deref(), Some("lo"));

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
                    Some(7)
                );
            }
            ProxySession::Completed(_) => panic!("expected streaming response"),
        }
    }

    #[tokio::test]
    async fn openai_driver_executes_streaming_responses() {
        let transport = Arc::new(MockTransport {
			response: None,
			stream_chunks: Some(vec![
				b"event: response.created\ndata: {\"response\":{\"id\":\"resp_1\"}}\n\n".to_vec(),
				b"event: response.output_text.delta\ndata: {\"delta\":\"hello\"}\n\n".to_vec(),
				b"event: response.completed\ndata: {\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":4,\"total_tokens\":7}}}\n\n".to_vec(),
				b"data: [DONE]\n\n".to_vec(),
			]),
			seen: Arc::new(Mutex::new(Vec::new())),
		});
        let driver = OpenAiCompatibleDriver::new(transport);

        let session = driver
            .execute_responses(
                endpoint(),
                ProxyResponsesRequest {
                    model: "gpt-4.1-mini".to_string(),
                    input: Some(json!([{"role": "user", "content": "hello"}])),
                    instructions: None,
                    temperature: None,
                    top_p: None,
                    max_output_tokens: None,
                    stream: true,
                    previous_response_id: None,
                    request_metadata: None,
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("responses stream session");

        match session {
            ProxySession::Streaming(streaming) => {
                let events = streaming
                    .stream
                    .map(|item| item.expect("event"))
                    .collect::<Vec<_>>()
                    .await;
                assert_eq!(events.len(), 3);
                assert_eq!(events[0].event_type, "response.created");
                assert_eq!(events[1].event_type, "response.output_text.delta");
                assert_eq!(
                    events[1].data.get("type").and_then(Value::as_str),
                    Some("response.output_text.delta")
                );

                let completion = streaming
                    .completion
                    .await
                    .expect("completion receiver")
                    .expect("completion result");
                assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
                assert_eq!(
                    completion
                        .report
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.total_tokens),
                    Some(7)
                );
            }
            ProxySession::Completed(_) => panic!("expected streaming response"),
        }
    }
}
