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
//! DeepSeek can be tested without additional endpoint overrides:
//!
//! ```bash
//! DEEPSEEK_API_KEY=sk-... \
//! cargo test -p unigateway-core --test live_openai_responses_upstream -- --ignored --nocapture
//! ```
//!
//! `OPENROUTER_API_KEY` and `DEEPSEEK_API_KEY` are accepted as provider-specific aliases.
//! Optional: `OPENROUTER_HTTP_REFERER`, `OPENROUTER_X_TITLE` (forwarded as HTTP headers).
//!
//! Skips automatically when no API key env var is set.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::json;
use unigateway_core::{
    Endpoint, EndpointCapabilities, ExecutionTarget, InMemoryDriverRegistry, ModelPolicy,
    OpenAiApiSurfaceCapabilities, ProviderKind, ProviderPool, ProxyResponsesRequest, ProxySession,
    SecretString, UniGatewayEngine, normalize_proxy_responses_request,
    protocol::openai::OpenAiCompatibleDriver,
    retry::{BackoffPolicy, LoadBalancingStrategy, RetryPolicy},
    transport::ReqwestHttpTransport,
};

struct LiveSettings {
    api_key: String,
    base_url: String,
    model: String,
    provider_family: &'static str,
}

fn live_settings() -> Option<LiveSettings> {
    let (api_key, default_base_url, default_model, provider_family) = [
        (
            "OPENAI_API_KEY",
            "https://api.openai.com/v1",
            "gpt-5.5",
            "openai",
        ),
        (
            "OPENROUTER_API_KEY",
            "https://openrouter.ai/api/v1",
            "openai/gpt-5.5",
            "openrouter",
        ),
        (
            "DEEPSEEK_API_KEY",
            "https://api.deepseek.com",
            "deepseek-v4-flash",
            "deepseek",
        ),
    ]
    .into_iter()
    .find_map(|(name, base_url, model, family)| {
        std::env::var(name)
            .ok()
            .filter(|key| !key.trim().is_empty())
            .map(|key| (key, base_url, model, family))
    })?;

    Some(LiveSettings {
        api_key,
        base_url: std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| default_base_url.to_string()),
        model: std::env::var("OPENAI_LIVE_MODEL").unwrap_or_else(|_| default_model.to_string()),
        provider_family,
    })
}

fn live_endpoint(settings: &LiveSettings) -> Endpoint {
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
        provider_family: Some(settings.provider_family.to_string()),
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url: settings.base_url.clone(),
        api_key: SecretString::new(settings.api_key.clone()),
        model_policy: ModelPolicy {
            default_model: Some(settings.model.clone()),
            model_mapping: HashMap::new(),
        },
        enabled: true,
        max_concurrency: None,
        capabilities: EndpointCapabilities {
            openai_api_surface: Some(OpenAiApiSurfaceCapabilities::resolve_for_model(
                &settings.model,
                None,
            )),
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

async fn live_engine(settings: &LiveSettings) -> UniGatewayEngine {
    let transport = Arc::new(ReqwestHttpTransport::default());
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(OpenAiCompatibleDriver::new(transport)));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_default_timeout(Duration::from_secs(120))
        .build()
        .expect("engine");

    engine
        .upsert_pool(live_pool(live_endpoint(settings)))
        .await
        .expect("upsert pool");
    engine
}

#[tokio::test]
#[ignore = "live upstream: set OPENAI_API_KEY, OPENROUTER_API_KEY, or DEEPSEEK_API_KEY"]
async fn live_responses_tools_and_reasoning_acceptance() {
    let Some(settings) = live_settings() else {
        eprintln!("skipping live_openai_responses_upstream: no supported live API key env var set");
        return;
    };
    eprintln!(
        "live upstream: base_url={} model={}",
        settings.base_url, settings.model
    );

    let engine = live_engine(&settings).await;

    let session = engine
        .proxy_responses(
            live_tool_request(
                &settings.model,
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

#[tokio::test]
#[ignore = "live upstream: set OPENAI_API_KEY, OPENROUTER_API_KEY, or DEEPSEEK_API_KEY"]
async fn live_responses_streaming_acceptance() {
    let Some(settings) = live_settings() else {
        eprintln!("skipping live_openai_responses_upstream: no supported live API key env var set");
        return;
    };
    eprintln!(
        "live upstream: base_url={} model={}",
        settings.base_url, settings.model
    );

    let engine = live_engine(&settings).await;
    let request = ProxyResponsesRequest {
        model: settings.model,
        input: Some(json!("Reply with exactly: pong")),
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: Some(128),
        stream: true,
        tools: None,
        tool_choice: None,
        reasoning: Some(json!({"effort": "low"})),
        previous_response_id: None,
        request_metadata: None,
        extra: HashMap::new(),
        metadata: HashMap::new(),
    };

    let session = engine
        .proxy_responses(
            request,
            ExecutionTarget::Pool {
                pool_id: "live-openai".to_string(),
            },
        )
        .await
        .expect("live streaming responses request");
    let ProxySession::Streaming(mut streaming) = session else {
        panic!("expected streaming response");
    };

    let mut event_types = Vec::new();
    while let Some(event) = streaming.stream.next().await {
        event_types.push(event.expect("stream event").event_type);
    }
    let completed = streaming
        .completion
        .await
        .expect("stream completion channel")
        .expect("stream completion");

    assert!(
        event_types
            .iter()
            .any(|event_type| event_type == "response.completed"),
        "expected response.completed event: {event_types:?}"
    );
    assert!(
        completed.response.output_text.is_some(),
        "expected output text in completed stream"
    );
    assert!(
        completed.report.usage.as_ref().is_some_and(|usage| {
            usage.input_tokens.is_some()
                && usage.output_tokens.is_some()
                && usage.reasoning_tokens.is_some()
        }),
        "stream usage should include input, output, and reasoning tokens: {:?}",
        completed.report.usage
    );
}
