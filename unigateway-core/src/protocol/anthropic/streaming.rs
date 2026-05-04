use std::sync::Arc;
use std::time::SystemTime;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::ProxyChatRequest;
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProxySession, RequestKind,
    StreamingResponse, TokenUsage,
};
use crate::transport::{HttpTransport, TransportByteStream};

use super::parsing::parse_anthropic_usage;
use super::requests::build_chat_request;

#[derive(Default)]
struct AnthropicChatStreamState {
    model: Option<String>,
    output_text: String,
    usage: Option<TokenUsage>,
    raw_events: Vec<Value>,
    done: bool,
    downstream_detached: bool,
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

async fn process_chat_frame(
    chunk_tx: &mpsc::UnboundedSender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: &DriverEndpointContext,
    state: &mut AnthropicChatStreamState,
    frame: super::super::ParsedSseEvent,
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
    if let Some(usage) = parse_anthropic_usage(&raw) {
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
        report: super::super::build_request_report(
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
