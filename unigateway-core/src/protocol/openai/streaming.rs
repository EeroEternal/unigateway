use std::sync::Arc;
use std::time::SystemTime;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::{ProxyChatRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProxySession, ResponsesEvent,
    ResponsesFinal, StreamingResponse, TokenUsage,
};
use crate::transport::{HttpTransport, TransportByteStream};

use super::parsing::{parse_openai_usage, parse_responses_usage};
use super::requests::{build_chat_request, build_responses_request};

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

pub(super) async fn start_chat_stream(
    transport: Arc<dyn HttpTransport>,
    endpoint: DriverEndpointContext,
    request: ProxyChatRequest,
) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let request_id = super::super::next_request_id();
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

pub(super) async fn start_responses_stream(
    transport: Arc<dyn HttpTransport>,
    endpoint: DriverEndpointContext,
    request: ProxyResponsesRequest,
) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
    let request_id = super::super::next_request_id();
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
                for frame in super::super::drain_sse_frames(&mut buffer) {
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
        && let Some(frame) = super::super::parse_sse_frame(&buffer)
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
                for frame in super::super::drain_sse_frames(&mut buffer) {
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
        && let Some(frame) = super::super::parse_sse_frame(&buffer)
    {
        process_responses_frame(&event_tx, &endpoint, &mut state, frame).await?;
    }

    finalize_responses_stream(endpoint, started_at, request_id, state)
}

async fn process_chat_frame(
    chunk_tx: &mpsc::Sender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: &DriverEndpointContext,
    state: &mut OpenAiChatStreamState,
    frame: super::super::ParsedSseEvent,
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
        .and_then(super::super::output_text_from_openai_message);

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
    frame: super::super::ParsedSseEvent,
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
        report: super::super::build_request_report(
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
        report: super::super::build_request_report(
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
