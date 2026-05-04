use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use unigateway_core::{Endpoint, ModelPolicy, ProviderKind, SecretString};
use unigateway_protocol::{anthropic_payload_to_chat_request, openai_payload_to_chat_request};

use super::super::dispatch::{
    HostDispatchOutcome, HostDispatchTarget, HostProtocol, HostRequest, dispatch_request,
};
use super::support::{
    NoopPoolHost, StaticTransport, dispatched_json_body, pool_with_endpoint, seen_request_json,
    sse_body, test_engine,
};
use crate::host::HostContext;

#[tokio::test]
async fn dispatch_openai_request_to_anthropic_upstream_renders_openai_response() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(StaticTransport {
        response: Some(unigateway_core::transport::TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "id": "msg_123",
                "type": "message",
                "model": "claude-3-5-sonnet",
                "content": [{"type": "text", "text": "pong from claude"}],
                "usage": {"input_tokens": 7, "output_tokens": 5}
            }))
            .expect("anthropic response body"),
        }),
        stream_chunks: None,
        seen: seen.clone(),
    });
    let engine = test_engine(transport);
    let pool = pool_with_endpoint(
        "pool-openai-to-anthropic",
        Endpoint {
            endpoint_id: "anthropic-main".to_string(),
            provider_name: Some("Anthropic Main".to_string()),
            source_endpoint_id: Some("anthropic:main".to_string()),
            provider_family: Some("anthropic".to_string()),
            provider_kind: ProviderKind::Anthropic,
            driver_id: "anthropic".to_string(),
            base_url: "https://api.anthropic.com/v1/".to_string(),
            api_key: SecretString::new("sk-ant"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::new(),
        },
    );
    engine.upsert_pool(pool.clone()).await.expect("upsert pool");

    let payload = json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "system", "content": "be concise"},
            {"role": "user", "content": "Reply with pong."}
        ],
        "max_tokens": 32
    });
    let request = openai_payload_to_chat_request(&payload, "gpt-4o-mini").expect("openai parse");
    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);

    let body = dispatched_json_body(
        dispatch_request(
            &context,
            HostDispatchTarget::Pool(pool),
            HostProtocol::OpenAiChat,
            Some("anthropic"),
            HostRequest::Chat(request),
        )
        .await
        .expect("dispatch"),
    );

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
        Some("pong from claude")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(Value::as_u64),
        Some(12)
    );

    let request_json = seen_request_json(&seen);
    assert_eq!(
        request_json.get("system").and_then(Value::as_str),
        Some("be concise")
    );
    assert_eq!(
        request_json
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("Reply with pong.")
    );
}

#[tokio::test]
async fn dispatch_anthropic_request_to_openai_upstream_renders_anthropic_response() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(StaticTransport {
        response: Some(unigateway_core::transport::TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "model": "gpt-4o-mini",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "reasoning_content": "need to inspect weather first",
                        "content": "Paris is sunny"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 11,
                    "completion_tokens": 7,
                    "total_tokens": 18
                }
            }))
            .expect("openai response body"),
        }),
        stream_chunks: None,
        seen: seen.clone(),
    });
    let engine = test_engine(transport);
    let pool = pool_with_endpoint(
        "pool-anthropic-to-openai",
        Endpoint {
            endpoint_id: "openai-main".to_string(),
            provider_name: Some("OpenAI Main".to_string()),
            source_endpoint_id: Some("openai:main".to_string()),
            provider_family: Some("openai".to_string()),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: "https://api.openai.com/v1/".to_string(),
            api_key: SecretString::new("sk-openai"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::new(),
        },
    );
    engine.upsert_pool(pool.clone()).await.expect("upsert pool");

    let payload = json!({
        "model": "claude-opus-4.7",
        "system": "be concise",
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "What is the weather in Paris?"}]
            }
        ],
        "max_tokens": 64,
        "stream": false
    });
    let request =
        anthropic_payload_to_chat_request(&payload, "claude-opus-4.7").expect("anthropic parse");
    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);

    let body = dispatched_json_body(
        dispatch_request(
            &context,
            HostDispatchTarget::Pool(pool),
            HostProtocol::AnthropicMessages,
            Some("openai"),
            HostRequest::Chat(request),
        )
        .await
        .expect("dispatch"),
    );

    assert_eq!(body.get("type").and_then(Value::as_str), Some("message"));
    assert_eq!(
        body.get("model").and_then(Value::as_str),
        Some("gpt-4o-mini")
    );
    assert_eq!(
        body.get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|block| block.get("type"))
            .and_then(Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        body.get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.get(1))
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("Paris is sunny")
    );
    assert_eq!(
        body.get("usage")
            .and_then(|usage| usage.get("output_tokens"))
            .and_then(Value::as_u64),
        Some(7)
    );

    let request_json = seen_request_json(&seen);
    assert_eq!(
        request_json.get("model").and_then(Value::as_str),
        Some("claude-opus-4.7")
    );
    assert_eq!(
        request_json
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str),
        Some("system")
    );
    assert_eq!(
        request_json
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.get(1))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("What is the weather in Paris?")
    );
}

#[tokio::test]
async fn dispatch_openai_stream_request_to_anthropic_upstream_renders_openai_sse() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(StaticTransport {
        response: None,
        stream_chunks: Some(vec![
            b"event: message_start\ndata: {\"type\":\"message_start\",\"model\":\"claude-3-5-sonnet\"}\n\n".to_vec(),
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_vec(),
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n".to_vec(),
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
        ]),
        seen: seen.clone(),
    });
    let engine = test_engine(transport);
    let pool = pool_with_endpoint(
        "pool-openai-stream-to-anthropic",
        Endpoint {
            endpoint_id: "anthropic-main".to_string(),
            provider_name: Some("Anthropic Main".to_string()),
            source_endpoint_id: Some("anthropic:main".to_string()),
            provider_family: Some("anthropic".to_string()),
            provider_kind: ProviderKind::Anthropic,
            driver_id: "anthropic".to_string(),
            base_url: "https://api.anthropic.com/v1/".to_string(),
            api_key: SecretString::new("sk-ant"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::new(),
        },
    );
    engine.upsert_pool(pool.clone()).await.expect("upsert pool");

    let payload = json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "system", "content": "be concise"},
            {"role": "user", "content": "Reply with hello."}
        ],
        "stream": true,
        "max_tokens": 32
    });
    let request = openai_payload_to_chat_request(&payload, "gpt-4o-mini").expect("openai parse");
    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);

    let response = match dispatch_request(
        &context,
        HostDispatchTarget::Pool(pool),
        HostProtocol::OpenAiChat,
        Some("anthropic"),
        HostRequest::Chat(request),
    )
    .await
    .expect("dispatch")
    {
        HostDispatchOutcome::Response(response) => response,
        HostDispatchOutcome::PoolNotFound => panic!("expected resolved pool"),
    };

    let chunks = sse_body(response).await;
    assert_eq!(chunks.len(), 4);
    assert!(chunks[0].contains("\"role\":\"assistant\""));
    assert!(chunks[0].contains("\"model\":\"claude-3-5-sonnet\""));
    assert!(chunks[1].contains("\"content\":\"hello\""));
    assert!(chunks[2].contains("\"finish_reason\":\"stop\""));
    assert_eq!(chunks[3], "data: [DONE]\n\n");

    let request_json = seen_request_json(&seen);
    assert_eq!(
        request_json.get("stream").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        request_json.get("system").and_then(Value::as_str),
        Some("be concise")
    );
    assert_eq!(
        request_json
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("Reply with hello.")
    );
}

#[tokio::test]
async fn dispatch_anthropic_stream_request_to_openai_upstream_renders_anthropic_sse() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(StaticTransport {
        response: None,
        stream_chunks: Some(vec![
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_vec(),
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n".to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]),
        seen: seen.clone(),
    });
    let engine = test_engine(transport);
    let pool = pool_with_endpoint(
        "pool-anthropic-stream-to-openai",
        Endpoint {
            endpoint_id: "openai-main".to_string(),
            provider_name: Some("OpenAI Main".to_string()),
            source_endpoint_id: Some("openai:main".to_string()),
            provider_family: Some("openai".to_string()),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: "https://api.openai.com/v1/".to_string(),
            api_key: SecretString::new("sk-openai"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::new(),
        },
    );
    engine.upsert_pool(pool.clone()).await.expect("upsert pool");

    let payload = json!({
        "model": "claude-opus-4.7",
        "system": "be concise",
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "Reply with hello."}]
            }
        ],
        "max_tokens": 64
    });
    let request =
        anthropic_payload_to_chat_request(&payload, "claude-opus-4.7").expect("anthropic parse");
    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);

    let response = match dispatch_request(
        &context,
        HostDispatchTarget::Pool(pool),
        HostProtocol::AnthropicMessages,
        Some("openai"),
        HostRequest::Chat(request),
    )
    .await
    .expect("dispatch")
    {
        HostDispatchOutcome::Response(response) => response,
        HostDispatchOutcome::PoolNotFound => panic!("expected resolved pool"),
    };

    let chunks = sse_body(response).await;
    assert!(chunks.iter().any(|chunk| {
        chunk.contains("event: message_start") && chunk.contains("\"model\":\"claude-opus-4.7\"")
    }));
    assert!(chunks.iter().any(|chunk| chunk.contains("event: ping")));
    assert!(chunks.iter().any(|chunk| {
        chunk.contains("event: content_block_start") && chunk.contains("\"type\":\"text\"")
    }));
    assert!(chunks.iter().any(|chunk| {
        chunk.contains("event: content_block_delta") && chunk.contains("\"text\":\"hel\"")
    }));
    assert!(chunks.iter().any(|chunk| {
        chunk.contains("event: content_block_delta") && chunk.contains("\"text\":\"lo\"")
    }));
    assert!(chunks.iter().any(|chunk| {
        chunk.contains("event: message_delta") && chunk.contains("\"output_tokens\":2")
    }));
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.contains("event: message_stop"))
    );

    let request_json = seen_request_json(&seen);
    assert_eq!(
        request_json.get("stream").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        request_json.get("model").and_then(Value::as_str),
        Some("claude-opus-4.7")
    );
    assert_eq!(
        request_json
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str),
        Some("system")
    );
}
