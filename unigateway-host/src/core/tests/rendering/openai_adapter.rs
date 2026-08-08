use serde_json::Value;
use unigateway_core::ChatResponseChunk;
use unigateway_protocol::testing::{OpenAiChatStreamAdapter, openai_sse_chunks_from_chat_chunk};

#[test]
fn openai_stream_adapter_translates_anthropic_events() {
    let mut adapter = OpenAiChatStreamAdapter::default();

    let role_chunk = openai_sse_chunks_from_chat_chunk(
        "req_1",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "message_start",
                "model": "claude-3-5-sonnet",
            }),
        },
    );
    let content_chunk = openai_sse_chunks_from_chat_chunk(
        "req_1",
        &mut adapter,
        ChatResponseChunk {
            delta: Some("hello".to_string()),
            raw: serde_json::json!({
                "type": "content_block_delta",
                "delta": { "text": "hello" },
            }),
        },
    );
    let stop_chunk = openai_sse_chunks_from_chat_chunk(
        "req_1",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "message_stop",
            }),
        },
    );

    let role_payload = role_chunk[0]
        .as_ref()
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("role payload");
    let content_payload = content_chunk[0]
        .as_ref()
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("content payload");
    let stop_payload = stop_chunk[0]
        .as_ref()
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("stop payload");

    let role_json: Value = serde_json::from_slice(role_payload).expect("role json");
    let content_json: Value = serde_json::from_slice(content_payload).expect("content json");
    let stop_json: Value = serde_json::from_slice(stop_payload).expect("stop json");

    assert_eq!(
        role_json
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("role"))
            .and_then(Value::as_str),
        Some("assistant")
    );
    assert_eq!(
        content_json
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str),
        Some("hello")
    );
    assert_eq!(
        stop_json
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str),
        Some("stop")
    );
}

#[test]
fn openai_stream_adapter_translates_anthropic_tool_use_events() {
    let mut adapter = OpenAiChatStreamAdapter::default();

    let _ = openai_sse_chunks_from_chat_chunk(
        "req_tool",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "message_start",
                "model": "claude-3-5-sonnet",
            }),
        },
    );
    let tool_start = openai_sse_chunks_from_chat_chunk(
        "req_tool",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "lookup_weather",
                    "input": {}
                }
            }),
        },
    );
    let tool_delta = openai_sse_chunks_from_chat_chunk(
        "req_tool",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"city\":\"Paris\"}"
                }
            }),
        },
    );
    let stop_chunk = openai_sse_chunks_from_chat_chunk(
        "req_tool",
        &mut adapter,
        ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"}
            }),
        },
    );
    let stop_chunk = [
        stop_chunk,
        openai_sse_chunks_from_chat_chunk(
            "req_tool",
            &mut adapter,
            ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({"type": "message_stop"}),
            },
        ),
    ]
    .concat();

    let tool_start_payload = tool_start[0]
        .as_ref()
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("tool start payload");
    let tool_delta_payload = tool_delta[0]
        .as_ref()
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("tool delta payload");
    let stop_payload = stop_chunk.last().expect("stop chunk").as_ref();
    let stop_payload = stop_payload
        .strip_prefix(b"data: ")
        .and_then(|bytes: &[u8]| bytes.strip_suffix(b"\n\n"))
        .expect("stop payload");

    let tool_start_json: Value =
        serde_json::from_slice(tool_start_payload).expect("tool start json");
    let tool_delta_json: Value =
        serde_json::from_slice(tool_delta_payload).expect("tool delta json");
    let stop_json: Value = serde_json::from_slice(stop_payload).expect("stop json");

    assert_eq!(
        tool_start_json
            .pointer("/choices/0/delta/tool_calls/0/id")
            .and_then(Value::as_str),
        Some("toolu_1")
    );
    assert_eq!(
        tool_delta_json
            .pointer("/choices/0/delta/tool_calls/0/function/arguments")
            .and_then(Value::as_str),
        Some("{\"city\":\"Paris\"}")
    );
    assert_eq!(
        stop_json
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str),
        Some("tool_calls")
    );
}
