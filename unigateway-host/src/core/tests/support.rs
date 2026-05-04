use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use http::StatusCode;
use serde_json::Value;
use unigateway_core::protocol::{AnthropicDriver, OpenAiCompatibleDriver};
use unigateway_core::transport::{
    HttpTransport, StreamingTransportResponse, TransportRequest, TransportResponse,
};
use unigateway_core::{
    Endpoint, GatewayError, InMemoryDriverRegistry, LoadBalancingStrategy, ModelPolicy,
    ProviderKind, ProviderPool, RetryPolicy, SecretString, UniGatewayEngine,
};
use unigateway_protocol::{ProtocolHttpResponse, ProtocolResponseBody};

use super::super::dispatch::HostDispatchOutcome;
use crate::host::{HostFuture, PoolHost, PoolLookupOutcome, PoolLookupResult};

pub(super) fn endpoint() -> Endpoint {
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

pub(super) fn pool_with_endpoint(pool_id: &str, endpoint: Endpoint) -> ProviderPool {
    ProviderPool {
        pool_id: pool_id.to_string(),
        endpoints: vec![endpoint],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: HashMap::new(),
    }
}

#[derive(Default)]
pub(super) struct NoopPoolHost;

impl PoolHost for NoopPoolHost {
    fn pool_for_service<'a>(
        &'a self,
        _service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async { Ok(PoolLookupOutcome::NotFound) })
    }
}

pub(super) struct StaticTransport {
    pub(super) response: Option<TransportResponse>,
    pub(super) stream_chunks: Option<Vec<Vec<u8>>>,
    pub(super) seen: Arc<Mutex<Vec<TransportRequest>>>,
}

impl HttpTransport for StaticTransport {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, GatewayError>> {
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
    ) -> BoxFuture<'static, Result<StreamingTransportResponse, GatewayError>> {
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

pub(super) fn test_engine(transport: Arc<StaticTransport>) -> UniGatewayEngine {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(OpenAiCompatibleDriver::new(transport.clone())));
    registry.register(Arc::new(AnthropicDriver::new(transport)));

    UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .expect("engine")
}

pub(super) fn json_body(response: ProtocolHttpResponse) -> Value {
    let (status, body) = response.into_parts();
    assert_eq!(status, StatusCode::OK);

    match body {
        ProtocolResponseBody::Json(body) => body,
        ProtocolResponseBody::ServerSentEvents(_) => panic!("expected json response"),
    }
}

pub(super) async fn sse_body(response: ProtocolHttpResponse) -> Vec<String> {
    let (status, body) = response.into_parts();
    assert_eq!(status, StatusCode::OK);

    match body {
        ProtocolResponseBody::Json(_) => panic!("expected sse response"),
        ProtocolResponseBody::ServerSentEvents(stream) => stream
            .map(|item| {
                item.map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8 sse chunk"))
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|item| item.expect("sse chunk"))
            .collect(),
    }
}

pub(super) fn dispatched_json_body(outcome: HostDispatchOutcome) -> Value {
    match outcome {
        HostDispatchOutcome::Response(response) => json_body(response),
        HostDispatchOutcome::PoolNotFound => panic!("expected resolved pool"),
    }
}

pub(super) fn seen_request_json(seen: &Arc<Mutex<Vec<TransportRequest>>>) -> Value {
    let guard = seen.lock().expect("seen lock");
    let request = guard.first().expect("transport request");
    serde_json::from_slice(request.body.as_ref().expect("request body")).expect("request json")
}
