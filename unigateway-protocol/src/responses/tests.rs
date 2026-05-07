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

#[tokio::test]
async fn openai_renderer_completed_response_reasoning_text() {
    use crate::REASONING_TEXT_ENCODING_KEY;
    use crate::responses::render_openai_chat_session;
    use std::collections::HashMap;
    use unigateway_core::{
        ChatResponseFinal, CompletedResponse, ProviderKind, ProxySession, RequestKind,
        RequestReport,
    };

    let response = render_openai_chat_session(ProxySession::Completed(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("model".to_string()),
            output_text: Some("<think>hidden</think>visible".to_string()),
            raw: serde_json::json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "<think>hidden</think>visible"
                    }
                }]
            }),
        },
        report: RequestReport {
            request_id: "req".to_string(),
            correlation_id: "req".to_string(),
            pool_id: None,
            selected_endpoint_id: "end".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: vec![],
            usage: None,
            latency_ms: 0,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                REASONING_TEXT_ENCODING_KEY.to_string(),
                "xml_think_tag".to_string(),
            )]),
        },
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::Json(json) = body {
        let message = json.pointer("/choices/0/message").expect("message");
        assert_eq!(
            message.get("reasoning_content").and_then(|v| v.as_str()),
            Some("hidden")
        );
        assert_eq!(
            message.get("content").and_then(|v| v.as_str()),
            Some("visible")
        );
    } else {
        panic!("expected json body");
    }
}

#[tokio::test]
async fn openai_renderer_streaming_response_reasoning_text() {
    use crate::REASONING_TEXT_ENCODING_KEY;
    use crate::responses::render_openai_chat_session;
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use tokio::sync::oneshot;
    use unigateway_core::{ChatResponseChunk, ProxySession, StreamingResponse};

    let (_, completion_rx) = oneshot::channel();
    let response = render_openai_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "<thi"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "nk>hidden</"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "think>visible"
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream".to_string(),
        request_metadata: HashMap::from([(
            REASONING_TEXT_ENCODING_KEY.to_string(),
            "xml_think_tag".to_string(),
        )]),
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::ServerSentEvents(mut stream) = body {
        let mut events = Vec::new();
        while let Some(chunk) = stream.next().await {
            events.push(String::from_utf8(chunk.unwrap().to_vec()).unwrap());
        }
        let full = events.join("");
        assert!(full.contains("\"reasoning_content\":\"hidden\""));
        assert!(full.contains("\"content\":\"visible\""));
        assert!(!full.contains("<think>"));
        assert!(!full.contains("</think>"));
    } else {
        panic!("expected sse body");
    }
}

#[tokio::test]
async fn anthropic_renderer_completed_response_reasoning_text() {
    use crate::REASONING_TEXT_ENCODING_KEY;
    use crate::responses::render_anthropic_chat_session;
    use std::collections::HashMap;
    use unigateway_core::{
        ChatResponseFinal, CompletedResponse, ProviderKind, ProxySession, RequestKind,
        RequestReport,
    };

    let response = render_anthropic_chat_session(ProxySession::Completed(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("model".to_string()),
            output_text: Some("<think>hidden</think>visible".to_string()),
            raw: serde_json::json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "<think>hidden</think>visible"
                    }
                }]
            }),
        },
        report: RequestReport {
            request_id: "req".to_string(),
            correlation_id: "req".to_string(),
            pool_id: None,
            selected_endpoint_id: "end".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: vec![],
            usage: None,
            latency_ms: 0,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                REASONING_TEXT_ENCODING_KEY.to_string(),
                "xml_think_tag".to_string(),
            )]),
        },
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::Json(json) = body {
        let content = json
            .pointer("/content")
            .expect("content")
            .as_array()
            .expect("array");
        assert_eq!(
            content[0].get("type").and_then(|v| v.as_str()),
            Some("thinking")
        );
        assert_eq!(
            content[0].get("thinking").and_then(|v| v.as_str()),
            Some("hidden")
        );
        assert_eq!(
            content[1].get("type").and_then(|v| v.as_str()),
            Some("text")
        );
        assert_eq!(
            content[1].get("text").and_then(|v| v.as_str()),
            Some("visible")
        );
    } else {
        panic!("expected json body");
    }
}

#[tokio::test]
async fn anthropic_renderer_completed_response_structured_reasoning() {
    use crate::responses::render_anthropic_chat_session;
    use std::collections::HashMap;
    use unigateway_core::{
        ChatResponseFinal, CompletedResponse, ProviderKind, ProxySession, RequestKind,
        RequestReport, THINKING_SIGNATURE_PLACEHOLDER_VALUE,
    };

    let response = render_anthropic_chat_session(ProxySession::Completed(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("model".to_string()),
            output_text: Some("visible".to_string()),
            raw: serde_json::json!({
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "reasoning_content": "hidden",
                        "content": "visible"
                    }
                }]
            }),
        },
        report: RequestReport {
            request_id: "req".to_string(),
            correlation_id: "req".to_string(),
            pool_id: None,
            selected_endpoint_id: "end".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: vec![],
            usage: None,
            latency_ms: 0,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::new(),
        },
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::Json(json) = body {
        let content = json
            .pointer("/content")
            .expect("content")
            .as_array()
            .expect("array");
        assert_eq!(
            content[0].get("type").and_then(|v| v.as_str()),
            Some("thinking")
        );
        assert_eq!(
            content[0].get("thinking").and_then(|v| v.as_str()),
            Some("hidden")
        );
        assert_eq!(
            content[0].get("signature").and_then(|v| v.as_str()),
            Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE)
        );
        assert_eq!(
            content[1].get("type").and_then(|v| v.as_str()),
            Some("text")
        );
        assert_eq!(
            content[1].get("text").and_then(|v| v.as_str()),
            Some("visible")
        );
    } else {
        panic!("expected json body");
    }
}

#[tokio::test]
async fn anthropic_renderer_streaming_response_reasoning_text() {
    use crate::REASONING_TEXT_ENCODING_KEY;
    use crate::responses::render_anthropic_chat_session;
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use tokio::sync::oneshot;
    use unigateway_core::{
        ChatResponseChunk, ChatResponseFinal, ProviderKind, ProxySession, RequestKind,
        RequestReport, StreamingResponse,
    };

    let (completion_tx, completion_rx) = oneshot::channel();
    let _ = completion_tx.send(Ok(unigateway_core::CompletedResponse {
        response: ChatResponseFinal {
            model: Some("model".to_string()),
            output_text: None,
            raw: serde_json::json!({}),
        },
        report: RequestReport {
            request_id: "req_stream".to_string(),
            correlation_id: "req_stream".to_string(),
            pool_id: None,
            selected_endpoint_id: "end".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: vec![],
            usage: None,
            latency_ms: 0,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                REASONING_TEXT_ENCODING_KEY.to_string(),
                "xml_think_tag".to_string(),
            )]),
        },
    }));

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "<thi"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "nk>hidden</"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "think>visible"
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream".to_string(),
        request_metadata: HashMap::from([(
            REASONING_TEXT_ENCODING_KEY.to_string(),
            "xml_think_tag".to_string(),
        )]),
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::ServerSentEvents(mut stream) = body {
        let mut events = Vec::new();
        while let Some(chunk) = stream.next().await {
            events.push(String::from_utf8(chunk.unwrap().to_vec()).unwrap());
        }
        let full = events.join("");
        assert!(full.contains("\"type\":\"thinking\""));
        assert!(full.contains("\"thinking\":\"hidden\""));
        assert!(full.contains("\"type\":\"signature_delta\""));
        assert!(full.contains("\"text\":\"visible\""));
        assert!(!full.contains("<think>"));
        assert!(!full.contains("</think>"));
    } else {
        panic!("expected sse body");
    }
}

#[tokio::test]
async fn anthropic_renderer_streaming_response_structured_reasoning() {
    use crate::responses::render_anthropic_chat_session;
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use tokio::sync::oneshot;
    use unigateway_core::{
        ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProviderKind, ProxySession,
        RequestKind, RequestReport, StreamingResponse, THINKING_SIGNATURE_PLACEHOLDER_VALUE,
    };

    let (completion_tx, completion_rx) = oneshot::channel();
    let _ = completion_tx.send(Ok(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("model".to_string()),
            output_text: Some("visible".to_string()),
            raw: serde_json::json!({}),
        },
        report: RequestReport {
            request_id: "req_stream".to_string(),
            correlation_id: "req_stream".to_string(),
            pool_id: None,
            selected_endpoint_id: "end".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: vec![],
            usage: None,
            latency_ms: 0,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::new(),
        },
    }));

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "reasoning_content": "hidden"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "visible"
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream".to_string(),
        request_metadata: HashMap::new(),
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, http::StatusCode::OK);

    if let crate::ProtocolResponseBody::ServerSentEvents(mut stream) = body {
        let mut events = Vec::new();
        while let Some(chunk) = stream.next().await {
            events.push(String::from_utf8(chunk.unwrap().to_vec()).unwrap());
        }
        let full = events.join("");
        assert!(full.contains("\"type\":\"thinking\""));
        assert!(full.contains("\"type\":\"thinking_delta\""));
        assert!(full.contains("\"thinking\":\"hidden\""));
        assert!(full.contains("\"type\":\"signature_delta\""));
        assert!(full.contains(THINKING_SIGNATURE_PLACEHOLDER_VALUE));
        assert!(full.contains("\"type\":\"text\""));
        assert!(full.contains("\"text\":\"visible\""));
    } else {
        panic!("expected sse body");
    }
}
    #[tokio::test]
    async fn openai_renderer_streaming_anthropic_thinking_blocks() {
        use std::collections::HashMap;

        use futures_util::StreamExt;
        use unigateway_core::{ChatResponseChunk, ProxySession, StreamingResponse};

        use crate::responses::render::render_openai_chat_session;

        let (_completion_tx, completion_rx) = tokio::sync::oneshot::channel();

        let response = render_openai_chat_session(ProxySession::Streaming(StreamingResponse {
            stream: Box::pin(futures_util::stream::iter(vec![
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "message_start",
                        "model": "claude-3-5"
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": { "type": "thinking" }
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "thinking_delta", "thinking": "Let me think..." }
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "content_block_stop",
                        "index": 0
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "content_block_start",
                        "index": 1,
                        "content_block": { "type": "text", "text": "" }
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "content_block_delta",
                        "index": 1,
                        "delta": { "type": "text_delta", "text": "Hello world" }
                    }),
                }),
                Ok(ChatResponseChunk {
                    delta: None,
                    raw: serde_json::json!({
                        "type": "message_stop"
                    }),
                }),
            ])),
            completion: completion_rx,
            request_id: "req_stream".to_string(),
            request_metadata: HashMap::new(),
        }));

        let (_status, body) = response.into_parts();
        match body {
            crate::ProtocolResponseBody::ServerSentEvents(mut stream) => {
                let mut events = Vec::new();
                while let Some(chunk) = stream.next().await {
                    events.push(String::from_utf8(chunk.unwrap().to_vec()).unwrap());
                }
                let full = events.join("");
                // Check for reasoning_content (OpenAI standard)
                assert!(full.contains("\"reasoning_content\":\"Let me think...\""));
                // Check for thinking (DeepSeek/Some providers compatible)
                assert!(full.contains("\"thinking\":\"Let me think...\""));
                // Check for content
                assert!(full.contains("\"content\":\"Hello world\""));
            }
            _ => panic!("expected sse body"),
        }
    }