use anyhow::{Result, anyhow};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use llm_connector::types::{ChatRequest, EmbedRequest, ResponsesRequest};
use serde_json::Value;

use crate::protocol::{
    UpstreamProtocol, chat_response_to_anthropic_json, chat_response_to_openai_json,
    embed_response_to_openai_json, invoke_embeddings,
};
use crate::routing::ResolvedProvider;

use super::chat::{invoke_direct_chat, invoke_provider_chat};
use super::context::RuntimeContext;
use super::responses_compat::{
    invoke_legacy_responses_for_env, invoke_legacy_responses_for_provider,
};

pub(crate) async fn invoke_openai_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_chat_via_legacy(
        runtime,
        service_id,
        "openai",
        hint,
        request,
        chat_response_to_openai_json,
    )
    .await
}

pub(crate) async fn invoke_openai_chat_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_direct_chat(
        UpstreamProtocol::OpenAi,
        base_url,
        api_key,
        request,
        None,
        chat_response_to_openai_json,
    )
    .await
}

pub(crate) async fn invoke_openai_responses_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ResponsesRequest,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, "openai", hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        match invoke_legacy_responses_for_provider(provider, &mapped).await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream responses error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

pub(crate) async fn invoke_openai_responses_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
) -> Result<Response> {
    invoke_legacy_responses_for_env(base_url, api_key, request).await
}

pub(crate) async fn invoke_anthropic_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_chat_via_legacy(
        runtime,
        service_id,
        "anthropic",
        hint,
        request,
        chat_response_to_anthropic_json,
    )
    .await
}

pub(crate) async fn invoke_anthropic_chat_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_direct_chat(
        UpstreamProtocol::Anthropic,
        base_url,
        api_key,
        request,
        None,
        chat_response_to_anthropic_json,
    )
    .await
}

pub(crate) async fn invoke_openai_embeddings_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &EmbedRequest,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, "openai", hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        tracing::debug!(
            provider_name = provider.name.as_str(),
            model = mapped.model.as_str(),
            "routing embeddings request to provider"
        );

        match invoke_embeddings(&provider.base_url, &provider.api_key, &mapped).await {
            Ok(resp) => {
                return Ok(
                    (StatusCode::OK, Json(embed_response_to_openai_json(&resp))).into_response()
                );
            }
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

pub(crate) async fn invoke_openai_embeddings_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &EmbedRequest,
) -> Result<Response> {
    invoke_embeddings(base_url, api_key, request)
        .await
        .map(|resp| (StatusCode::OK, Json(embed_response_to_openai_json(&resp))).into_response())
}

async fn invoke_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    protocol_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
    response_json: fn(&llm_connector::ChatResponse) -> Value,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, protocol_id, hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        let upstream_protocol = match provider.provider_type.as_str() {
            "anthropic" => UpstreamProtocol::Anthropic,
            _ => UpstreamProtocol::OpenAi,
        };

        tracing::debug!(
            provider_name = provider.name.as_str(),
            base_url = provider.base_url.as_str(),
            model = mapped.model.as_str(),
            upstream_protocol = ?upstream_protocol,
            "routing request to provider"
        );

        match invoke_provider_chat(upstream_protocol, provider, &mapped, response_json).await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

async fn resolve_legacy_providers(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    protocol: &str,
    hint: Option<&str>,
) -> Result<Vec<ResolvedProvider>> {
    runtime.resolve_providers(service_id, protocol, hint).await
}
