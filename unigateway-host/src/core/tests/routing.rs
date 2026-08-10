use std::collections::HashMap;

use unigateway_core::{
    Endpoint, EndpointCapabilities, EndpointRef, ExecutionPlan, ExecutionTarget, GatewayError,
    ModelPolicy, OpenAiApiSurfaceCapabilities, ProviderKind, ProxyResponsesRequest, SecretString,
    should_retry_responses_without_tools,
};

use super::super::dispatch::{should_preserve_stream_error, without_response_tools};
use super::super::targeting::{build_openai_compatible_target, endpoint_matches_hint};
use super::support::endpoint;
use crate::env::{EnvProvider, build_env_pool};

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
    let pool = build_env_pool(
        EnvProvider::OpenAi,
        "gpt-4o-mini",
        "https://api.openai.com",
        "sk-test",
    );
    let endpoint = pool.endpoints.first().expect("endpoint");

    assert!(endpoint_matches_hint(endpoint, "env-openai"));
    assert!(endpoint_matches_hint(endpoint, "openai"));
    assert!(!endpoint_matches_hint(endpoint, "deepseek"));
}

#[test]
fn env_anthropic_pool_matches_basic_anthropic_hints() {
    let pool = build_env_pool(
        EnvProvider::Anthropic,
        "claude-3-5-sonnet",
        "https://api.anthropic.com",
        "sk-ant",
    );
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
        reasoning: None,
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
fn gpt55_responses_does_not_retry_without_tools_when_reasoning_and_tools_present() {
    let request = ProxyResponsesRequest {
        model: "gpt-5.5".to_string(),
        input: None,
        instructions: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stream: false,
        tools: Some(serde_json::json!([])),
        tool_choice: None,
        reasoning: Some(serde_json::json!({"effort": "low"})),
        previous_response_id: None,
        request_metadata: None,
        extra: HashMap::new(),
        metadata: HashMap::new(),
    };
    let error = GatewayError::UpstreamHttp {
        status: 500,
        body: Some("upstream failed".to_string()),
        endpoint_id: "ep".to_string(),
    };
    let surface = OpenAiApiSurfaceCapabilities::resolve_for_model("gpt-5.5", None);

    assert!(!should_retry_responses_without_tools(
        &request, &error, &surface
    ));
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
        provider_name: Some("anthropic-main".to_string()),
        source_endpoint_id: None,
        provider_family: Some("anthropic".to_string()),
        provider_kind: ProviderKind::Anthropic,
        driver_id: "anthropic".to_string(),
        base_url: "https://api.anthropic.com".to_string(),
        api_key: SecretString::new("sk-ant"),
        model_policy: ModelPolicy::default(),
        enabled: true,
        max_concurrency: None,
        capabilities: EndpointCapabilities::default(),
        metadata: HashMap::new(),
        forward_metadata_as_headers: None,
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
