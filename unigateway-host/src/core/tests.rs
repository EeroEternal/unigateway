use std::collections::HashMap;

use futures_util::StreamExt;
use http::StatusCode;
use tokio::sync::oneshot;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, Endpoint, EndpointRef, ExecutionPlan,
    ExecutionTarget, GatewayError, ModelPolicy, ProviderKind, ProxyResponsesRequest, ProxySession,
    RequestReport, SecretString, StreamingResponse,
};
use unigateway_protocol::testing::{
    OpenAiChatStreamAdapter, anthropic_completed_chat_body, openai_completed_chat_body,
    openai_sse_chunks_from_chat_chunk,
};
use unigateway_protocol::{
    ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY, ProtocolResponseBody, render_anthropic_chat_session,
};

use super::super::env::{EnvProvider, build_env_pool};
use super::dispatch::{should_preserve_stream_error, without_response_tools};
use super::targeting::{build_openai_compatible_target, endpoint_matches_hint};

fn endpoint() -> Endpoint {
    Endpoint {
        endpoint_id: "deepseek-main".to_string(),
        provider_name: Some("DeepSeek-Main".to_string()),
        source_endpoint_id: Some("deepseek:global".to_string()),
        provider_family: Some("deepseek".to_string()),
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url: "https://api.example.com".to_string(),
        api_key: SecretString::new("sk-test"),
        model_policy: ModelPolicy::default(),
        enabled: true,
        metadata: HashMap::new(),
    }
}

#[test]
fn endpoint_hint_matching_supports_existing_product_forms() {
    let endpoint = endpoint();
    assert!(endpoint_matches_hint(&endpoint, "deepseek-main"));
    assert!(endpoint_matches_hint(&endpoint, "DeepSeek-Main"));
    assert!(endpoint_matches_hint(&endpoint, "deepseek:global"));
    assert!(endpoint_matches_hint(&endpoint, "deepseek"));
    assert!(!endpoint_matches_hint(&endpoint, "zhipu"));
}

#[test]
fn env_openai_pool_matches_basic_openai_hints() {
    let pool = build_env_pool(
        EnvProvider::OpenAi,
        "gpt-4o-mini",
        "https://api.openai.com",
        "sk-test",
    );
    let endpoint = pool.endpoints.first().expect("endpoint");

    assert!(endpoint_matches_hint(endpoint, "env-openai"));
    assert!(endpoint_matches_hint(endpoint, "openai"));
    assert!(!endpoint_matches_hint(endpoint, "deepseek"));
}

#[test]
fn env_anthropic_pool_matches_basic_anthropic_hints() {
    let pool = build_env_pool(
        EnvProvider::Anthropic,
        "claude-3-5-sonnet",
        "https://api.anthropic.com",
        "sk-ant",
    );
    let endpoint = pool.endpoints.first().expect("endpoint");

    assert!(endpoint_matches_hint(endpoint, "env-anthropic"));
    assert!(endpoint_matches_hint(endpoint, "anthropic"));
    assert!(!endpoint_matches_hint(endpoint, "openai"));
}

#[test]
fn responses_tool_stripping_clears_tool_fields_only() {
    let request = without_response_tools(ProxyResponsesRequest {
        model: "gpt-4.1-mini".to_string(),
        input: Some(serde_json::json!("hello")),
        instructions: Some("be terse".to_string()),
        temperature: Some(0.1),
        top_p: Some(0.8),
        max_output_tokens: Some(128),
        stream: true,
        tools: Some(serde_json::json!([])),
        tool_choice: Some(serde_json::json!("auto")),
        previous_response_id: Some("resp_prev".to_string()),
        request_metadata: Some(serde_json::json!({"trace_id": "abc"})),
        extra: std::collections::HashMap::new(),
        metadata: HashMap::new(),
    });

    assert!(request.tools.is_none());
    assert!(request.tool_choice.is_none());
    assert_eq!(request.instructions.as_deref(), Some("be terse"));
    assert_eq!(request.previous_response_id.as_deref(), Some("resp_prev"));
}

#[test]
fn stream_error_preservation_prefers_routing_failures() {
    assert!(should_preserve_stream_error(
        &GatewayError::InvalidRequest("bad target".to_string()),
        &GatewayError::UpstreamHttp {
            status: 500,
            body: Some("boom".to_string()),
            endpoint_id: "ep-1".to_string(),
        }
    ));
    assert!(should_preserve_stream_error(
        &GatewayError::Transport {
            message: "stream failed".to_string(),
            endpoint_id: Some("ep-1".to_string()),
        },
        &GatewayError::PoolNotFound("svc".to_string()),
    ));
    assert!(!should_preserve_stream_error(
        &GatewayError::Transport {
            message: "stream failed".to_string(),
            endpoint_id: Some("ep-1".to_string()),
        },
        &GatewayError::UpstreamHttp {
            status: 500,
            body: Some("boom".to_string()),
            endpoint_id: "ep-1".to_string(),
        }
    ));
}

#[test]
fn openai_compatible_target_filters_mixed_pool() {
    let anthropic_endpoint = Endpoint {
        endpoint_id: "anthropic-main".to_string(),
        provider_name: Some("anthropic-main".to_string()),
        source_endpoint_id: None,
        provider_family: Some("anthropic".to_string()),
        provider_kind: ProviderKind::Anthropic,
        driver_id: "anthropic".to_string(),
        base_url: "https://api.anthropic.com".to_string(),
        api_key: SecretString::new("sk-ant"),
        model_policy: ModelPolicy::default(),
        enabled: true,
        metadata: HashMap::new(),
    };

    let target = build_openai_compatible_target(&[endpoint(), anthropic_endpoint], "pool-1", None)
        .expect("target");

    assert_eq!(
        target,
        ExecutionTarget::Plan(ExecutionPlan {
            pool_id: Some("pool-1".to_string()),
            candidates: vec![EndpointRef {
                endpoint_id: "deepseek-main".to_string(),
            }],
            load_balancing_override: None,
            retry_policy_override: None,
            metadata: HashMap::new(),
        })
    );
}

#[test]
fn openai_compatible_target_keeps_pool_when_all_endpoints_match() {
    let target = build_openai_compatible_target(&[endpoint()], "pool-1", None).expect("target");

    assert_eq!(
        target,
        ExecutionTarget::Pool {
            pool_id: "pool-1".to_string(),
        }
    );
}

#[test]
fn openai_compatible_target_rejects_target_without_match() {
    let error = build_openai_compatible_target(&[endpoint()], "pool-1", Some("anthropic"))
        .expect_err("target mismatch");

    assert_eq!(error.to_string(), "no provider matches target 'anthropic'");
}

#[test]
fn anthropic_completed_body_normalizes_openai_provider_output() {
    let body = anthropic_completed_chat_body(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("glm-4.5".to_string()),
            output_text: Some("pong".to_string()),
            raw: serde_json::json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
            }),
        },
        report: RequestReport {
            request_id: "req_123".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "zhipu-main".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            attempts: Vec::new(),
            usage: Some(unigateway_core::TokenUsage {
                input_tokens: Some(10),
                output_tokens: Some(4),
                total_tokens: Some(14),
            }),
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            metadata: HashMap::from([(
                unigateway_protocol::ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    assert_eq!(
        body.get("type").and_then(serde_json::Value::as_str),
        Some("message")
    );
    assert_eq!(
        body.get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("pong")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(serde_json::Value::as_u64),
        Some(10)
    );
}

#[test]
fn anthropic_completed_body_converts_openai_tool_calls_to_tool_use() {
    let body = anthropic_completed_chat_body(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("gpt-4o-mini".to_string()),
            output_text: None,
            raw: serde_json::json!({
                "id": "chatcmpl_456",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "I'll call a tool",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "lookup_weather",
                                "arguments": "{\"city\":\"Paris\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 9,
                    "completion_tokens": 3,
                    "total_tokens": 12,
                    "cache_creation_input_tokens": 2,
                    "cache_read_input_tokens": 1
                }
            }),
        },
        report: RequestReport {
            request_id: "req_tool_1".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "zhipu-main".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            attempts: Vec::new(),
            usage: None,
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            metadata: HashMap::from([(
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    let content = body
        .get("content")
        .and_then(serde_json::Value::as_array)
        .expect("content array");

    assert_eq!(
        content[0].get("text").and_then(serde_json::Value::as_str),
        Some("I'll call a tool")
    );
    assert_eq!(
        content[1].get("type").and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        content[1]
            .get("input")
            .and_then(|input| input.get("city"))
            .and_then(serde_json::Value::as_str),
        Some("Paris")
    );
    assert_eq!(
        body.get("stop_reason").and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        body.get("id").and_then(serde_json::Value::as_str),
        Some("msg_chatcmpl_456")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(serde_json::Value::as_u64),
        Some(9)
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("cache_creation_input_tokens"))
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("cache_read_input_tokens"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn anthropic_completed_body_converts_openai_reasoning_to_thinking_block() {
    let body = anthropic_completed_chat_body(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("gpt-4o-mini".to_string()),
            output_text: Some("Paris is sunny".to_string()),
            raw: serde_json::json!({
                "id": "chatcmpl_reasoning_1",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "reasoning_content": "need to inspect weather first",
                        "content": "Paris is sunny"
                    },
                    "finish_reason": "stop"
                }]
            }),
        },
        report: RequestReport {
            request_id: "req_reasoning_1".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "zhipu-main".to_string(),
            selected_provider: ProviderKind::OpenAiCompatible,
            attempts: Vec::new(),
            usage: None,
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            metadata: HashMap::from([(
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    let content = body
        .get("content")
        .and_then(serde_json::Value::as_array)
        .expect("content array");

    assert_eq!(
        content[0].get("type").and_then(serde_json::Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        content[0]
            .get("thinking")
            .and_then(serde_json::Value::as_str),
        Some("need to inspect weather first")
    );
    assert_eq!(
        content[1].get("text").and_then(serde_json::Value::as_str),
        Some("Paris is sunny")
    );
}

#[test]
fn openai_completed_body_normalizes_anthropic_provider_output() {
    let body = openai_completed_chat_body(CompletedResponse {
        response: ChatResponseFinal {
            model: Some("claude-3-5-sonnet".to_string()),
            output_text: Some("pong".to_string()),
            raw: serde_json::json!({
                "id": "msg_123",
                "type": "message",
            }),
        },
        report: RequestReport {
            request_id: "req_456".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "anthropic-main".to_string(),
            selected_provider: ProviderKind::Anthropic,
            attempts: Vec::new(),
            usage: Some(unigateway_core::TokenUsage {
                input_tokens: Some(10),
                output_tokens: Some(4),
                total_tokens: Some(14),
            }),
            latency_ms: 10,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            metadata: HashMap::new(),
        },
    });

    assert_eq!(
        body.get("object").and_then(serde_json::Value::as_str),
        Some("chat.completion")
    );
    assert_eq!(
        body.get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("pong")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
}

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
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: ProviderKind::OpenAiCompatible,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(2),
                        total_tokens: Some(9),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
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
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: ProviderKind::OpenAiCompatible,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(6),
                        output_tokens: Some(4),
                        total_tokens: Some(10),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
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
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: ProviderKind::OpenAiCompatible,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(5),
                        output_tokens: Some(2),
                        total_tokens: Some(7),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
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
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: ProviderKind::OpenAiCompatible,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        total_tokens: Some(14),
                    }),
                    latency_ms: 8,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
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

    let role_json: serde_json::Value = serde_json::from_slice(role_payload).expect("role json");
    let content_json: serde_json::Value =
        serde_json::from_slice(content_payload).expect("content json");
    let stop_json: serde_json::Value = serde_json::from_slice(stop_payload).expect("stop json");

    assert_eq!(
        role_json
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("role"))
            .and_then(serde_json::Value::as_str),
        Some("assistant")
    );
    assert_eq!(
        content_json
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        stop_json
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(serde_json::Value::as_str),
        Some("stop")
    );
}
