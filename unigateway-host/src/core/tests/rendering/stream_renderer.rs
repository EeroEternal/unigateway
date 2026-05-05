use std::collections::HashMap;

use futures_util::StreamExt;
use http::StatusCode;
use tokio::sync::oneshot;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProxySession, RequestKind,
    RequestReport, StreamingResponse,
};
use unigateway_protocol::{
    ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY, ProtocolResponseBody, REASONING_TEXT_ENCODING_KEY,
    REASONING_TEXT_ENCODING_XML_THINK_TAG, render_anthropic_chat_session,
};

#[tokio::test]
async fn anthropic_stream_renderer_converts_openai_tool_call_deltas() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: None,
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "tool_calls"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_1".to_string(),
                    correlation_id: "req_stream_1".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(2),
                        total_tokens: Some(9),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: Some("Let me check ".to_string()),
                raw: serde_json::json!({
                    "id": "chatcmpl_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "content": "Let me check "
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "lookup_weather",
                                    "arguments": "{\"city\":\""
                                }
                            }]
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "function": {
                                    "arguments": "Paris\"}"
                                }
                            }]
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {},
                        "finish_reason": "tool_calls"
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream_1".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (status, body) = response.into_parts();
    assert_eq!(status, StatusCode::OK);

    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    assert!(
        events
            .iter()
            .any(|event| event.contains("event: message_start"))
    );
    assert!(events.iter().any(|event| event.contains("event: ping")));
    assert!(events.iter().any(|event| {
        event.contains("event: message_start") && event.contains("\"id\":\"msg_req_stream_1\"")
    }));
    let text_start = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_start") && event.contains("\"type\":\"text\"")
        })
        .expect("text block start");
    let text_stop = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_stop") && event.contains("\"index\":0")
        })
        .expect("text block stop");
    let tool_start = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_start") && event.contains("\"type\":\"tool_use\"")
        })
        .expect("tool block start");

    assert!(text_start < text_stop);
    assert!(text_stop < tool_start);

    assert!(events.iter().any(|event| {
        event.contains("event: content_block_delta")
            && event.contains("\"type\":\"text_delta\"")
            && event.contains("Let me check ")
    }));
    let tool_deltas = events
        .iter()
        .filter(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"input_json_delta\"")
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_deltas.len(), 2);
    assert!(tool_deltas[0].contains("{\\\"city\\\":\\\""));
    assert!(tool_deltas[1].contains("Paris\\\"}"));
    assert!(events.iter().any(|event| {
        event.contains("event: message_delta") && event.contains("\"stop_reason\":\"tool_use\"")
    }));
}

#[tokio::test]
async fn anthropic_stream_renderer_converts_openai_reasoning_deltas_to_thinking_blocks() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: Some("final answer".to_string()),
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "stop"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_reasoning_1".to_string(),
                    correlation_id: "req_stream_reasoning_1".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(6),
                        output_tokens: Some(4),
                        total_tokens: Some(10),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_reasoning_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "reasoning_content": "need to think first"
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: Some("final answer".to_string()),
                raw: serde_json::json!({
                    "id": "chatcmpl_reasoning_1",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "content": "final answer"
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream_reasoning_1".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    let thinking_start = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_start") && event.contains("\"type\":\"thinking\"")
        })
        .expect("thinking block start");
    let thinking_delta = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"thinking_delta\"")
                && event.contains("need to think first")
        })
        .expect("thinking delta");
    let signature_delta = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"signature_delta\"")
        })
        .expect("signature delta");
    let thinking_stop = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_stop") && event.contains("\"index\":0")
        })
        .expect("thinking stop");
    let text_start = events
        .iter()
        .position(|event| {
            event.contains("event: content_block_start") && event.contains("\"type\":\"text\"")
        })
        .expect("text block start");

    assert!(thinking_start < thinking_delta);
    assert!(thinking_delta < signature_delta);
    assert!(signature_delta < thinking_stop);
    assert!(thinking_stop < text_start);
}

#[tokio::test]
async fn anthropic_stream_renderer_can_reconstruct_prefixed_think_tags_when_enabled() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("claude-opus-4-7".to_string()),
                    output_text: Some("final answer".to_string()),
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "stop"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_reasoning_text_1".to_string(),
                    correlation_id: "req_stream_reasoning_text_1".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "compat-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(6),
                        output_tokens: Some(4),
                        total_tokens: Some(10),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: Some("<think>inspect ".to_string()),
                raw: serde_json::json!({
                    "id": "chatcmpl_reasoning_text_1",
                    "model": "claude-opus-4-7",
                    "choices": [{
                        "delta": {
                            "content": "<think>inspect "
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: Some("the puzzle</think>final answer".to_string()),
                raw: serde_json::json!({
                    "id": "chatcmpl_reasoning_text_1",
                    "model": "claude-opus-4-7",
                    "choices": [{
                        "delta": {
                            "content": "the puzzle</think>final answer"
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream_reasoning_text_1".to_string(),
        request_metadata: HashMap::from([
            (
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            ),
            (
                REASONING_TEXT_ENCODING_KEY.to_string(),
                REASONING_TEXT_ENCODING_XML_THINK_TAG.to_string(),
            ),
        ]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| {
        event.contains("event: content_block_start") && event.contains("\"type\":\"thinking\"")
    }));
    assert!(events.iter().any(|event| {
        event.contains("event: content_block_delta")
            && event.contains("\"type\":\"thinking_delta\"")
            && event.contains("inspect the puzzle")
    }));
    assert!(events.iter().any(|event| {
        event.contains("event: content_block_delta")
            && event.contains("\"type\":\"signature_delta\"")
    }));
    assert!(events.iter().any(|event| {
        event.contains("event: content_block_delta")
            && event.contains("\"type\":\"text_delta\"")
            && event.contains("final answer")
    }));
}

#[tokio::test]
async fn anthropic_stream_renderer_flushes_unfinished_tool_calls_with_placeholders() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: None,
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "tool_calls"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_2".to_string(),
                    correlation_id: "req_stream_2".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(5),
                        output_tokens: Some(2),
                        total_tokens: Some(7),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![Ok(ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "id": "chatcmpl_2",
                "model": "gpt-4o-mini",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": "{\"city\":\"Paris\"}"
                            }
                        }]
                    }
                }]
            }),
        })])),
        completion: completion_rx,
        request_id: "req_stream_2".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| {
        event.contains("event: content_block_start")
            && event.contains("\"type\":\"tool_use\"")
            && event.contains("toolu_unknown")
            && event.contains("\"name\":\"tool\"")
    }));
    assert!(events.iter().any(|event| {
        event.contains("event: content_block_delta")
            && event.contains("\"type\":\"input_json_delta\"")
            && event.contains("{\\\"city\\\":\\\"Paris\\\"}")
    }));
}

#[tokio::test]
async fn anthropic_stream_renderer_multiplexes_interleaved_tool_calls() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: None,
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "tool_calls"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_3".to_string(),
                    correlation_id: "req_stream_3".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        total_tokens: Some(14),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_3",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [
                                {
                                    "index": 0,
                                    "id": "call_weather",
                                    "type": "function",
                                    "function": {
                                        "name": "lookup_weather",
                                        "arguments": "{\"city\":\""
                                    }
                                },
                                {
                                    "index": 1,
                                    "id": "call_time",
                                    "type": "function",
                                    "function": {
                                        "name": "lookup_time",
                                        "arguments": "{\"timezone\":\""
                                    }
                                }
                            ]
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_3",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [
                                {
                                    "index": 1,
                                    "function": {
                                        "arguments": "UTC\"}"
                                    }
                                },
                                {
                                    "index": 0,
                                    "function": {
                                        "arguments": "Paris\"}"
                                    }
                                }
                            ]
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream_3".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    let tool_starts = events
        .iter()
        .filter(|event| {
            event.contains("event: content_block_start") && event.contains("\"type\":\"tool_use\"")
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_starts.len(), 2);
    assert!(
        tool_starts
            .iter()
            .any(|event| event.contains("call_weather"))
    );
    assert!(tool_starts.iter().any(|event| event.contains("call_time")));

    let tool_deltas = events
        .iter()
        .filter(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"input_json_delta\"")
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_deltas.len(), 4);
    assert!(
        tool_deltas
            .iter()
            .any(|event| event.contains("{\\\"city\\\":\\\""))
    );
    assert!(tool_deltas.iter().any(|event| event.contains("Paris\\\"}")));
    assert!(
        tool_deltas
            .iter()
            .any(|event| event.contains("{\\\"timezone\\\":\\\""))
    );
    assert!(tool_deltas.iter().any(|event| event.contains("UTC\\\"}")));

    let tool_stops = events
        .iter()
        .filter(|event| event.contains("event: content_block_stop"))
        .count();
    assert_eq!(tool_stops, 2);
}

#[tokio::test]
async fn anthropic_stream_renderer_deduplicates_cumulative_tool_argument_snapshots() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: None,
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "tool_calls"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_4".to_string(),
                    correlation_id: "req_stream_4".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(2),
                        total_tokens: Some(9),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_4",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "lookup_weather",
                                    "arguments": "{\"city\":\""
                                }
                            }]
                        }
                    }]
                }),
            }),
            Ok(ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "id": "chatcmpl_4",
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "function": {
                                    "arguments": "{\"city\":\"Paris\"}"
                                }
                            }]
                        }
                    }]
                }),
            }),
        ])),
        completion: completion_rx,
        request_id: "req_stream_4".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    let tool_deltas = events
        .iter()
        .filter(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"input_json_delta\"")
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_deltas.len(), 2);
    assert!(tool_deltas[0].contains("{\\\"city\\\":\\\""));
    assert!(tool_deltas[1].contains("Paris\\\"}"));
    assert!(!tool_deltas[1].contains("{\\\"city\\\":\\\"Paris\\\"}"));
}

#[tokio::test]
async fn anthropic_stream_renderer_normalizes_double_encoded_and_prefixed_tool_arguments() {
    let (completion_tx, completion_rx) = oneshot::channel();
    assert!(
        completion_tx
            .send(Ok(CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("gpt-4o-mini".to_string()),
                    output_text: None,
                    raw: serde_json::json!({
                        "choices": [{
                            "finish_reason": "tool_calls"
                        }]
                    }),
                },
                report: RequestReport {
                    request_id: "req_stream_5".to_string(),
                    correlation_id: "req_stream_5".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
                    kind: RequestKind::Chat,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(2),
                        total_tokens: Some(9),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    error_kind: None,
                    stream: None,
                    metadata: HashMap::new(),
                },
            }))
            .is_ok()
    );

    let response = render_anthropic_chat_session(ProxySession::Streaming(StreamingResponse {
        stream: Box::pin(futures_util::stream::iter(vec![Ok(ChatResponseChunk {
            delta: None,
            raw: serde_json::json!({
                "id": "chatcmpl_5",
                "model": "gpt-4o-mini",
                "choices": [{
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_weather",
                                "type": "function",
                                "function": {
                                    "name": "lookup_weather",
                                    "arguments": "\"{\\\"city\\\":\\\"Paris\\\"}\""
                                }
                            },
                            {
                                "index": 1,
                                "id": "call_time",
                                "type": "function",
                                "function": {
                                    "name": "lookup_time",
                                    "arguments": "{}{\"timezone\":\"UTC\"}"
                                }
                            }
                        ]
                    }
                }]
            }),
        })])),
        completion: completion_rx,
        request_id: "req_stream_5".to_string(),
        request_metadata: HashMap::from([(
            ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
            "claude-3-5-sonnet-latest".to_string(),
        )]),
    }));

    let (_, body) = response.into_parts();
    let ProtocolResponseBody::ServerSentEvents(stream) = body else {
        panic!("expected sse body");
    };

    let events = stream
        .map(|item| String::from_utf8(item.expect("sse chunk").to_vec()).expect("utf8 chunk"))
        .collect::<Vec<_>>()
        .await;

    let tool_deltas = events
        .iter()
        .filter(|event| {
            event.contains("event: content_block_delta")
                && event.contains("\"type\":\"input_json_delta\"")
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_deltas.len(), 2);
    assert!(
        tool_deltas
            .iter()
            .any(|event| event.contains("{\\\"city\\\":\\\"Paris\\\"}"))
    );
    assert!(
        tool_deltas
            .iter()
            .any(|event| event.contains("{\\\"timezone\\\":\\\"UTC\\\"}"))
    );
    assert!(
        tool_deltas
            .iter()
            .all(|event| !event.contains("\\\"{\\\\\\\""))
    );
    assert!(tool_deltas.iter().all(|event| !event.contains("{}{")));
}
