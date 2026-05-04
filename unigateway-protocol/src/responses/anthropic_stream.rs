use std::collections::BTreeMap;
use std::io;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, StreamingResponse, THINKING_SIGNATURE_PLACEHOLDER_VALUE,
    TokenUsage,
    conversion::{
        PendingOpenAiToolCall, apply_openai_tool_call_delta_update,
        flush_openai_tool_call_stop_update,
    },
};

#[derive(Default)]
struct AnthropicOpenAiStreamState {
    prelude_sent: bool,
    next_content_index: usize,
    active_content_block_index: Option<usize>,
    active_content_block_type: Option<&'static str>,
    stop_reason: Option<String>,
    pending_tool_calls: BTreeMap<usize, PendingOpenAiToolCall>,
    saw_native_anthropic: bool,
}

pub(super) async fn drive_anthropic_chat_stream(
    mut streaming: StreamingResponse<ChatResponseChunk, ChatResponseFinal>,
    requested_model: String,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let request_id = streaming.request_id.clone();
    let mut state = AnthropicOpenAiStreamState::default();

    while let Some(item) = streaming.stream.next().await {
        match item {
            Ok(chunk) => {
                if is_native_anthropic_chunk(&chunk.raw) {
                    state.saw_native_anthropic = true;
                    let event_type = chunk
                        .raw
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("message_delta");
                    if emit_sse_json(&sender, event_type, chunk.raw.clone())
                        .await
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }

                if ensure_anthropic_stream_prelude(
                    &sender,
                    &request_id,
                    requested_model.as_str(),
                    &mut state,
                )
                .await
                .is_err()
                {
                    return;
                }

                if process_openai_chunk_for_anthropic_stream(&sender, &chunk.raw, &mut state)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
        }
    }

    let completion = match streaming.into_completion().await {
        Ok(completed) => completed,
        Err(error) => {
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
    };

    if state.saw_native_anthropic {
        return;
    }

    if ensure_anthropic_stream_prelude(&sender, &request_id, requested_model.as_str(), &mut state)
        .await
        .is_err()
    {
        return;
    }

    if state.next_content_index == 0
        && let Some(text) = completion
            .response
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
        && emit_text_delta(&sender, &mut state, text).await.is_err()
    {
        return;
    }

    if close_active_content_block(&sender, &mut state)
        .await
        .is_err()
    {
        return;
    }

    if flush_pending_tool_call_stops(&sender, &mut state)
        .await
        .is_err()
    {
        return;
    }

    let stop_reason = state.stop_reason.clone().unwrap_or_else(|| {
        if state.pending_tool_calls.is_empty() {
            "end_turn".to_string()
        } else {
            "tool_use".to_string()
        }
    });

    if emit_sse_json(
        &sender,
        "message_delta",
        serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null,
            },
            "usage": anthropic_usage_payload(completion.report.usage.as_ref()),
        }),
    )
    .await
    .is_err()
    {
        return;
    }

    let _ = emit_sse_json(
        &sender,
        "message_stop",
        serde_json::json!({
            "type": "message_stop",
        }),
    )
    .await;
}

pub(super) fn anthropic_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
    })
}

pub(super) fn map_finish_reason(reason: &str) -> String {
    match reason {
        "stop" | "eos" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" | "function_call" | "tool_use" => "tool_use",
        _ => "end_turn",
    }
    .to_string()
}

fn is_native_anthropic_chunk(raw: &serde_json::Value) -> bool {
    matches!(
        raw.get("type").and_then(serde_json::Value::as_str),
        Some(
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
                | "ping"
        )
    )
}

async fn ensure_anthropic_stream_prelude(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    request_id: &str,
    requested_model: &str,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    if state.prelude_sent {
        return Ok(());
    }

    let anthropic_id = format!("msg_{request_id}");

    emit_sse_json(
        sender,
        "message_start",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": anthropic_id,
                "type": "message",
                "role": "assistant",
                "model": requested_model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                }
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;
    emit_sse_json(sender, "ping", serde_json::json!({ "type": "ping" }))
        .await
        .map_err(io::Error::other)?;
    state.prelude_sent = true;
    Ok(())
}

async fn emit_text_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<usize, io::Error> {
    emit_content_block_start(sender, state, "text").await
}

async fn emit_thinking_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<usize, io::Error> {
    emit_content_block_start(sender, state, "thinking").await
}

async fn emit_content_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    block_type: &'static str,
) -> Result<usize, io::Error> {
    if let Some(index) = state.active_content_block_index
        && state.active_content_block_type == Some(block_type)
    {
        return Ok(index);
    }

    close_active_content_block(sender, state).await?;

    let index = state.next_content_index;
    state.next_content_index += 1;

    let content_block = if block_type == "thinking" {
        serde_json::json!({
            "type": "thinking",
            "thinking": "",
            "signature": "",
        })
    } else {
        serde_json::json!({
            "type": "text",
            "text": "",
        })
    };

    emit_sse_json(
        sender,
        "content_block_start",
        serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": content_block
        }),
    )
    .await
    .map_err(io::Error::other)?;
    state.active_content_block_index = Some(index);
    state.active_content_block_type = Some(block_type);
    Ok(index)
}

async fn emit_text_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    text: &str,
) -> Result<(), io::Error> {
    let index = emit_text_block_start(sender, state).await?;

    emit_sse_json(
        sender,
        "content_block_delta",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "text_delta",
                "text": text,
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn emit_thinking_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    thinking: &str,
) -> Result<(), io::Error> {
    let index = emit_thinking_block_start(sender, state).await?;

    emit_sse_json(
        sender,
        "content_block_delta",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "thinking_delta",
                "thinking": thinking,
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn close_active_content_block(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let Some(index) = state.active_content_block_index.take() else {
        return Ok(());
    };

    let active_type = state.active_content_block_type.take();

    if active_type == Some("thinking") {
        emit_sse_json(
            sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {
                    "type": "signature_delta",
                    "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE,
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    emit_sse_json(
        sender,
        "content_block_stop",
        serde_json::json!({
            "type": "content_block_stop",
            "index": index,
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn process_openai_chunk_for_anthropic_stream(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    raw: &serde_json::Value,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let choice = raw
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first());

    if let Some(finish_reason) = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(serde_json::Value::as_str)
        .filter(|reason| !reason.is_empty())
    {
        state.stop_reason = Some(map_finish_reason(finish_reason));
    }

    let delta = choice.and_then(|choice| choice.get("delta"));
    if let Some(thinking) = delta
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("thinking"))
        })
        .and_then(serde_json::Value::as_str)
        .filter(|thinking| !thinking.is_empty())
    {
        emit_thinking_delta(sender, state, thinking).await?;
    }

    if let Some(text) = delta
        .and_then(|delta| delta.get("content"))
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
    {
        emit_text_delta(sender, state, text).await?;
    }

    if let Some(tool_calls) = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(serde_json::Value::as_array)
    {
        for (fallback_index, tool_call) in tool_calls.iter().enumerate() {
            let tool_index = tool_call
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(fallback_index);
            apply_openai_tool_call_delta(sender, state, tool_index, tool_call).await?;
        }
    }

    Ok(())
}

async fn apply_openai_tool_call_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    tool_index: usize,
    tool_call: &serde_json::Value,
) -> Result<(), io::Error> {
    let update = apply_openai_tool_call_delta_update(
        &mut state.pending_tool_calls,
        &mut state.next_content_index,
        tool_index,
        tool_call,
    );

    if update.start.is_some() {
        close_active_content_block(sender, state).await?;
    }

    if let Some(start) = update.start {
        emit_sse_json(
            sender,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": start.anthropic_index,
                "content_block": {
                    "type": "tool_use",
                    "id": start.id,
                    "name": start.name,
                    "input": {}
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    if let Some(delta) = update.delta {
        emit_sse_json(
            sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": delta.anthropic_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": delta.partial_json,
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    Ok(())
}

async fn flush_pending_tool_call_stops(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let tool_indexes = state.pending_tool_calls.keys().copied().collect::<Vec<_>>();
    for tool_index in tool_indexes {
        apply_openai_tool_call_delta(sender, state, tool_index, &serde_json::json!({})).await?;

        let update = flush_openai_tool_call_stop_update(&mut state.pending_tool_calls, tool_index);

        let Some(anthropic_index) = update.stop_index else {
            continue;
        };

        if let Some(start) = update.start {
            emit_sse_json(
                sender,
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start",
                    "index": start.anthropic_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": start.id,
                        "name": start.name,
                        "input": {}
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;
        }

        if let Some(delta) = update.delta {
            emit_sse_json(
                sender,
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": delta.anthropic_index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": delta.partial_json,
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;
        }

        emit_sse_json(
            sender,
            "content_block_stop",
            serde_json::json!({
                "type": "content_block_stop",
                "index": anthropic_index,
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    Ok(())
}

async fn emit_sse_json(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    event_type: &str,
    data: serde_json::Value,
) -> Result<()> {
    let json = serde_json::to_string(&data)?;
    sender
        .send(Ok(Bytes::from(format!(
            "event: {event_type}\ndata: {json}\n\n"
        ))))
        .await
        .map_err(|_| anyhow!("anthropic downstream receiver dropped"))
}
