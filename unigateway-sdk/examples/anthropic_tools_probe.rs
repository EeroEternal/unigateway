use std::env;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use unigateway_sdk::core::UniGatewayEngine;
use unigateway_sdk::host::{
    EnvProvider, HostContext, HostDispatchOutcome, HostDispatchTarget, HostFuture, HostProtocol,
    HostRequest, PoolHost, PoolLookupOutcome, build_env_pool, dispatch_request,
};
use unigateway_sdk::protocol::{
    ProtocolByteStream, ProtocolHttpResponse, ProtocolResponseBody,
    anthropic_payload_to_chat_request,
};

struct NoopPoolHost;

impl PoolHost for NoopPoolHost {
    fn pool_for_service<'a>(
        &'a self,
        _service_id: &'a str,
    ) -> HostFuture<'a, unigateway_sdk::host::PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async { Ok(PoolLookupOutcome::not_found()) })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = env_var(["ANTHROPIC_BASE_URL", "UPSTREAM_BASE_URL", "BAI_BASE_URL"])
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    let api_key = env_var(["ANTHROPIC_API_KEY", "UPSTREAM_API_KEY", "BAI_API_KEY"])
        .context("set ANTHROPIC_API_KEY, UPSTREAM_API_KEY, or BAI_API_KEY")?;
    let model = env_var(["ANTHROPIC_MODEL", "UPSTREAM_MODEL", "BAI_MODEL"])
        .unwrap_or_else(|| "claude-haiku-4.5".to_string());

    let engine = UniGatewayEngine::builder()
        .with_builtin_http_drivers()
        .build()
        .context("build unigateway engine")?;

    let pool = build_env_pool(EnvProvider::Anthropic, &model, &base_url, &api_key);
    engine
        .upsert_pool(pool.clone())
        .await
        .context("upsert anthropic env pool")?;

    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);

    let first_payload = json!({
        "model": model,
        "max_tokens": 128,
        "stream": false,
        "tool_choice": {"type": "tool", "name": "lookup_weather"},
        "tools": [
            {
                "name": "lookup_weather",
                "description": "Look up the weather for a city.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"},
                        "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                    },
                    "required": ["city"]
                }
            }
        ],
        "messages": [
            {
                "role": "user",
                "content": "What is the weather in Paris? Use the tool."
            }
        ]
    });

    let first_body = dispatch_anthropic(&context, &pool, &first_payload).await?;
    println!(
        "FIRST_RESPONSE={}",
        serde_json::to_string_pretty(&first_body)?
    );

    let tool_use = first_body
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .context("missing first content block")?;

    if tool_use.get("type").and_then(Value::as_str) != Some("tool_use") {
        bail!("expected first response to be tool_use, got: {first_body}");
    }

    let tool_use_id = tool_use
        .get("id")
        .and_then(Value::as_str)
        .context("tool_use missing id")?;
    let tool_name = tool_use
        .get("name")
        .and_then(Value::as_str)
        .context("tool_use missing name")?;
    let tool_input = tool_use.get("input").cloned().unwrap_or_else(|| json!({}));

    let mut stream_payload = first_payload.clone();
    stream_payload["stream"] = Value::Bool(true);

    let stream_events = dispatch_anthropic_sse(&context, &pool, &stream_payload).await?;
    println!(
        "STREAM_EVENTS={}",
        serde_json::to_string_pretty(&json!(stream_events))?
    );

    let stream_tool_start = stream_events
        .iter()
        .find(|event| {
            event
                .get("event")
                .and_then(Value::as_str)
                .is_some_and(|event_type| event_type == "content_block_start")
                && event
                    .pointer("/data/content_block/type")
                    .and_then(Value::as_str)
                    .is_some_and(|block_type| block_type == "tool_use")
        })
        .context("missing streamed tool_use content_block_start event")?;

    if stream_tool_start
        .pointer("/data/content_block/name")
        .and_then(Value::as_str)
        != Some("lookup_weather")
    {
        bail!("unexpected streamed tool name: {stream_tool_start}");
    }

    let saw_input_json_delta = stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "content_block_delta")
            && event
                .pointer("/data/delta/type")
                .and_then(Value::as_str)
                .is_some_and(|delta_type| delta_type == "input_json_delta")
    });

    if !saw_input_json_delta {
        bail!("missing streamed input_json_delta event for tool input");
    }

    let saw_tool_use_stop = stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "message_delta")
            && event
                .pointer("/data/delta/stop_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason == "tool_use")
    });

    if !saw_tool_use_stop {
        bail!("missing streamed message_delta stop_reason=tool_use");
    }

    let saw_message_stop = stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "message_stop")
    });

    if !saw_message_stop {
        bail!("missing streamed message_stop event");
    }

    let second_payload = json!({
        "model": first_payload["model"],
        "max_tokens": 128,
        "stream": false,
        "tools": first_payload["tools"],
        "messages": [
            {
                "role": "user",
                "content": "What is the weather in Paris? Use the tool."
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": tool_name,
                        "input": tool_input
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": "18C, clear"
                    }
                ]
            }
        ]
    });

    let mut second_stream_payload = second_payload.clone();
    second_stream_payload["stream"] = Value::Bool(true);

    let second_stream_events =
        dispatch_anthropic_sse(&context, &pool, &second_stream_payload).await?;
    println!(
        "SECOND_STREAM_EVENTS={}",
        serde_json::to_string_pretty(&json!(second_stream_events))?
    );

    let saw_text_start = second_stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "content_block_start")
            && event
                .pointer("/data/content_block/type")
                .and_then(Value::as_str)
                .is_some_and(|block_type| block_type == "text")
    });

    if !saw_text_start {
        bail!("missing streamed text content_block_start event after tool_result");
    }

    let streamed_text = second_stream_events
        .iter()
        .filter(|event| {
            event
                .get("event")
                .and_then(Value::as_str)
                .is_some_and(|event_type| event_type == "content_block_delta")
                && event
                    .pointer("/data/delta/type")
                    .and_then(Value::as_str)
                    .is_some_and(|delta_type| delta_type == "text_delta")
        })
        .filter_map(|event| event.pointer("/data/delta/text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");

    if streamed_text.is_empty() {
        bail!("missing streamed text_delta events after tool_result");
    }

    let saw_end_turn = second_stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "message_delta")
            && event
                .pointer("/data/delta/stop_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason == "end_turn")
    });

    if !saw_end_turn {
        bail!("missing streamed message_delta stop_reason=end_turn after tool_result");
    }

    let saw_second_message_stop = second_stream_events.iter().any(|event| {
        event
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type == "message_stop")
    });

    if !saw_second_message_stop {
        bail!("missing streamed message_stop event after tool_result");
    }

    let second_body = dispatch_anthropic(&context, &pool, &second_payload).await?;
    println!(
        "SECOND_RESPONSE={}",
        serde_json::to_string_pretty(&second_body)?
    );

    let final_text = second_body
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|block| block.get("text"))
        .and_then(Value::as_str)
        .context("missing final text block")?;

    if second_body.get("stop_reason").and_then(Value::as_str) != Some("end_turn") {
        bail!("expected final stop_reason=end_turn, got: {second_body}");
    }

    println!("STREAM_FINAL_TEXT={streamed_text}");
    println!("FINAL_TEXT={final_text}");
    println!("UNIGATEWAY_ANTHROPIC_TOOLS_OK");
    println!("UNIGATEWAY_ANTHROPIC_STREAM_TOOLS_OK");
    println!("UNIGATEWAY_ANTHROPIC_STREAM_TOOL_RESULT_OK");

    Ok(())
}

async fn dispatch_anthropic(
    context: &HostContext<'_>,
    pool: &unigateway_sdk::core::ProviderPool,
    payload: &Value,
) -> Result<Value> {
    let default_model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("claude-haiku-4.5");
    let request = anthropic_payload_to_chat_request(payload, default_model)
        .context("parse anthropic payload")?;

    let outcome = dispatch_request(
        context,
        HostDispatchTarget::PoolRef(pool),
        HostProtocol::AnthropicMessages,
        Some("anthropic"),
        HostRequest::Chat(request),
    )
    .await
    .context("dispatch anthropic request through unigateway")?;

    match outcome {
        HostDispatchOutcome::Response(response) => json_body(response),
        HostDispatchOutcome::PoolNotFound => bail!("unigateway reported pool not found"),
        _ => bail!("unigateway returned an unsupported dispatch outcome"),
    }
}

async fn dispatch_anthropic_sse(
    context: &HostContext<'_>,
    pool: &unigateway_sdk::core::ProviderPool,
    payload: &Value,
) -> Result<Vec<Value>> {
    let default_model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("claude-haiku-4.5");
    let request = anthropic_payload_to_chat_request(payload, default_model)
        .context("parse streaming anthropic payload")?;

    let outcome = dispatch_request(
        context,
        HostDispatchTarget::PoolRef(pool),
        HostProtocol::AnthropicMessages,
        Some("anthropic"),
        HostRequest::Chat(request),
    )
    .await
    .context("dispatch streaming anthropic request through unigateway")?;

    match outcome {
        HostDispatchOutcome::Response(response) => sse_events(response).await,
        HostDispatchOutcome::PoolNotFound => bail!("unigateway reported pool not found"),
        _ => bail!("unigateway returned an unsupported dispatch outcome"),
    }
}

fn json_body(response: ProtocolHttpResponse) -> Result<Value> {
    let (_status, body) = response.into_parts();
    match body {
        ProtocolResponseBody::Json(value) => Ok(value),
        ProtocolResponseBody::ServerSentEvents(_) => {
            bail!("expected non-streaming JSON response, got SSE")
        }
    }
}

async fn sse_events(response: ProtocolHttpResponse) -> Result<Vec<Value>> {
    let (_status, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        bail!("expected SSE response, got JSON")
    };

    collect_sse_events(stream).await
}

async fn collect_sse_events(mut stream: ProtocolByteStream) -> Result<Vec<Value>> {
    let mut events = Vec::new();
    let mut buffer = String::new();

    while let Some(item) = stream.next().await {
        let bytes = item.context("read streamed anthropic SSE chunk")?;
        let chunk = String::from_utf8(bytes.to_vec()).context("decode SSE chunk as UTF-8")?;
        buffer.push_str(&chunk);

        while let Some(frame_end) = buffer.find("\n\n") {
            let frame = buffer[..frame_end].to_string();
            buffer.drain(..frame_end + 2);

            if frame.trim().is_empty() {
                continue;
            }

            events.push(parse_sse_frame(&frame)?);
        }
    }

    Ok(events)
}

fn parse_sse_frame(frame: &str) -> Result<Value> {
    let mut event_type = None;
    let mut data_lines = Vec::new();

    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event_type = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data_lines.push(value);
        }
    }

    let event_type = event_type.context("SSE frame missing event")?;
    let data = data_lines.join("\n");
    let data = serde_json::from_str::<Value>(&data).context("parse SSE JSON payload")?;

    Ok(json!({
        "event": event_type,
        "data": data,
    }))
}

fn env_var<const N: usize>(names: [&str; N]) -> Option<String> {
    names.into_iter().find_map(|name| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}
