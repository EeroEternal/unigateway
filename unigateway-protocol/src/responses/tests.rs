use serde_json::json;

use super::AnthropicStreamAggregator;

#[test]
fn anthropic_stream_aggregator_rebuilds_thinking_signature_and_tool_use() {
    let mut aggregator = AnthropicStreamAggregator::default();

    aggregator
        .push_event(
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": "msg_123",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-4-6",
                    "content": []
                }
            }),
        )
        .expect("message_start");
    aggregator
        .push_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "thinking",
                    "thinking": "",
                    "signature": ""
                }
            }),
        )
        .expect("thinking start");
    aggregator
        .push_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "thinking_delta",
                    "thinking": "need weather first"
                }
            }),
        )
        .expect("thinking delta");
    aggregator
        .push_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "signature_delta",
                    "signature": "real-signature"
                }
            }),
        )
        .expect("signature delta");
    aggregator
        .push_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "lookup_weather",
                    "input": {}
                }
            }),
        )
        .expect("tool_use start");
    aggregator
        .push_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"city\":"
                }
            }),
        )
        .expect("tool_use delta 1");
    aggregator
        .push_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "\"Paris\"}"
                }
            }),
        )
        .expect("tool_use delta 2");
    aggregator
        .push_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 2,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        )
        .expect("text start");
    aggregator
        .push_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 2,
                "delta": {
                    "type": "text_delta",
                    "text": "Let me check."
                }
            }),
        )
        .expect("text delta");
    aggregator
        .push_event(
            "message_delta",
            &json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": "tool_use",
                    "stop_sequence": null
                },
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5
                }
            }),
        )
        .expect("message delta");
    aggregator
        .push_event("message_stop", &json!({"type": "message_stop"}))
        .expect("message stop");

    assert!(aggregator.is_complete());

    let message = aggregator.snapshot_message().expect("aggregated message");
    assert_eq!(
        message.get("role").and_then(serde_json::Value::as_str),
        Some("assistant")
    );
    assert_eq!(
        message
            .get("stop_reason")
            .and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        message
            .pointer("/content/0/type")
            .and_then(serde_json::Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        message
            .pointer("/content/0/signature")
            .and_then(serde_json::Value::as_str),
        Some("real-signature")
    );
    assert_eq!(
        message
            .pointer("/content/1/type")
            .and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        message
            .pointer("/content/1/input/city")
            .and_then(serde_json::Value::as_str),
        Some("Paris")
    );
    assert_eq!(
        message
            .pointer("/content/2/text")
            .and_then(serde_json::Value::as_str),
        Some("Let me check.")
    );
}

#[test]
fn anthropic_stream_aggregator_push_chunk_uses_chunk_type() {
    let mut aggregator = AnthropicStreamAggregator::default();
    let chunk = unigateway_core::ChatResponseChunk {
        delta: None,
        raw: json!({
            "type": "message_start",
            "message": {
                "id": "msg_456",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": []
            }
        }),
    };

    aggregator.push_chunk(&chunk).expect("push chunk");

    let message = aggregator.snapshot_message().expect("snapshot");
    assert_eq!(
        message.get("id").and_then(serde_json::Value::as_str),
        Some("msg_456")
    );
    assert_eq!(
        message.get("model").and_then(serde_json::Value::as_str),
        Some("claude-sonnet-4-5")
    );
}
