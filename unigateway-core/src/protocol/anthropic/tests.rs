use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::{AnthropicDriver, build_chat_request};
use crate::GatewayError;
use crate::capabilities::EndpointCapabilities;
use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::pool::{ModelPolicy, ProviderKind, SecretString};
use crate::request::{
    ClientProtocol, ContentBlock, Message, MessageRole, ProxyChatRequest,
    THINKING_SIGNATURE_PLACEHOLDER_VALUE,
};
use crate::response::ProxySession;
use crate::transport::{
    HttpTransport, StreamingTransportResponse, TransportRequest, TransportResponse,
};

struct MockTransport {
    response: Option<TransportResponse>,
    stream_chunks: Option<Vec<Vec<u8>>>,
    seen: Arc<Mutex<Vec<TransportRequest>>>,
}

impl HttpTransport for MockTransport {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, crate::GatewayError>> {
        let seen = self.seen.clone();
        let response = self.response.clone().expect("missing non-stream response");
        Box::pin(async move {
            seen.lock().expect("seen lock").push(request);
            Ok(response)
        })
    }

    fn send_stream(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<StreamingTransportResponse, crate::GatewayError>> {
        let seen = self.seen.clone();
        let chunks = self.stream_chunks.clone().expect("missing stream chunks");

        Box::pin(async move {
            seen.lock().expect("seen lock").push(request);
            Ok(StreamingTransportResponse {
                status: 200,
                headers: HashMap::new(),
                stream: Box::pin(futures_util::stream::iter(
                    chunks.into_iter().map(Ok::<Vec<u8>, GatewayError>),
                )),
            })
        })
    }
}

fn endpoint() -> DriverEndpointContext {
    DriverEndpointContext {
        endpoint_id: "anth-1".to_string(),
        provider_kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com/v1/".to_string(),
        api_key: SecretString::new("sk-ant"),
        model_policy: ModelPolicy::default(),
        capabilities: EndpointCapabilities::default(),
        metadata: HashMap::from([("pool_id".to_string(), "beta".to_string())]),
        forward_metadata_as_headers: None,
    }
}

#[test]
fn build_chat_request_moves_system_messages_to_top_level_field() {
    let request = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: vec![
                Message::text(MessageRole::System, "be concise"),
                Message::text(MessageRole::User, "hello"),
            ],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: Some(0.2),
            top_p: None,
            top_k: Some(8),
            max_tokens: None,
            stop_sequences: Some(json!(["DONE", "HALT"])),
            stream: false,
            gateway_fields: HashMap::new(),
            extra: HashMap::new(),
            metadata: HashMap::new(),
        },
    )
    .expect("anthropic request");

    assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
    assert_eq!(
        request.headers.get("x-api-key").map(String::as_str),
        Some("sk-ant")
    );

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("system").and_then(serde_json::Value::as_str),
        Some("be concise")
    );
    assert_eq!(
        body.get("max_tokens").and_then(serde_json::Value::as_u64),
        Some(1024)
    );
    assert_eq!(
        body.get("top_k").and_then(serde_json::Value::as_u64),
        Some(8)
    );
    assert_eq!(
        body.get("stop_sequences")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn build_chat_request_preserves_structured_image_blocks_without_raw_messages() {
    let request = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: vec![Message::from_blocks(
                MessageRole::User,
                vec![
                    ContentBlock::Text {
                        text: "describe this".to_string(),
                    },
                    ContentBlock::Image {
                        source: json!({
                            "type": "url",
                            "url": "https://example.com/a.png"
                        }),
                        detail: Some("high".to_string()),
                    },
                ],
            )],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: Some(128),
            stop_sequences: None,
            stream: false,
            gateway_fields: HashMap::new(),
            extra: HashMap::new(),
            metadata: HashMap::new(),
        },
    )
    .expect("anthropic request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.pointer("/messages/0/content/0/type")
            .and_then(Value::as_str),
        Some("text")
    );
    assert_eq!(
        body.pointer("/messages/0/content/1/type")
            .and_then(Value::as_str),
        Some("image")
    );
    assert_eq!(
        body.pointer("/messages/0/content/1/source/type")
            .and_then(Value::as_str),
        Some("url")
    );
    assert_eq!(
        body.pointer("/messages/0/content/1/source/url")
            .and_then(Value::as_str),
        Some("https://example.com/a.png")
    );
}

#[test]
fn build_chat_request_converts_openai_raw_messages_to_anthropic_messages() {
    let mut request = ProxyChatRequest {
        model: "claude-3-5-sonnet".to_string(),
        messages: Vec::new(),
        system: None,
        tools: Some(json!([{
            "type": "function",
            "function": {
                "name": "search",
                "description": "Search documents",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }
            }
        }])),
        tool_choice: Some(json!({
            "type": "function",
            "function": {"name": "search"}
        })),
        raw_messages: Some(json!([
            {"role": "system", "content": "be precise"},
            {"role": "user", "content": "find rust examples"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust examples\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "result text"
            }
        ])),
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: Some(256),
        stop_sequences: None,
        stream: false,
        gateway_fields: HashMap::new(),
        extra: HashMap::new(),
        metadata: HashMap::new(),
    };
    request.set_client_protocol(ClientProtocol::OpenAiChat);
    request.mark_openai_raw_messages();

    let request = build_chat_request(&mut endpoint(), &request).expect("anthropic request");

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("system").and_then(serde_json::Value::as_str),
        Some("be precise")
    );

    let messages = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .expect("messages");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("user")
    );
    assert_eq!(
        messages[0]
            .get("content")
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("find rust examples")
    );

    let tool_use = messages[1]
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.first())
        .expect("tool use block");
    assert_eq!(
        tool_use.get("type").and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(tool_use.get("id").and_then(Value::as_str), Some("call_1"));
    assert_eq!(tool_use.get("name").and_then(Value::as_str), Some("search"));
    assert_eq!(
        tool_use
            .get("input")
            .and_then(|input| input.get("query"))
            .and_then(Value::as_str),
        Some("rust examples")
    );

    let tool_result = messages[2]
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.first())
        .expect("tool result block");
    assert_eq!(
        tool_result.get("type").and_then(Value::as_str),
        Some("tool_result")
    );
    assert_eq!(
        tool_result.get("tool_use_id").and_then(Value::as_str),
        Some("call_1")
    );

    let tool = body
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| tools.first())
        .expect("converted tool");
    assert_eq!(tool.get("name").and_then(Value::as_str), Some("search"));
    assert!(tool.get("input_schema").is_some());

    assert_eq!(
        body.get("tool_choice")
            .and_then(|choice| choice.get("type"))
            .and_then(Value::as_str),
        Some("tool")
    );
}

#[test]
fn build_chat_request_preserves_anthropic_raw_messages() {
    let raw_messages = json!([{
        "role": "assistant",
        "content": [{
            "type": "thinking",
            "thinking": "original reasoning",
            "signature": "real-signature"
        }, {
            "type": "text",
            "text": "answer"
        }]
    }]);

    let request = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: Vec::new(),
            system: Some(json!("native system")),
            tools: Some(json!([{
                "name": "native_tool",
                "input_schema": {"type": "object", "properties": {}}
            }])),
            tool_choice: Some(json!({"type": "auto"})),
            raw_messages: Some(raw_messages.clone()),
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: Some(256),
            stop_sequences: None,
            stream: false,
            gateway_fields: HashMap::new(),
            extra: HashMap::new(),
            metadata: HashMap::from([(
                "unigateway.client_protocol".to_string(),
                ClientProtocol::AnthropicMessages
                    .as_metadata_value()
                    .to_string(),
            )]),
        },
    )
    .expect("anthropic request");

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(body.get("messages"), Some(&raw_messages));
    assert_eq!(
        body.pointer("/messages/0/content/0/signature")
            .and_then(Value::as_str),
        Some("real-signature")
    );
    assert_eq!(
        body.pointer("/tools/0/name").and_then(Value::as_str),
        Some("native_tool")
    );
    assert_eq!(
        body.pointer("/tool_choice/type").and_then(Value::as_str),
        Some("auto")
    );
}

#[test]
fn build_chat_request_rejects_placeholder_signature_in_anthropic_raw_messages() {
    let error = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: Vec::new(),
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: Some(json!([{
                "role": "assistant",
                "content": [{
                    "type": "thinking",
                    "thinking": "renderer-only reasoning",
                    "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE
                }]
            }])),
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: Some(256),
            stop_sequences: None,
            stream: false,
            gateway_fields: HashMap::new(),
            extra: HashMap::new(),
            metadata: HashMap::from([(
                "unigateway.client_protocol".to_string(),
                ClientProtocol::AnthropicMessages
                    .as_metadata_value()
                    .to_string(),
            )]),
        },
    )
    .expect_err("placeholder signature should be rejected");

    assert!(matches!(error, GatewayError::InvalidRequest(_)));
}

#[test]
fn build_chat_request_merges_anthropic_extra_without_overriding_core_fields() {
    let request = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-opus-4-6".to_string(),
            messages: vec![crate::request::Message::text(MessageRole::User, "hello")],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: Some(1400),
            stop_sequences: None,
            stream: false,
            gateway_fields: HashMap::new(),
            extra: HashMap::from([
                (
                    "thinking".to_string(),
                    json!({
                        "type": "enabled",
                        "budget_tokens": 1024,
                        "display": "omitted"
                    }),
                ),
                (
                    "output_config".to_string(),
                    json!({
                        "effort": "medium"
                    }),
                ),
                ("max_tokens".to_string(), json!(999)),
            ]),
            metadata: HashMap::from([(
                "unigateway.client_protocol".to_string(),
                ClientProtocol::AnthropicMessages
                    .as_metadata_value()
                    .to_string(),
            )]),
        },
    )
    .expect("anthropic request");

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("thinking"),
        Some(&json!({
            "type": "enabled",
            "budget_tokens": 1024,
            "display": "omitted"
        }))
    );
    assert_eq!(
        body.get("output_config"),
        Some(&json!({"effort": "medium"}))
    );
    assert_eq!(body.get("max_tokens").and_then(Value::as_u64), Some(1400));
}

#[tokio::test]
async fn anthropic_driver_executes_non_streaming_chat() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(MockTransport {
        response: Some(TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "model": "claude-3-5-sonnet",
                "content": [{"type": "text", "text": "hello from claude"}],
                "usage": {"input_tokens": 11, "output_tokens": 13}
            }))
            .expect("response body"),
        }),
        stream_chunks: None,
        seen: seen.clone(),
    });
    let driver = AnthropicDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "claude-3-5-sonnet".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: Some(256),
                stop_sequences: None,
                stream: false,
                gateway_fields: HashMap::new(),
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("chat result");

    match session {
        ProxySession::Completed(response) => {
            assert_eq!(
                response.response.output_text.as_deref(),
                Some("hello from claude")
            );
            assert_eq!(response.report.selected_endpoint_id, "anth-1");
            assert_eq!(
                response
                    .report
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.total_tokens),
                Some(24)
            );
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }

    assert_eq!(seen.lock().expect("seen lock").len(), 1);
}

#[tokio::test]
async fn anthropic_driver_executes_streaming_chat() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"event: message_start\ndata: {\"type\":\"message_start\",\"model\":\"claude-3-5-sonnet\"}\n\n".to_vec(),
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_vec(),
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n".to_vec(),
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = AnthropicDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "claude-3-5-sonnet".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: Some(128),
                stop_sequences: None,
                stream: true,
                gateway_fields: HashMap::new(),
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("streaming chat session");

    match session {
        ProxySession::Streaming(streaming) => {
            let chunks = streaming
                .stream
                .map(|item| item.expect("chunk"))
                .collect::<Vec<_>>()
                .await;
            assert_eq!(chunks.len(), 4);
            assert_eq!(chunks[1].delta.as_deref(), Some("hello"));

            let completion = streaming
                .completion
                .await
                .expect("completion receiver")
                .expect("completion result");
            assert_eq!(completion.report.request_id, streaming.request_id);
            assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
            assert_eq!(
                completion
                    .report
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.total_tokens),
                Some(15)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}

#[test]
fn build_chat_request_drops_top_p_when_both_temperature_and_top_p_present() {
    // Regression test: even if extra re-introduces top_p, the defensive check
    // after the extra merge must remove it when temperature is also present.
    let request = build_chat_request(
        &mut endpoint(),
        &ProxyChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            max_tokens: Some(1024),
            stop_sequences: None,
            stream: false,
            // Simulate a client that sends both params and extra somehow retains top_p
            gateway_fields: HashMap::new(),
            extra: HashMap::from([("top_p".to_string(), json!(0.9))]),
            metadata: HashMap::new(),
        },
    )
    .expect("anthropic request");

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert!(
        body.get("temperature").is_some(),
        "temperature should be present"
    );
    assert!(
        body.get("top_p").is_none(),
        "top_p must be removed when temperature is also present, but got: {}",
        body
    );
}

#[tokio::test]
async fn anthropic_driver_streaming_chat_completion_survives_dropped_stream() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"event: message_start\ndata: {\"type\":\"message_start\",\"model\":\"claude-3-5-sonnet\"}\n\n".to_vec(),
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_vec(),
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n".to_vec(),
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = AnthropicDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "claude-3-5-sonnet".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: Some(128),
                stop_sequences: None,
                stream: true,
                gateway_fields: HashMap::new(),
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("streaming chat session");

    match session {
        ProxySession::Streaming(streaming) => {
            let completion = streaming
                .into_completion()
                .await
                .expect("completion result after dropped stream");
            assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
            assert_eq!(
                completion
                    .report
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.total_tokens),
                Some(15)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}

// ===========================================================================
// Render-determinism golden contract
//
// Byte-level twin of the contract in `openai/tests.rs`; see the comment block
// there for the full rationale. Key invariants locked here:
//   1. Same logical request => byte-identical upstream payload on every render
//      (fresh `HashMap` instances each time, i.e. fresh SipHash seeds).
//   2. `THINKING_SIGNATURE_PLACEHOLDER_VALUE` never reaches an upstream request
//      body: OpenAI-format history is converted with thinking blocks omitted,
//      and Anthropic-format raw messages carrying the placeholder are rejected.
//
// Endpoint pinning caveat: every render uses a freshly constructed,
// fixed-value `DriverEndpointContext`. If retry/fallback switches to a
// DIFFERENT endpoint, the upstream prefix cache is invalidated by design;
// that scheduling-layer property is OUT of scope for this contract.
// ===========================================================================

/// Longest common byte prefix of two serialized bodies.
fn byte_common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Byte offset of the `],"model":"` boundary that follows the `messages`
/// array in a serialized Anthropic payload (compact form, sorted keys).
fn messages_array_close_offset(body: &[u8]) -> usize {
    const MARKER: &[u8] = b"],\"model\":\"";
    body.windows(MARKER.len())
        .rposition(|window| window == MARKER)
        .expect("serialized anthropic payload must contain the messages/model boundary")
}

/// A wide `extra` map designed to expose iteration-order dependence, plus a
/// deliberate temperature/top_p conflict for the defensive-removal path.
fn wide_extra() -> HashMap<String, Value> {
    let mut extra = HashMap::new();
    for i in 0..32 {
        extra.insert(format!("x_field_{i:02}"), json!(format!("value-{i}")));
    }
    // Must never override core fields (or_insert semantics).
    extra.insert("model".to_string(), json!("smuggled-model"));
    extra.insert("max_tokens".to_string(), json!(999));
    // Conflicts with the typed temperature; must be removed defensively.
    extra.insert("top_p".to_string(), json!(0.9));
    // Gateway-only fields are dropped before forwarding.
    extra.insert("_internal_flag".to_string(), json!("secret"));
    extra
}

fn golden_tool_calling_turn(messages: Value) -> ProxyChatRequest {
    let mut request = ProxyChatRequest {
        model: "claude-3-7-sonnet".to_string(),
        messages: Vec::new(),
        raw_messages: Some(messages),
        temperature: Some(0.2),
        top_p: None,
        top_k: None,
        max_tokens: Some(1024),
        stop_sequences: None,
        stream: false,
        system: Some(json!("You are terse.")),
        tools: Some(json!([{
            "name": "get_weather",
            "description": "Look up current weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}}
            }
        }])),
        tool_choice: Some(json!({"type": "auto"})),
        gateway_fields: HashMap::new(),
        extra: wide_extra(),
        metadata: HashMap::from([(
            "unigateway.client_protocol".to_string(),
            ClientProtocol::AnthropicMessages
                .as_metadata_value()
                .to_string(),
        )]),
    };
    request.set_client_protocol(ClientProtocol::AnthropicMessages);
    request
}

fn golden_turn_n_messages() -> Value {
    json!([
        {"role": "user", "content": [{"type": "text", "text": "What's the weather in Stockholm?"}]},
        {"role": "assistant", "content": [
            {"type": "tool_use", "id": "toolu_1", "name": "get_weather",
             "input": {"city": "Stockholm"}}
        ]},
        {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "12°C, clear"}
        ]}
    ])
}

const GOLDEN_TURN_N_MESSAGE_COUNT: usize = 3;

fn render_chat_body(request: &ProxyChatRequest) -> Vec<u8> {
    build_chat_request(&mut endpoint(), request)
        .expect("anthropic chat request must render")
        .body
        .expect("anthropic chat request must have a body")
}

#[test]
fn anthropic_chat_request_render_bytes_are_stable_across_repeated_renders() {
    let request = golden_tool_calling_turn(golden_turn_n_messages());

    let first = render_chat_body(&request);
    for _ in 0..31 {
        assert_eq!(render_chat_body(&request), first);
    }

    let body: Value = serde_json::from_slice(&first).expect("json body");
    // Extra-merge semantics that keep rendering deterministic:
    assert_eq!(
        body.get("model"),
        Some(&Value::String("claude-3-7-sonnet".to_string())),
        "extra must not override the resolved model"
    );
    assert_eq!(
        body.get("max_tokens"),
        Some(&json!(1024)),
        "extra must not override the typed max_tokens"
    );
    assert_eq!(body.get("system"), Some(&json!("You are terse.")));
    // Defensive conflict removal: typed temperature wins, extra top_p is gone.
    assert_eq!(body.get("temperature"), Some(&json!(f64::from(0.2_f32))));
    assert!(
        body.get("top_p").is_none(),
        "temperature/top_p conflict must be resolved deterministically"
    );
    assert!(body.get("_internal_flag").is_none());
    assert!(body.get("x_field_00").is_some());
}

#[test]
fn placeholder_signature_never_reaches_anthropic_upstream_payload() {
    // OpenAI-format history carrying renderer-only reasoning must be rendered
    // without any thinking block or signature leaking into the upstream bytes.
    let mut request = ProxyChatRequest {
        model: "claude-3-7-sonnet".to_string(),
        messages: Vec::new(),
        raw_messages: Some(json!([
            {"role": "user", "content": "hi"},
            {
                "role": "assistant",
                "content": "answer",
                "reasoning_content": "renderer-only reasoning",
                "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE
            }
        ])),
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: Some(256),
        stop_sequences: None,
        stream: false,
        system: None,
        tools: None,
        tool_choice: None,
        gateway_fields: HashMap::new(),
        extra: HashMap::new(),
        metadata: HashMap::from([(
            "unigateway.client_protocol".to_string(),
            ClientProtocol::OpenAiChat.as_metadata_value().to_string(),
        )]),
    };
    request.set_client_protocol(ClientProtocol::OpenAiChat);
    request.mark_openai_raw_messages();

    let first = render_chat_body(&request);
    for _ in 0..15 {
        assert_eq!(render_chat_body(&request), first);
    }

    assert!(
        !first
            .windows(THINKING_SIGNATURE_PLACEHOLDER_VALUE.len())
            .any(|window| window == THINKING_SIGNATURE_PLACEHOLDER_VALUE.as_bytes()),
        "placeholder signatures must never reach an upstream request body"
    );
    let body: Value = serde_json::from_slice(&first).expect("json body");
    let serialized = serde_json::to_string(&body).expect("re-serialize");
    assert!(
        !serialized.contains("\"thinking\""),
        "thinking blocks must be omitted from converted request payloads"
    );
    // The Anthropic-format rejection counterpart is covered by
    // build_chat_request_rejects_placeholder_signature_in_anthropic_raw_messages.
}

#[test]
fn anthropic_tool_calling_turns_keep_byte_identical_prefix_up_to_first_edit() {
    let render_turn = |messages: Value| render_chat_body(&golden_tool_calling_turn(messages));

    let turn_n = render_turn(golden_turn_n_messages());

    // Turn N+1 evolves append-only: one more exchange at the end.
    let mut turn_n_plus_1_messages = golden_turn_n_messages();
    turn_n_plus_1_messages
        .as_array_mut()
        .expect("messages array")
        .extend([
            json!({"role": "assistant", "content": [{"type": "text", "text": "It is 12°C and clear."}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "Thanks! And tomorrow?"}]}),
        ]);
    let turn_n_plus_1 = render_turn(turn_n_plus_1_messages);

    // Serialized bytes identical up to the close of the last shared message;
    // everything after the messages array identical as well.
    let boundary = messages_array_close_offset(&turn_n);
    assert_eq!(
        byte_common_prefix(&turn_n, &turn_n_plus_1),
        boundary,
        "append-only evolution must keep every byte before the first new \
         message identical, including all non-message fields"
    );
    let tail_n = &turn_n[boundary..];
    let tail_n1 = &turn_n_plus_1[messages_array_close_offset(&turn_n_plus_1)..];
    assert_eq!(
        tail_n, tail_n1,
        "non-message fields must render identically"
    );

    // Structural restatement, robust to future layout changes: removing the
    // appended elements from turn N+1's parsed payload and re-serializing
    // must reproduce turn N's bytes exactly.
    let mut truncated: Value = serde_json::from_slice(&turn_n_plus_1).expect("json body");
    truncated["messages"]
        .as_array_mut()
        .expect("messages array")
        .truncate(GOLDEN_TURN_N_MESSAGE_COUNT);
    assert_eq!(
        serde_json::to_vec(&truncated).expect("re-serialize"),
        turn_n,
        "turn N+1 must differ from turn N only by appended messages"
    );

    // Control: editing an early message invalidates the prefix at the edit
    // position — strictly earlier than the append-only boundary above.
    let mut edited_messages = golden_turn_n_messages();
    edited_messages[0]["content"][0]["text"] = json!("What's the weather in Paris?");
    let turn_n_edited = render_turn(edited_messages);
    let edited_prefix = byte_common_prefix(&turn_n, &turn_n_edited);
    assert!(
        edited_prefix < boundary,
        "an edit inside the history must invalidate the common prefix at or \
         before the edited position (got {edited_prefix} >= {boundary})"
    );
}
