use std::io;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use unigateway_core::{ChatResponseChunk, ChatResponseFinal, StreamingResponse, TokenUsage};

#[derive(Default)]
pub(crate) struct OpenAiChatStreamAdapter {
    model: Option<String>,
    sent_role_chunk: bool,
}

pub(crate) fn openai_sse_chunks_from_chat_chunk(
    request_id: &str,
    adapter: &mut OpenAiChatStreamAdapter,
    chunk: ChatResponseChunk,
) -> Vec<Bytes> {
    if chunk
        .raw
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .is_some()
    {
        return serde_json::to_string(&chunk.raw)
            .map(|json| vec![Bytes::from(format!("data: {json}\n\n"))])
            .unwrap_or_default();
    }

    let event_type = chunk
        .raw
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    match event_type {
        "message_start" => {
            adapter.model = chunk
                .raw
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    chunk
                        .raw
                        .get("message")
                        .and_then(|message| message.get("model"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });

            if adapter.sent_role_chunk {
                return Vec::new();
            }

            adapter.sent_role_chunk = true;
            vec![openai_chat_sse_bytes(
                request_id,
                adapter.model.as_deref().unwrap_or_default(),
                serde_json::json!({"role": "assistant"}),
                None,
            )]
        }
        "content_block_delta" => {
            let Some(delta) = chunk
                .raw
                .get("delta")
                .and_then(|delta| delta.get("text"))
                .and_then(serde_json::Value::as_str)
            else {
                return Vec::new();
            };

            if !adapter.sent_role_chunk {
                adapter.sent_role_chunk = true;
                return vec![
                    openai_chat_sse_bytes(
                        request_id,
                        adapter.model.as_deref().unwrap_or_default(),
                        serde_json::json!({"role": "assistant"}),
                        None,
                    ),
                    openai_chat_sse_bytes(
                        request_id,
                        adapter.model.as_deref().unwrap_or_default(),
                        serde_json::json!({"content": delta}),
                        None,
                    ),
                ];
            }

            vec![openai_chat_sse_bytes(
                request_id,
                adapter.model.as_deref().unwrap_or_default(),
                serde_json::json!({"content": delta}),
                None,
            )]
        }
        "message_stop" => vec![openai_chat_sse_bytes(
            request_id,
            adapter.model.as_deref().unwrap_or_default(),
            serde_json::json!({}),
            Some("stop"),
        )],
        _ => Vec::new(),
    }
}

fn openai_chat_sse_bytes(
    request_id: &str,
    model: &str,
    delta: serde_json::Value,
    finish_reason: Option<&str>,
) -> Bytes {
    let payload = serde_json::json!({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason,
        }],
    });

    Bytes::from(format!("data: {}\n\n", payload))
}

pub(super) async fn drive_anthropic_chat_stream(
    mut streaming: StreamingResponse<ChatResponseChunk, ChatResponseFinal>,
    requested_model: String,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let request_id = streaming.request_id.clone();
    let mut content_block_started = false;
    let mut buffered_text = String::new();

    if emit_sse_json(
        &sender,
        "message_start",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": request_id,
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
    .is_err()
    {
        return;
    }

    while let Some(item) = streaming.stream.next().await {
        match item {
            Ok(chunk) => {
                if let Some(delta) = chunk.delta.filter(|delta| !delta.is_empty()) {
                    if !content_block_started {
                        if emit_sse_json(
                            &sender,
                            "content_block_start",
                            serde_json::json!({
                                "type": "content_block_start",
                                "index": 0,
                                "content_block": {
                                    "type": "text",
                                    "text": "",
                                }
                            }),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        content_block_started = true;
                    }

                    buffered_text.push_str(&delta);
                    if emit_sse_json(
                        &sender,
                        "content_block_delta",
                        serde_json::json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {
                                "type": "text_delta",
                                "text": delta,
                            }
                        }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
        }
    }

    let completion = match streaming.completion.await {
        Ok(Ok(completed)) => completed,
        Ok(Err(error)) => {
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
        Err(error) => {
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
    };

    if !content_block_started
        && let Some(text) = completion
            .response
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
    {
        if emit_sse_json(
            &sender,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": "",
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }
        if emit_sse_json(
            &sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": text,
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }
        buffered_text.push_str(text);
        content_block_started = true;
    }

    if content_block_started
        && emit_sse_json(
            &sender,
            "content_block_stop",
            serde_json::json!({
                "type": "content_block_stop",
                "index": 0,
            }),
        )
        .await
        .is_err()
    {
        return;
    }

    if emit_sse_json(
        &sender,
        "message_delta",
        serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
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
