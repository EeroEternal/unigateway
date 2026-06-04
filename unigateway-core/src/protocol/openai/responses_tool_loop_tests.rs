//! Regression tests for OpenAI Responses API tool loops (non-stream + stream).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::OpenAiCompatibleDriver;
use super::build_responses_request;
use crate::capabilities::EndpointCapabilities;
use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::pool::{ModelPolicy, ProviderKind, SecretString};
use crate::request::ProxyResponsesRequest;
use crate::response::ProxySession;
use crate::transport::{
    HttpTransport, StreamingTransportResponse, TransportRequest, TransportResponse,
};

struct SequenceMockTransport {
    responses: Mutex<Vec<TransportResponse>>,
    stream_chunks: Mutex<Vec<Vec<Vec<u8>>>>,
    seen: Arc<Mutex<Vec<TransportRequest>>>,
    call_index: Mutex<usize>,
}

impl SequenceMockTransport {
    fn new(responses: Vec<TransportResponse>, stream_chunks: Vec<Vec<Vec<u8>>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            stream_chunks: Mutex::new(stream_chunks),
            seen: Arc::new(Mutex::new(Vec::new())),
            call_index: Mutex::new(0),
        }
    }

    fn seen_requests(&self) -> Vec<TransportRequest> {
        self.seen.lock().expect("seen lock").clone()
    }
}

impl HttpTransport for SequenceMockTransport {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, crate::GatewayError>> {
        let seen = self.seen.clone();
        let responses = self.responses.lock().expect("responses lock").clone();
        let mut call_index = self.call_index.lock().expect("call index lock");
        let index = *call_index;
        *call_index += 1;
        let response = responses
            .get(index)
            .or_else(|| responses.last())
            .expect("at least one mock response")
            .clone();

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
        let chunks_list = self.stream_chunks.lock().expect("stream lock").clone();
        let mut call_index = self.call_index.lock().expect("call index lock");
        let index = *call_index;
        *call_index += 1;
        let chunks = chunks_list
            .get(index)
            .or_else(|| chunks_list.last())
            .expect("at least one mock stream")
            .clone();

        Box::pin(async move {
            seen.lock().expect("seen lock").push(request);
            Ok(StreamingTransportResponse {
                status: 200,
                headers: HashMap::new(),
                stream: Box::pin(futures_util::stream::iter(
                    chunks.into_iter().map(Ok::<Vec<u8>, crate::GatewayError>),
                )),
            })
        })
    }
}

fn endpoint() -> DriverEndpointContext {
    DriverEndpointContext {
        endpoint_id: "ep-openai".to_string(),
        provider_kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.example.com/v1/".to_string(),
        api_key: SecretString::new("sk-test"),
        model_policy: ModelPolicy {
            default_model: Some("gpt-5.5".to_string()),
            model_mapping: HashMap::new(),
        },
        capabilities: EndpointCapabilities::default(),
        metadata: HashMap::new(),
    }
}

fn weather_tool_decl() -> Value {
    json!([{
        "type": "function",
        "name": "get_weather",
        "description": "Get weather for a city",
        "parameters": {
            "type": "object",
            "properties": {
                "city": {"type": "string"}
            },
            "required": ["city"]
        }
    }])
}

fn gpt55_responses_request(
    input: Value,
    previous_response_id: Option<&str>,
    stream: bool,
) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        model: "gpt-5.5".to_string(),
        input: Some(input),
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stream,
        tools: Some(weather_tool_decl()),
        tool_choice: Some(json!("auto")),
        reasoning: None,
        previous_response_id: previous_response_id.map(str::to_string),
        request_metadata: None,
        extra: HashMap::from([("reasoning_effort".to_string(), json!("low"))]),
        metadata: HashMap::new(),
    }
}

fn transport_body(request: &TransportRequest) -> Value {
    serde_json::from_slice(request.body.as_ref().expect("json body")).expect("parse body")
}

fn round1_tool_call_response() -> TransportResponse {
    TransportResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_vec(&json!({
            "id": "resp_round1",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_weather_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"San Francisco\"}"
            }],
            "usage": {
                "input_tokens": 40,
                "output_tokens": 12,
                "total_tokens": 52,
                "output_tokens_details": { "reasoning_tokens": 4 }
            }
        }))
        .expect("serialize"),
    }
}

fn round2_text_response() -> TransportResponse {
    TransportResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_vec(&json!({
            "id": "resp_round2",
            "object": "response",
            "status": "completed",
            "output_text": "It is 72F and sunny in San Francisco.",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "It is 72F and sunny in San Francisco."
                }]
            }],
            "usage": {
                "input_tokens": 60,
                "output_tokens": 18,
                "total_tokens": 78,
                "output_tokens_details": { "reasoning_tokens": 2 }
            }
        }))
        .expect("serialize"),
    }
}

#[tokio::test]
async fn non_stream_single_round_returns_function_call_in_raw() {
    let transport = Arc::new(SequenceMockTransport::new(
        vec![round1_tool_call_response()],
        vec![],
    ));
    let driver = OpenAiCompatibleDriver::new(transport.clone());

    let session = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(
                json!([{
                    "role": "user",
                    "content": [{"type": "input_text", "text": "What's the weather in SF?"}]
                }]),
                None,
                false,
            ),
        )
        .await
        .expect("responses session");

    let ProxySession::Completed(completed) = session else {
        panic!("expected completed session");
    };

    assert_eq!(
        completed.report.kind,
        crate::response::RequestKind::Responses
    );
    assert_eq!(
        completed
            .report
            .usage
            .as_ref()
            .and_then(|u| u.reasoning_tokens),
        Some(4)
    );

    let output = completed
        .response
        .raw
        .get("output")
        .and_then(Value::as_array)
        .expect("output array");
    assert_eq!(
        output[0].get("type").and_then(Value::as_str),
        Some("function_call")
    );
    assert_eq!(
        output[0].get("name").and_then(Value::as_str),
        Some("get_weather")
    );

    let body = transport_body(&transport.seen_requests()[0]);
    assert!(body.get("tools").and_then(Value::as_array).is_some());
    assert_eq!(
        body.get("reasoning")
            .and_then(|v| v.get("effort"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert!(body.get("reasoning_effort").is_none());
}

#[tokio::test]
async fn non_stream_two_round_tool_loop_preserves_function_call_output() {
    let transport = Arc::new(SequenceMockTransport::new(
        vec![round1_tool_call_response(), round2_text_response()],
        vec![],
    ));
    let driver = OpenAiCompatibleDriver::new(transport.clone() as Arc<dyn HttpTransport>);

    let round1 = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(
                json!([{
                    "role": "user",
                    "content": [{"type": "input_text", "text": "What's the weather in SF?"}]
                }]),
                None,
                false,
            ),
        )
        .await
        .expect("round1");

    let ProxySession::Completed(round1) = round1 else {
        panic!("expected completed round1");
    };
    let response_id = round1
        .response
        .raw
        .get("id")
        .and_then(Value::as_str)
        .expect("response id");

    let round2 = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(
                json!([
                    {
                        "type": "function_call_output",
                        "call_id": "call_weather_1",
                        "output": "72F and sunny"
                    }
                ]),
                Some(response_id),
                false,
            ),
        )
        .await
        .expect("round2");

    let ProxySession::Completed(round2) = round2 else {
        panic!("expected completed round2");
    };
    assert_eq!(
        round2.response.output_text.as_deref(),
        Some("It is 72F and sunny in San Francisco.")
    );

    let seen = transport.seen_requests();
    assert_eq!(seen.len(), 2);

    let round2_body = transport_body(&seen[1]);
    let input = round2_body
        .get("input")
        .and_then(Value::as_array)
        .expect("round2 input array");
    assert_eq!(
        input[0].get("type").and_then(Value::as_str),
        Some("function_call_output")
    );
    assert_eq!(
        input[0].get("call_id").and_then(Value::as_str),
        Some("call_weather_1")
    );
    assert_eq!(
        round2_body
            .get("previous_response_id")
            .and_then(Value::as_str),
        Some("resp_round1")
    );
    assert!(round2_body.get("tools").is_some());
    assert_eq!(
        round2_body
            .get("reasoning")
            .and_then(|v| v.get("effort"))
            .and_then(Value::as_str),
        Some("low")
    );
}

#[test]
fn build_responses_request_keeps_structured_input_for_tool_continuation() {
    let body = transport_body(
        &build_responses_request(
            &mut endpoint(),
            &gpt55_responses_request(
                json!([
                    {"type": "function_call_output", "call_id": "call_1", "output": "ok"},
                    {"type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{}"}
                ]),
                Some("resp_prev"),
                false,
            ),
        )
        .expect("request"),
    );

    let input = body.get("input").and_then(Value::as_array).expect("input");
    assert_eq!(input.len(), 2);
    assert_eq!(
        input[0].get("type").and_then(Value::as_str),
        Some("function_call_output")
    );
    assert_eq!(
        input[1].get("type").and_then(Value::as_str),
        Some("function_call")
    );
}

#[tokio::test]
async fn streaming_single_round_emits_function_call_events() {
    let stream_chunks = vec![
        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream_1\"}}\n\n"
            .to_vec(),
        b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_stream_1\",\"name\":\"get_weather\",\"arguments\":\"\"}}\n\n"
            .to_vec(),
        b"event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"city\\\":\\\"SF\\\"}\"}\n\n"
            .to_vec(),
        b"event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"arguments\":\"{\\\"city\\\":\\\"SF\\\"}\"}\n\n"
            .to_vec(),
        b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":6,\"total_tokens\":16,\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n"
            .to_vec(),
        b"data: [DONE]\n\n".to_vec(),
    ];

    let transport = Arc::new(SequenceMockTransport::new(vec![], vec![stream_chunks]));
    let driver = OpenAiCompatibleDriver::new(transport);

    let session = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(json!([{"role": "user", "content": "weather?"}]), None, true),
        )
        .await
        .expect("stream session");

    let ProxySession::Streaming(streaming) = session else {
        panic!("expected streaming session");
    };

    let events: Vec<_> = streaming
        .stream
        .map(|item| item.expect("event"))
        .collect()
        .await;

    let event_types: Vec<_> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert!(event_types.contains(&"response.output_item.added"));
    assert!(event_types.contains(&"response.function_call_arguments.delta"));
    assert!(event_types.contains(&"response.function_call_arguments.done"));
    assert!(event_types.contains(&"response.completed"));

    let completion = streaming
        .completion
        .await
        .expect("completion channel")
        .expect("completion");
    assert_eq!(
        completion.report.kind,
        crate::response::RequestKind::Responses
    );
    assert_eq!(
        completion
            .report
            .usage
            .as_ref()
            .and_then(|u| u.reasoning_tokens),
        Some(1)
    );
}

#[tokio::test]
async fn streaming_two_round_tool_loop_allows_follow_up_request() {
    let round1_stream = vec![
        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_s1\"}}\n\n".to_vec(),
        b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_s1\",\"name\":\"get_weather\",\"arguments\":\"{}\"}}\n\n".to_vec(),
        b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_s1\",\"usage\":{\"input_tokens\":5,\"output_tokens\":3,\"total_tokens\":8}}}\n\n".to_vec(),
        b"data: [DONE]\n\n".to_vec(),
    ];
    let round2_stream = vec![
        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_s2\"}}\n\n".to_vec(),
        b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Sunny\"}\n\n".to_vec(),
        b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":8,\"output_tokens\":4,\"total_tokens\":12}}}\n\n".to_vec(),
        b"data: [DONE]\n\n".to_vec(),
    ];

    let transport = Arc::new(SequenceMockTransport::new(
        vec![],
        vec![round1_stream, round2_stream],
    ));
    let driver = OpenAiCompatibleDriver::new(transport.clone());

    let round1 = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(json!([{"role": "user", "content": "weather?"}]), None, true),
        )
        .await
        .expect("round1 stream");
    let ProxySession::Streaming(streaming) = round1 else {
        panic!("expected streaming round1");
    };
    let _ = streaming
        .into_completion()
        .await
        .expect("round1 completion");

    let round2 = driver
        .execute_responses(
            endpoint(),
            gpt55_responses_request(
                json!([{
                    "type": "function_call_output",
                    "call_id": "call_s1",
                    "output": "72F"
                }]),
                Some("resp_s1"),
                true,
            ),
        )
        .await
        .expect("round2 stream");
    let ProxySession::Streaming(streaming2) = round2 else {
        panic!("expected streaming round2");
    };
    let completion = streaming2
        .into_completion()
        .await
        .expect("round2 completion");
    assert_eq!(completion.response.output_text.as_deref(), Some("Sunny"));

    let round2_body = transport_body(&transport.seen_requests()[1]);
    assert_eq!(
        round2_body
            .get("input")
            .and_then(|v| v.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str),
        Some("function_call_output")
    );
}
