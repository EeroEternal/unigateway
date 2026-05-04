use std::collections::HashMap;

use serde_json::Value;
use unigateway_core::{ChatResponseFinal, CompletedResponse, RequestKind, RequestReport};
use unigateway_protocol::ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY;
use unigateway_protocol::testing::{anthropic_completed_chat_body, openai_completed_chat_body};

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
            correlation_id: "req_123".to_string(),
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
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    assert_eq!(body.get("type").and_then(Value::as_str), Some("message"));
    assert_eq!(
        body.get("content")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str),
        Some("pong")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(Value::as_u64),
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
            correlation_id: "req_tool_1".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "zhipu-main".to_string(),
            selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: Vec::new(),
            usage: None,
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    let content = body
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");

    assert_eq!(
        content[0].get("text").and_then(Value::as_str),
        Some("I'll call a tool")
    );
    assert_eq!(
        content[1].get("type").and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        content[1]
            .get("input")
            .and_then(|input| input.get("city"))
            .and_then(Value::as_str),
        Some("Paris")
    );
    assert_eq!(
        body.get("stop_reason").and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        body.get("id").and_then(Value::as_str),
        Some("msg_chatcmpl_456")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(Value::as_u64),
        Some(9)
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64),
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
            correlation_id: "req_reasoning_1".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "zhipu-main".to_string(),
            selected_provider: unigateway_core::ProviderKind::OpenAiCompatible,
            kind: RequestKind::Chat,
            attempts: Vec::new(),
            usage: None,
            latency_ms: 12,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::from([(
                ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
                "claude-3-5-sonnet-latest".to_string(),
            )]),
        },
    });

    let content = body
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");

    assert_eq!(
        content[0].get("type").and_then(Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        content[0].get("thinking").and_then(Value::as_str),
        Some("need to inspect weather first")
    );
    assert_eq!(
        content[1].get("text").and_then(Value::as_str),
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
            correlation_id: "req_456".to_string(),
            pool_id: Some("svc".to_string()),
            selected_endpoint_id: "anthropic-main".to_string(),
            selected_provider: unigateway_core::ProviderKind::Anthropic,
            kind: RequestKind::Chat,
            attempts: Vec::new(),
            usage: Some(unigateway_core::TokenUsage {
                input_tokens: Some(10),
                output_tokens: Some(4),
                total_tokens: Some(14),
            }),
            latency_ms: 10,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::new(),
        },
    });

    assert_eq!(
        body.get("object").and_then(Value::as_str),
        Some("chat.completion")
    );
    assert_eq!(
        body.get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
        Some("pong")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
        Some(4)
    );
}

#[test]
fn anthropic_response_body_preserves_thinking_block_structure() {
    let response = CompletedResponse {
        response: ChatResponseFinal {
            model: Some("claude-3-7-sonnet-latest".to_string()),
            output_text: Some("The answer is 42".to_string()),
            raw: serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "Let me analyze this...",
                        "signature": "Ep8DCkYICxgCKkC/3LZH..."
                    },
                    {
                        "type": "text",
                        "text": "The answer is 42"
                    }
                ],
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 50
                }
            }),
        },
        report: RequestReport {
            request_id: "req_test".to_string(),
            correlation_id: "req_test".to_string(),
            pool_id: None,
            kind: RequestKind::Chat,
            selected_endpoint_id: "anth-1".to_string(),
            selected_provider: unigateway_core::ProviderKind::Anthropic,
            usage: Some(unigateway_core::TokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
            }),
            attempts: vec![],
            latency_ms: 100,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::new(),
        },
    };

    let body = anthropic_completed_chat_body(response);

    assert_eq!(body.get("type").and_then(Value::as_str), Some("message"));
    assert_eq!(body.get("role").and_then(Value::as_str), Some("assistant"));

    let content = body
        .get("content")
        .and_then(Value::as_array)
        .expect("content should be array");
    assert_eq!(content.len(), 2);

    let thinking_block = &content[0];
    assert_eq!(
        thinking_block.get("type").and_then(Value::as_str),
        Some("thinking")
    );
    assert!(
        thinking_block.get("signature").is_some(),
        "Thinking signature must be preserved in Anthropic response"
    );

    let usage = body.get("usage").expect("usage should exist");
    assert_eq!(usage.get("input_tokens").and_then(Value::as_u64), Some(100));
    assert_eq!(usage.get("output_tokens").and_then(Value::as_u64), Some(50));
}

#[test]
fn openai_response_body_includes_reasoning_content() {
    let response = CompletedResponse {
        response: ChatResponseFinal {
            model: Some("claude-3-7-sonnet-latest".to_string()),
            output_text: Some("The answer is 42".to_string()),
            raw: serde_json::json!({
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "Let me analyze this...",
                        "signature": "Ep8DCkYICxgCKkC/3LZH..."
                    },
                    {
                        "type": "text",
                        "text": "The answer is 42"
                    }
                ]
            }),
        },
        report: RequestReport {
            request_id: "req_test".to_string(),
            correlation_id: "req_test".to_string(),
            pool_id: None,
            kind: RequestKind::Chat,
            selected_endpoint_id: "anth-1".to_string(),
            selected_provider: unigateway_core::ProviderKind::Anthropic,
            usage: Some(unigateway_core::TokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
            }),
            attempts: vec![],
            latency_ms: 100,
            started_at: std::time::SystemTime::UNIX_EPOCH,
            finished_at: std::time::SystemTime::UNIX_EPOCH,
            error_kind: None,
            stream: None,
            metadata: HashMap::new(),
        },
    };

    let body = openai_completed_chat_body(response);

    assert_eq!(
        body.get("object").and_then(Value::as_str),
        Some("chat.completion")
    );

    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .expect("choices should be array");
    assert!(!choices.is_empty());

    let first_choice = &choices[0];
    let message = first_choice.get("message").expect("message should exist");

    assert!(
        message.get("content").is_some(),
        "OpenAI response should have content"
    );

    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"));
    assert!(
        reasoning.is_some() || message.get("content").is_some(),
        "Reasoning content should be present or content should include reasoning"
    );
}
