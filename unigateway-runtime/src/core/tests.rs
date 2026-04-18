use std::collections::HashMap;

use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, Endpoint, EndpointRef, ExecutionPlan,
    ExecutionTarget, GatewayError, ModelPolicy, ProviderKind, ProxyResponsesRequest, RequestReport,
    SecretString,
};

use super::chat::{
    OpenAiChatStreamAdapter, anthropic_completed_chat_body, openai_completed_chat_body,
    openai_sse_chunks_from_chat_chunk,
};
use super::responses::{should_preserve_stream_error, without_response_tools};
use super::targeting::{
    build_env_anthropic_pool, build_env_openai_pool, build_openai_compatible_target,
    endpoint_matches_hint,
};

fn endpoint() -> Endpoint {
    Endpoint {
        endpoint_id: "deepseek-main".to_string(),
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url: "https://api.example.com".to_string(),
        api_key: SecretString::new("sk-test"),
        model_policy: ModelPolicy::default(),
        enabled: true,
        metadata: HashMap::from([
            ("provider_name".to_string(), "DeepSeek-Main".to_string()),
            (
                "source_endpoint_id".to_string(),
                "deepseek:global".to_string(),
            ),
            ("provider_family".to_string(), "deepseek".to_string()),
        ]),
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
    let pool = build_env_openai_pool("gpt-4o-mini", "https://api.openai.com", "sk-test");
    let endpoint = pool.endpoints.first().expect("endpoint");

    assert!(endpoint_matches_hint(endpoint, "env-openai"));
    assert!(endpoint_matches_hint(endpoint, "openai"));
    assert!(!endpoint_matches_hint(endpoint, "deepseek"));
}

#[test]
fn env_anthropic_pool_matches_basic_anthropic_hints() {
    let pool = build_env_anthropic_pool("claude-3-5-sonnet", "https://api.anthropic.com", "sk-ant");
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
    let body = anthropic_completed_chat_body(
        CompletedResponse {
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
                metadata: HashMap::new(),
            },
        },
        "claude-3-5-sonnet-latest",
    );

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
