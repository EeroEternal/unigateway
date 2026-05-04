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
