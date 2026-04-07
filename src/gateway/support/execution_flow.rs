use std::sync::Arc;

use axum::response::Response;
use serde_json::Value;
use tracing::info;
use unigateway_core::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use unigateway_runtime::{
    core::{
        embeddings_payload_is_core_compatible, responses_payload_is_core_compatible,
        try_anthropic_chat_via_core, try_anthropic_chat_via_env_core, try_openai_chat_via_core,
        try_openai_chat_via_env_core, try_openai_embeddings_via_core,
        try_openai_embeddings_via_env_core, try_openai_responses_via_core,
        try_openai_responses_via_env_core,
    },
    flow::{
        RuntimeResponseResult, missing_upstream_api_key_response, prepare_anthropic_env_config,
        prepare_openai_env_config, resolve_authenticated_runtime_flow, resolve_env_runtime_flow,
    },
};

use crate::gateway::legacy_runtime;
use crate::types::AppState;

use super::request_flow::PreparedGatewayRequest;
use super::response_flow::respond_prepared_runtime_result;

pub(super) async fn execute_prepared_openai_chat(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyChatRequest,
) -> Response {
    let endpoint = "/v1/chat/completions";

    let result = if let Some(auth) = prepared.auth.as_ref() {
        resolve_authenticated_runtime_flow(
            try_openai_chat_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request.clone(),
            ),
            legacy_runtime::invoke_openai_chat_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
        )
        .await
    } else {
        execute_openai_chat_env(prepared, request).await
    };

    respond_prepared_runtime_result(state, prepared, endpoint, result).await
}

pub(super) async fn execute_prepared_openai_responses(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: ProxyResponsesRequest,
    payload: &Value,
) -> Response {
    let endpoint = "/v1/responses";

    let result = if let Some(auth) = prepared.auth.as_ref() {
        if responses_payload_is_core_compatible(payload) {
            resolve_authenticated_runtime_flow(
                try_openai_responses_via_core(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    request.clone(),
                ),
                legacy_runtime::invoke_openai_responses_via_legacy(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    &request,
                ),
            )
            .await
        } else {
            resolve_authenticated_runtime_flow(
                std::future::ready(Ok(None::<Response>)),
                legacy_runtime::invoke_openai_responses_via_legacy(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    &request,
                ),
            )
            .await
        }
    } else {
        execute_openai_responses_env(prepared, &request, payload).await
    };

    respond_prepared_runtime_result(state, prepared, endpoint, result).await
}

pub(super) async fn execute_prepared_anthropic_chat(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyChatRequest,
) -> Response {
    let endpoint = "/v1/messages";

    info!(
        endpoint,
        gateway_key_matched = prepared.auth.is_some(),
        token_present = !prepared.token.is_empty(),
        "anthropic request authentication result"
    );

    if prepared.auth.is_none() {
        info!(
            endpoint,
            token_present = !prepared.token.is_empty(),
            env_key_present = !prepared.runtime.config.anthropic_api_key.is_empty(),
            using_env_fallback =
                prepared.token.is_empty() && !prepared.runtime.config.anthropic_api_key.is_empty(),
            "anthropic request falling back to env upstream key"
        );
    }

    let result = if let Some(auth) = prepared.auth.as_ref() {
        resolve_authenticated_runtime_flow(
            try_anthropic_chat_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request.clone(),
                &request.model,
            ),
            legacy_runtime::invoke_anthropic_chat_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
        )
        .await
    } else {
        execute_anthropic_chat_env(prepared, request).await
    };

    respond_prepared_runtime_result(state, prepared, endpoint, result).await
}

pub(super) async fn execute_prepared_openai_embeddings(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyEmbeddingsRequest,
    payload: &Value,
) -> Response {
    let endpoint = "/v1/embeddings";

    let result = if let Some(auth) = prepared.auth.as_ref() {
        if embeddings_payload_is_core_compatible(payload) {
            resolve_authenticated_runtime_flow(
                try_openai_embeddings_via_core(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    request.clone(),
                ),
                legacy_runtime::invoke_openai_embeddings_via_legacy(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    request,
                ),
            )
            .await
        } else {
            resolve_authenticated_runtime_flow(
                std::future::ready(Ok(None::<Response>)),
                legacy_runtime::invoke_openai_embeddings_via_legacy(
                    &prepared.runtime,
                    &auth.key.service_id,
                    prepared.hint.as_deref(),
                    request,
                ),
            )
            .await
        }
    } else {
        execute_openai_embeddings_env(prepared, request).await
    };

    respond_prepared_runtime_result(state, prepared, endpoint, result).await
}

async fn execute_openai_chat_env(
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyChatRequest,
) -> RuntimeResponseResult {
    let env = match prepare_openai_env_config(&prepared.token, prepared.runtime.config) {
        Some(env) => env,
        None => return Err(missing_upstream_api_key_response()),
    };

    resolve_env_runtime_flow(
        try_openai_chat_via_env_core(
            &prepared.runtime,
            prepared.hint.as_deref(),
            request.clone(),
            env.base_url,
            &env.api_key,
        ),
        legacy_runtime::invoke_openai_chat_via_env_legacy(env.base_url, &env.api_key, request),
    )
    .await
}

async fn execute_openai_responses_env(
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyResponsesRequest,
    payload: &Value,
) -> RuntimeResponseResult {
    let env = match prepare_openai_env_config(&prepared.token, prepared.runtime.config) {
        Some(env) => env,
        None => return Err(missing_upstream_api_key_response()),
    };

    if responses_payload_is_core_compatible(payload) {
        resolve_env_runtime_flow(
            try_openai_responses_via_env_core(
                &prepared.runtime,
                prepared.hint.as_deref(),
                request.clone(),
                env.base_url,
                &env.api_key,
            ),
            legacy_runtime::invoke_openai_responses_via_env_legacy(
                env.base_url,
                &env.api_key,
                request,
            ),
        )
        .await
    } else {
        resolve_env_runtime_flow(
            std::future::ready(Ok(None::<Response>)),
            legacy_runtime::invoke_openai_responses_via_env_legacy(
                env.base_url,
                &env.api_key,
                request,
            ),
        )
        .await
    }
}

async fn execute_anthropic_chat_env(
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyChatRequest,
) -> RuntimeResponseResult {
    let env = match prepare_anthropic_env_config(&prepared.token, prepared.runtime.config) {
        Some(env) => env,
        None => return Err(missing_upstream_api_key_response()),
    };

    resolve_env_runtime_flow(
        try_anthropic_chat_via_env_core(
            &prepared.runtime,
            prepared.hint.as_deref(),
            request.clone(),
            &request.model,
            env.base_url,
            &env.api_key,
        ),
        legacy_runtime::invoke_anthropic_chat_via_env_legacy(env.base_url, &env.api_key, request),
    )
    .await
}

async fn execute_openai_embeddings_env(
    prepared: &PreparedGatewayRequest<'_>,
    request: &ProxyEmbeddingsRequest,
) -> RuntimeResponseResult {
    let env = match prepare_openai_env_config(&prepared.token, prepared.runtime.config) {
        Some(env) => env,
        None => return Err(missing_upstream_api_key_response()),
    };

    resolve_env_runtime_flow(
        try_openai_embeddings_via_env_core(
            &prepared.runtime,
            prepared.hint.as_deref(),
            request.clone(),
            env.base_url,
            &env.api_key,
        ),
        legacy_runtime::invoke_openai_embeddings_via_env_legacy(
            env.base_url,
            &env.api_key,
            request,
        ),
    )
    .await
}
