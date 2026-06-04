use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde_json::json;

use crate::InMemoryDriverRegistry;
use crate::pool::ExecutionTarget;
use crate::protocol::openai::OpenAiCompatibleDriver;
use crate::request::ProxyResponsesRequest;
use crate::response::{ProxySession, RequestKind};
use crate::transport::{HttpTransport, TransportRequest, TransportResponse};

use super::super::UniGatewayEngine;
use super::support::{HookRecorder, endpoint, pool};
use crate::retry::LoadBalancingStrategy;

struct SingleResponseTransport {
    response: TransportResponse,
    seen: Arc<Mutex<Vec<TransportRequest>>>,
}

impl HttpTransport for SingleResponseTransport {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, crate::GatewayError>> {
        let seen = self.seen.clone();
        let response = self.response.clone();
        Box::pin(async move {
            seen.lock().expect("seen lock").push(request);
            Ok(response)
        })
    }

    fn send_stream(
        &self,
        _request: TransportRequest,
    ) -> BoxFuture<'static, Result<crate::transport::StreamingTransportResponse, crate::GatewayError>>
    {
        Box::pin(async move {
            Err(crate::GatewayError::InvalidRequest(
                "stream not supported in SingleResponseTransport".to_string(),
            ))
        })
    }
}

fn gpt55_tool_request() -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        model: "gpt-5.5".to_string(),
        input: Some(json!([{"role": "user", "content": "weather?"}])),
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stream: false,
        tools: Some(json!([{
            "type": "function",
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {}}
        }])),
        tool_choice: Some(json!("auto")),
        reasoning: None,
        previous_response_id: None,
        request_metadata: None,
        extra: HashMap::from([("reasoning_effort".to_string(), json!("low"))]),
        metadata: HashMap::new(),
    }
}

#[tokio::test]
async fn proxy_responses_emits_responses_kind_hooks_and_usage() {
    let hooks = HookRecorder::default();
    let transport = Arc::new(SingleResponseTransport {
        response: TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "id": "resp_hook_1",
                "object": "response",
                "output": [{
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{}"
                }],
                "usage": {
                    "input_tokens": 11,
                    "output_tokens": 5,
                    "total_tokens": 16,
                    "output_tokens_details": { "reasoning_tokens": 2 }
                }
            }))
            .expect("serialize"),
        },
        seen: Arc::new(Mutex::new(Vec::new())),
    });

    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(OpenAiCompatibleDriver::new(transport)));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_hooks(Arc::new(hooks.clone()))
        .build()
        .expect("engine");

    engine
        .upsert_pool(pool(
            "openai",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("openai-main")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_responses(
            gpt55_tool_request(),
            ExecutionTarget::Pool {
                pool_id: "openai".to_string(),
            },
        )
        .await
        .expect("proxy responses");

    let ProxySession::Completed(completed) = session else {
        panic!("expected completed session");
    };

    assert_eq!(completed.report.kind, RequestKind::Responses);
    assert_eq!(
        completed
            .report
            .usage
            .as_ref()
            .and_then(|u| u.reasoning_tokens),
        Some(2)
    );

    let started = hooks
        .state
        .request_started
        .lock()
        .expect("request_started lock");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].kind, RequestKind::Responses);

    let finished = hooks.state.requests.lock().expect("requests lock");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].kind, RequestKind::Responses);
    assert_eq!(
        finished[0].usage.as_ref().and_then(|u| u.reasoning_tokens),
        Some(2)
    );
}
