use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::{
    OpenAiCompatibleDriver, build_chat_request, build_embeddings_request, build_responses_request,
    parse_responses_response,
};
use crate::GatewayError;
use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::pool::{ModelPolicy, ProviderKind, SecretString};
use crate::request::{
    Message, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
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
        endpoint_id: "ep-1".to_string(),
        provider_kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.example.com/v1/".to_string(),
        api_key: SecretString::new("sk-test"),
        model_policy: ModelPolicy {
            default_model: Some("gpt-4o-mini".to_string()),
            model_mapping: HashMap::from([("alias".to_string(), "mapped-model".to_string())]),
        },
        metadata: HashMap::from([("pool_id".to_string(), "alpha".to_string())]),
    }
}

#[test]
fn build_chat_request_maps_model_and_url() {
    let request = build_chat_request(
        &endpoint(),
        &ProxyChatRequest {
            model: "alias".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            max_tokens: Some(32),
            stop_sequences: None,
            stream: false,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        },
    )
    .expect("chat request");

    assert_eq!(request.url, "https://api.example.com/v1/chat/completions");
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer sk-test")
    );

    let body: serde_json::Value =
        serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("model").and_then(serde_json::Value::as_str),
        Some("mapped-model")
    );
}

#[test]
fn build_chat_request_merges_extra_without_overriding_core_fields() {
    let request = build_chat_request(
        &endpoint(),
        &ProxyChatRequest {
            model: "alias".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: Some(32),
            stop_sequences: None,
            stream: false,
            extra: HashMap::from([
                ("reasoning_effort".to_string(), json!("high")),
                ("max_completion_tokens".to_string(), json!(1024)),
                ("max_tokens".to_string(), json!(999)),
                ("model".to_string(), json!("wrong-model")),
            ]),
            metadata: HashMap::new(),
        },
    )
    .expect("chat request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("reasoning_effort").and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        body.get("max_completion_tokens").and_then(Value::as_u64),
        Some(1024)
    );
    assert_eq!(body.get("max_tokens").and_then(Value::as_u64), Some(32));
    assert_eq!(
        body.get("model").and_then(Value::as_str),
        Some("mapped-model")
    );
}

#[test]
fn build_chat_request_translates_anthropic_raw_messages_and_tool_choice() {
    let request = build_chat_request(
        &endpoint(),
        &ProxyChatRequest {
            model: "alias".to_string(),
            messages: Vec::new(),
            system: Some(json!("be concise")),
            tools: Some(json!([{
                "name": "lookup_weather",
                "description": "Look up the weather",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }
            }])),
            tool_choice: Some(json!({
                "type": "tool",
                "name": "lookup_weather"
            })),
            raw_messages: Some(json!([
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "weather in paris"}]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "thinking",
                            "thinking": "need weather first"
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "lookup_weather",
                            "input": {"city": "Paris"}
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_1",
                            "content": "18C"
                        }
                    ]
                }
            ])),
            temperature: None,
            top_p: None,
            top_k: Some(7),
            max_tokens: Some(64),
            stop_sequences: Some(json!(["DONE", "HALT"])),
            stream: false,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        },
    )
    .expect("chat request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages array");

    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("system")
    );
    assert_eq!(
        messages[0].get("content").and_then(Value::as_str),
        Some("be concise")
    );
    assert_eq!(
        messages[1]
            .get("content")
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.first())
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str),
        Some("weather in paris")
    );
    assert_eq!(
        messages[2]
            .get("tool_calls")
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(|call| call.get("function"))
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str),
        Some("{\"city\":\"Paris\"}")
    );
    assert_eq!(body.get("top_k").and_then(Value::as_u64), Some(7));
    assert_eq!(
        messages[3].get("tool_call_id").and_then(Value::as_str),
        Some("toolu_1")
    );
    assert_eq!(
        messages[3].get("role").and_then(Value::as_str),
        Some("tool")
    );
    assert_eq!(
        body.get("tool_choice")
            .and_then(|choice| choice.get("function"))
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str),
        Some("lookup_weather")
    );
    assert_eq!(
        body.get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get("type"))
            .and_then(Value::as_str),
        Some("function")
    );
    assert_eq!(
        body.get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get("function"))
            .and_then(|function| function.get("parameters"))
            .and_then(|parameters| parameters.get("required"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn build_chat_request_normalizes_string_any_tool_choice() {
    let request = build_chat_request(
        &endpoint(),
        &ProxyChatRequest {
            model: "alias".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            system: None,
            tools: Some(json!([{ "name": "lookup_weather" }])),
            tool_choice: Some(json!("any")),
            raw_messages: None,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        },
    )
    .expect("chat request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("tool_choice").and_then(Value::as_str),
        Some("required")
    );
}

#[test]
fn build_responses_request_forwards_supported_optional_fields() {
    let request = build_responses_request(
        &endpoint(),
        &ProxyResponsesRequest {
            model: "alias".to_string(),
            input: Some(json!([{"role": "user", "content": "hello"}])),
            instructions: Some("be terse".to_string()),
            temperature: Some(0.2),
            top_p: Some(0.9),
            max_output_tokens: Some(128),
            stream: true,
            tools: Some(json!([{
                "type": "function",
                "name": "lookup_weather",
                "description": "Look up current weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }
            }])),
            tool_choice: Some(json!("auto")),
            previous_response_id: Some("resp_prev".to_string()),
            request_metadata: Some(json!({"trace_id": "abc"})),
            extra: HashMap::from([("reasoning".to_string(), json!({"effort": "high"}))]),
            metadata: HashMap::new(),
        },
    )
    .expect("responses request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("model").and_then(Value::as_str),
        Some("mapped-model")
    );
    assert_eq!(
        body.get("instructions").and_then(Value::as_str),
        Some("be terse")
    );
    assert_eq!(
        body.get("max_output_tokens").and_then(Value::as_u64),
        Some(128)
    );
    assert_eq!(
        body.get("previous_response_id").and_then(Value::as_str),
        Some("resp_prev")
    );
    assert_eq!(
        body.get("tool_choice").and_then(Value::as_str),
        Some("auto")
    );
    assert_eq!(
        body.get("tools").and_then(Value::as_array).map(Vec::len),
        Some(1)
    );
    assert_eq!(
        body.get("metadata")
            .and_then(|value| value.get("trace_id"))
            .and_then(Value::as_str),
        Some("abc")
    );
    assert_eq!(
        body.get("reasoning")
            .and_then(|value| value.get("effort"))
            .and_then(Value::as_str),
        Some("high")
    );
}

#[test]
fn build_embeddings_request_preserves_encoding_format() {
    let request = build_embeddings_request(
        &endpoint(),
        &ProxyEmbeddingsRequest {
            model: "text-embedding-3-small".to_string(),
            input: vec!["hello".to_string()],
            encoding_format: Some("float".to_string()),
            metadata: HashMap::new(),
        },
    )
    .expect("embeddings request");

    let body: Value = serde_json::from_slice(&request.body.expect("body")).expect("json body");
    assert_eq!(
        body.get("model").and_then(Value::as_str),
        Some("gpt-4o-mini")
    );
    assert_eq!(
        body.get("encoding_format").and_then(Value::as_str),
        Some("float")
    );
}

#[test]
fn parse_responses_response_reads_responses_usage_shape() {
    let (response, usage) = parse_responses_response(
        &serde_json::to_vec(&json!({
            "id": "resp_1",
            "object": "response",
            "output_text": "hello",
            "usage": {
                "input_tokens": 7,
                "output_tokens": 5,
                "total_tokens": 12
            }
        }))
        .expect("response body"),
    )
    .expect("parse response");

    assert_eq!(response.output_text.as_deref(), Some("hello"));
    assert_eq!(usage.and_then(|usage| usage.total_tokens), Some(12));
}

#[tokio::test]
async fn openai_driver_executes_non_streaming_operations() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(MockTransport {
        response: Some(TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "id": "chatcmpl-1",
                "model": "gpt-4o-mini",
                "choices": [{"message": {"content": "hello back"}}],
                "usage": {
                    "prompt_tokens": 5,
                    "completion_tokens": 7,
                    "total_tokens": 12
                }
            }))
            .expect("response body"),
        }),
        stream_chunks: None,
        seen: seen.clone(),
    });
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "alias".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: false,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("chat result");

    match session {
        ProxySession::Completed(response) => {
            assert_eq!(response.response.output_text.as_deref(), Some("hello back"));
            assert_eq!(response.report.selected_endpoint_id, "ep-1");
            assert_eq!(response.report.pool_id.as_deref(), Some("alpha"));
            assert_eq!(
                response
                    .report
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.total_tokens),
                Some(12)
            );
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }

    assert_eq!(seen.lock().expect("seen lock").len(), 1);

    let embeddings_transport = Arc::new(MockTransport {
        response: Some(TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "data": [{"embedding": [0.1, 0.2], "index": 0}],
                "usage": {"prompt_tokens": 3, "total_tokens": 3}
            }))
            .expect("embeddings body"),
        }),
        stream_chunks: None,
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let embeddings_driver = OpenAiCompatibleDriver::new(embeddings_transport);
    let embeddings = embeddings_driver
        .execute_embeddings(
            endpoint(),
            ProxyEmbeddingsRequest {
                model: "text-embedding-3-small".to_string(),
                input: vec!["hello".to_string()],
                encoding_format: Some("float".to_string()),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("embeddings result");
    assert!(embeddings.response.raw.get("data").is_some());

    let responses_transport = Arc::new(MockTransport {
        response: Some(TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "output": [
                    {"content": [{"type": "output_text", "text": "response text"}]}
                ]
            }))
            .expect("responses body"),
        }),
        stream_chunks: None,
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let responses_driver = OpenAiCompatibleDriver::new(responses_transport);
    let responses = responses_driver
        .execute_responses(
            endpoint(),
            ProxyResponsesRequest {
                model: "gpt-4.1-mini".to_string(),
                input: Some(json!([{"role": "user", "content": "hello"}])),
                instructions: None,
                temperature: None,
                top_p: None,
                max_output_tokens: None,
                stream: false,
                tools: None,
                tool_choice: None,
                previous_response_id: None,
                request_metadata: None,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("responses result");

    match responses {
        ProxySession::Completed(response) => {
            assert_eq!(
                response.response.output_text.as_deref(),
                Some("response text")
            );
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn openai_driver_executes_streaming_chat() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_vec(),
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n".to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "alias".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: true,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("chat stream session");

    match session {
        ProxySession::Streaming(streaming) => {
            let chunks = streaming
                .stream
                .map(|item| item.expect("chunk"))
                .collect::<Vec<_>>()
                .await;
            assert_eq!(chunks.len(), 2);
            assert_eq!(chunks[0].delta.as_deref(), Some("hel"));
            assert_eq!(chunks[1].delta.as_deref(), Some("lo"));

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
                Some(7)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}

#[tokio::test]
async fn openai_driver_streaming_chat_completion_survives_dropped_stream() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_vec(),
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n".to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_chat(
            endpoint(),
            ProxyChatRequest {
                model: "alias".to_string(),
                messages: vec![Message::text(MessageRole::User, "hello")],
                system: None,
                tools: None,
                tool_choice: None,
                raw_messages: None,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: None,
                stop_sequences: None,
                stream: true,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("chat stream session");

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
                Some(7)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}

#[tokio::test]
async fn openai_driver_executes_streaming_responses() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"event: response.created\ndata: {\"response\":{\"id\":\"resp_1\"}}\n\n".to_vec(),
            b"event: response.output_text.delta\ndata: {\"delta\":\"hello\"}\n\n".to_vec(),
            b"event: response.completed\ndata: {\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":4,\"total_tokens\":7}}}\n\n".to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_responses(
            endpoint(),
            ProxyResponsesRequest {
                model: "gpt-4.1-mini".to_string(),
                input: Some(json!([{"role": "user", "content": "hello"}])),
                instructions: None,
                temperature: None,
                top_p: None,
                max_output_tokens: None,
                stream: true,
                tools: None,
                tool_choice: None,
                previous_response_id: None,
                request_metadata: None,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("responses stream session");

    match session {
        ProxySession::Streaming(streaming) => {
            let events = streaming
                .stream
                .map(|item| item.expect("event"))
                .collect::<Vec<_>>()
                .await;
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].event_type, "response.created");
            assert_eq!(events[1].event_type, "response.output_text.delta");
            assert_eq!(
                events[1].data.get("type").and_then(Value::as_str),
                Some("response.output_text.delta")
            );

            let completion = streaming
                .completion
                .await
                .expect("completion receiver")
                .expect("completion result");
            assert_eq!(completion.response.output_text.as_deref(), Some("hello"));
            assert_eq!(
                completion
                    .report
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.total_tokens),
                Some(7)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}

#[tokio::test]
async fn openai_driver_streaming_responses_completion_survives_dropped_stream() {
    let transport = Arc::new(MockTransport {
        response: None,
        stream_chunks: Some(vec![
            b"event: response.created\ndata: {\"response\":{\"id\":\"resp_1\"}}\n\n".to_vec(),
            b"event: response.output_text.delta\ndata: {\"delta\":\"hello\"}\n\n".to_vec(),
            b"event: response.completed\ndata: {\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":4,\"total_tokens\":7}}}\n\n".to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_responses(
            endpoint(),
            ProxyResponsesRequest {
                model: "gpt-4.1-mini".to_string(),
                input: Some(json!([{"role": "user", "content": "hello"}])),
                instructions: None,
                temperature: None,
                top_p: None,
                max_output_tokens: None,
                stream: true,
                tools: None,
                tool_choice: None,
                previous_response_id: None,
                request_metadata: None,
                extra: HashMap::new(),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("responses stream session");

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
                Some(7)
            );
        }
        ProxySession::Completed(_) => panic!("expected streaming response"),
    }
}
