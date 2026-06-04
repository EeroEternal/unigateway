//! Live acceptance against an OpenAI-compatible upstream.
//!
//! Run manually (OpenAI or OpenRouter):
//!
//! ```bash
//! OPENAI_API_KEY=sk-or-... \
//! OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
//! OPENAI_LIVE_MODEL=openai/gpt-5.5 \
//! cargo test -p unigateway-core --test live_openai_responses_upstream -- --ignored --nocapture
//! ```
//!
//! `OPENROUTER_API_KEY` is accepted as an alias for `OPENAI_API_KEY`.
//! Optional: `OPENROUTER_HTTP_REFERER`, `OPENROUTER_X_TITLE` (forwarded as HTTP headers).
//!
//! Skips automatically when no API key env var is set.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use unigateway_core::{
    Endpoint, EndpointCapabilities, ExecutionTarget, InMemoryDriverRegistry, ModelPolicy,
    OpenAiApiSurfaceCapabilities, ProviderKind, ProviderPool, ProxyResponsesRequest, ProxySession,
    SecretString, UniGatewayEngine, normalize_proxy_responses_request,
    protocol::openai::OpenAiCompatibleDriver,
    retry::{BackoffPolicy, LoadBalancingStrategy, RetryPolicy},
    transport::ReqwestHttpTransport,
};

fn live_api_key() -> Option<String> {
    ["OPENAI_API_KEY", "OPENROUTER_API_KEY"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|key| !key.trim().is_empty())
}

fn live_endpoint(api_key: String, model: &str) -> Endpoint {
    let mut metadata = HashMap::new();
    if let Ok(referer) = std::env::var("OPENROUTER_HTTP_REFERER") {
        metadata.insert("http_header.HTTP-Referer".to_string(), referer);
    }
    if let Ok(title) = std::env::var("OPENROUTER_X_TITLE") {
        metadata.insert("http_header.X-Title".to_string(), title);
    }

    Endpoint {
        endpoint_id: "openai-live".to_string(),
        provider_name: Some("openai-live".to_string()),
        source_endpoint_id: None,
        provider_family: Some("openai".to_string()),
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url: std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        api_key: SecretString::new(api_key),
        model_policy: ModelPolicy {
            default_model: Some(model.to_string()),
            model_mapping: HashMap::new(),
        },
        enabled: true,
        max_concurrency: None,
        capabilities: EndpointCapabilities {
            openai_api_surface: Some(OpenAiApiSurfaceCapabilities::resolve_for_model(model, None)),
            ..EndpointCapabilities::default()
        },
        metadata,
    }
}

fn live_pool(endpoint: Endpoint) -> ProviderPool {
    ProviderPool {
        pool_id: "live-openai".to_string(),
        endpoints: vec![endpoint],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy {
            max_attempts: 1,
            per_attempt_timeout: Some(Duration::from_secs(120)),
            retry_on: vec![],
            backoff: BackoffPolicy::None,
            stop_after_stream_started: true,
        },
        metadata: HashMap::new(),
    }
}

fn live_tool_request(model: &str, input: serde_json::Value) -> ProxyResponsesRequest {
    let mut request = ProxyResponsesRequest {
        model: model.to_string(),
        input: Some(input),
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: Some(256),
        stream: false,
        tools: Some(json!([{
            "type": "function",
            "name": "get_weather",
            "description": "Get the weather for a city",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }
        }])),
        tool_choice: Some(json!("auto")),
        reasoning: None,
        previous_response_id: None,
        request_metadata: None,
        extra: HashMap::from([("reasoning_effort".to_string(), json!("low"))]),
        metadata: HashMap::new(),
    };
    normalize_proxy_responses_request(&mut request);
    request
}

#[tokio::test]
#[ignore = "live upstream: set OPENAI_API_KEY (optional OPENAI_LIVE_MODEL, default gpt-5.5)"]
async fn live_responses_tools_and_reasoning_acceptance() {
    let Some(api_key) = live_api_key() else {
        eprintln!(
            "skipping live_openai_responses_upstream: OPENAI_API_KEY / OPENROUTER_API_KEY not set"
        );
        return;
    };

    let model = std::env::var("OPENAI_LIVE_MODEL").unwrap_or_else(|_| "openai/gpt-5.5".to_string());
    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    eprintln!("live upstream: base_url={base_url} model={model}");

    let transport = Arc::new(ReqwestHttpTransport::default());
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(OpenAiCompatibleDriver::new(transport)));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_default_timeout(Duration::from_secs(120))
        .build()
        .expect("engine");

    engine
        .upsert_pool(live_pool(live_endpoint(api_key, &model)))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_responses(
            live_tool_request(
                &model,
                json!([{
                    "role": "user",
                    "content": [{"type": "input_text", "text": "What's the weather in San Francisco?"}]
                }]),
            ),
            ExecutionTarget::Pool {
                pool_id: "live-openai".to_string(),
            },
        )
        .await
        .unwrap_or_else(|error| {
            if let unigateway_core::GatewayError::AllAttemptsFailed { last_error, .. } = &error
                && let unigateway_core::GatewayError::UpstreamHttp { status, body, .. } =
                    last_error.as_ref()
            {
                panic!(
                    "live responses upstream HTTP {status}: {}",
                    body.as_deref().unwrap_or("<empty body>")
                );
            }
            panic!("live responses failed: {error:?}");
        });

    let ProxySession::Completed(completed) = session else {
        panic!("expected non-streaming completed response");
    };

    assert_eq!(
        completed.report.kind,
        unigateway_core::RequestKind::Responses
    );
    assert!(
        completed
            .report
            .usage
            .as_ref()
            .is_some_and(|usage| { usage.input_tokens.is_some() && usage.output_tokens.is_some() }),
        "usage should be populated for billing: {:?}",
        completed.report.usage
    );

    let has_tool_or_text = completed.response.output_text.is_some()
        || completed
            .response
            .raw
            .get("output")
            .and_then(|output| output.as_array())
            .is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(
                        item.get("type").and_then(|v| v.as_str()),
                        Some("function_call") | Some("message")
                    )
                })
            });

    assert!(
        has_tool_or_text,
        "expected tool call or assistant output in response: {}",
        completed.response.raw
    );
}
