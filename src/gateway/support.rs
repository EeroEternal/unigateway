use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use llm_connector::types::{ChatRequest, EmbedRequest, ResponsesRequest};
use serde_json::Value;
use tracing::info;

use crate::middleware::{
    GatewayAuth, error_json, extract_openai_api_key, extract_x_api_key, record_stat,
};
use crate::protocol::{
    anthropic_payload_to_chat_request, openai_payload_to_chat_request,
    openai_payload_to_embed_request, openai_payload_to_responses_request,
};
use crate::routing::target_provider_hint;
use crate::runtime::{
    RuntimeContext, invoke_anthropic_chat_via_env_legacy, invoke_anthropic_chat_via_legacy,
    invoke_openai_chat_via_env_legacy, invoke_openai_chat_via_legacy,
    invoke_openai_embeddings_via_env_legacy, invoke_openai_embeddings_via_legacy,
    invoke_openai_responses_via_env_legacy, invoke_openai_responses_via_legacy,
    status_for_core_error, status_for_legacy_error, try_anthropic_chat_via_core,
    try_openai_chat_via_core, try_openai_chat_via_env_core, try_openai_embeddings_via_core,
    try_openai_responses_via_core, try_openai_responses_via_env_core,
};
use crate::types::AppState;

pub(super) async fn handle_openai_chat_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let prepared = match prepare_openai_request(state, headers, payload).await {
        Ok(prepared) => prepared,
        Err(resp) => return resp,
    };

    let request =
        match openai_payload_to_chat_request(payload, prepared.runtime.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    execute_prepared_openai_chat(state, &prepared, &request).await
}

pub(super) async fn handle_openai_responses_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let prepared = match prepare_openai_request(state, headers, payload).await {
        Ok(prepared) => prepared,
        Err(resp) => return resp,
    };

    let request =
        match openai_payload_to_responses_request(payload, prepared.runtime.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    execute_prepared_openai_responses(state, &prepared, request, payload).await
}

pub(super) async fn handle_anthropic_messages_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let prepared = match prepare_anthropic_request(state, headers, payload).await {
        Ok(prepared) => prepared,
        Err(resp) => return resp,
    };

    let request =
        match anthropic_payload_to_chat_request(payload, prepared.runtime.config.anthropic_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    execute_prepared_anthropic_chat(state, &prepared, &request).await
}

pub(super) async fn handle_openai_embeddings_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let prepared = match prepare_openai_request(state, headers, payload).await {
        Ok(prepared) => prepared,
        Err(resp) => return resp,
    };

    let request =
        match openai_payload_to_embed_request(payload, prepared.runtime.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    execute_prepared_openai_embeddings(state, &prepared, &request, payload).await
}

pub(super) struct PreparedGatewayRequest<'a> {
    pub start: Instant,
    pub runtime: RuntimeContext<'a>,
    pub token: String,
    pub hint: Option<String>,
    pub auth: Option<GatewayAuth>,
}

async fn prepare_openai_request<'a>(
    state: &'a Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Result<PreparedGatewayRequest<'a>, Response> {
    prepare_gateway_request(state, headers, payload, |runtime| {
        extract_openai_api_key(headers, runtime.config.openai_api_key)
    })
    .await
}

async fn prepare_anthropic_request<'a>(
    state: &'a Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Result<PreparedGatewayRequest<'a>, Response> {
    let prepared = prepare_gateway_request(state, headers, payload, |runtime| {
        extract_x_api_key(headers, runtime.config.anthropic_api_key)
    })
    .await?;

    log_prepared_anthropic_request(headers, payload, &prepared);

    Ok(prepared)
}

fn log_prepared_anthropic_request(
    headers: &HeaderMap,
    payload: &Value,
    prepared: &PreparedGatewayRequest<'_>,
) {
    let has_x_api_key = headers.get("x-api-key").is_some();
    let has_bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.starts_with("Bearer "))
        .unwrap_or(false);

    info!(
        endpoint = "/v1/messages",
        has_x_api_key,
        has_bearer,
        token_present = !prepared.token.is_empty(),
        model = payload
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        "received anthropic request"
    );
}

async fn prepare_gateway_request<'a, ExtractToken>(
    state: &'a Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
    extract_token: ExtractToken,
) -> Result<PreparedGatewayRequest<'a>, Response>
where
    ExtractToken: FnOnce(&RuntimeContext<'a>) -> String,
{
    let start = Instant::now();
    let runtime = RuntimeContext::new(state.as_ref());
    let token = extract_token(&runtime);
    let hint = target_provider_hint(headers, payload);
    let auth = GatewayAuth::try_authenticate(state, &token).await?;

    Ok(PreparedGatewayRequest {
        start,
        runtime,
        token,
        hint,
        auth,
    })
}

async fn execute_prepared_openai_chat(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &ChatRequest,
) -> Response {
    let endpoint = "/v1/chat/completions";

    if let Some(ref auth) = prepared.auth {
        return handle_authenticated_runtime_flow(
            state,
            auth,
            endpoint,
            &prepared.start,
            try_openai_chat_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
            invoke_openai_chat_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
        )
        .await;
    }

    let api_key = fallback_api_key(&prepared.token, prepared.runtime.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    let base_url = prepared.runtime.config.openai_base_url;
    handle_env_runtime_flow(
        state,
        endpoint,
        &prepared.start,
        try_openai_chat_via_env_core(
            &prepared.runtime,
            prepared.hint.as_deref(),
            request,
            base_url,
            &api_key,
        ),
        invoke_openai_chat_via_env_legacy(base_url, &api_key, request),
    )
    .await
}

async fn execute_prepared_openai_responses(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    mut request: ResponsesRequest,
    payload: &Value,
) -> Response {
    let endpoint = "/v1/responses";
    let original_model = request.model.clone();

    if let Some(ref auth) = prepared.auth {
        return handle_authenticated_runtime_flow(
            state,
            auth,
            endpoint,
            &prepared.start,
            try_openai_responses_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                &request,
                payload,
            ),
            invoke_openai_responses_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                &request,
            ),
        )
        .await;
    }

    let api_key = fallback_api_key(&prepared.token, prepared.runtime.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    request.model = original_model;
    let request = request.clone();

    handle_env_runtime_flow(
        state,
        endpoint,
        &prepared.start,
        try_openai_responses_via_env_core(
            &prepared.runtime,
            prepared.hint.as_deref(),
            &request,
            payload,
            prepared.runtime.config.openai_base_url,
            &api_key,
        ),
        invoke_openai_responses_via_env_legacy(
            prepared.runtime.config.openai_base_url,
            &api_key,
            &request,
        ),
    )
    .await
}

async fn execute_prepared_anthropic_chat(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &ChatRequest,
) -> Response {
    let endpoint = "/v1/messages";

    info!(
        endpoint,
        gateway_key_matched = prepared.auth.is_some(),
        token_present = !prepared.token.is_empty(),
        "anthropic request authentication result"
    );

    if let Some(ref auth) = prepared.auth {
        return handle_authenticated_runtime_flow(
            state,
            auth,
            endpoint,
            &prepared.start,
            try_anthropic_chat_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
            invoke_anthropic_chat_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
        )
        .await;
    }

    info!(
        endpoint,
        token_present = !prepared.token.is_empty(),
        env_key_present = !prepared.runtime.config.anthropic_api_key.is_empty(),
        using_env_fallback =
            prepared.token.is_empty() && !prepared.runtime.config.anthropic_api_key.is_empty(),
        "anthropic request falling back to env upstream key"
    );

    let api_key = fallback_api_key(&prepared.token, prepared.runtime.config.anthropic_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    handle_env_runtime_flow(
        state,
        endpoint,
        &prepared.start,
        std::future::ready(Ok(None::<Response>)),
        invoke_anthropic_chat_via_env_legacy(
            prepared.runtime.config.anthropic_base_url,
            &api_key,
            request,
        ),
    )
    .await
}

async fn execute_prepared_openai_embeddings(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: &EmbedRequest,
    payload: &Value,
) -> Response {
    let endpoint = "/v1/embeddings";

    if let Some(ref auth) = prepared.auth {
        return handle_authenticated_runtime_flow(
            state,
            auth,
            endpoint,
            &prepared.start,
            try_openai_embeddings_via_core(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
                payload,
            ),
            invoke_openai_embeddings_via_legacy(
                &prepared.runtime,
                &auth.key.service_id,
                prepared.hint.as_deref(),
                request,
            ),
        )
        .await;
    }

    let api_key = fallback_api_key(&prepared.token, prepared.runtime.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    handle_env_runtime_flow(
        state,
        endpoint,
        &prepared.start,
        std::future::ready(Ok(None::<Response>)),
        invoke_openai_embeddings_via_env_legacy(
            prepared.runtime.config.openai_base_url,
            &api_key,
            request,
        ),
    )
    .await
}

async fn handle_authenticated_runtime_flow<CoreFuture, LegacyFuture>(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    core_attempt: CoreFuture,
    legacy_attempt: LegacyFuture,
) -> Response
where
    CoreFuture: Future<Output = anyhow::Result<Option<Response>>>,
    LegacyFuture: Future<Output = anyhow::Result<Response>>,
{
    match core_attempt.await {
        Ok(Some(response)) => {
            return gateway_success_response(state, auth, endpoint, start, response).await;
        }
        Ok(None) => {}
        Err(error) => {
            return gateway_error_response(
                state,
                auth,
                endpoint,
                start,
                core_error_response(&error),
            )
            .await;
        }
    }

    match legacy_attempt.await {
        Ok(response) => gateway_success_response(state, auth, endpoint, start, response).await,
        Err(error) => {
            gateway_error_response(state, auth, endpoint, start, legacy_error_response(&error))
                .await
        }
    }
}

async fn handle_env_runtime_flow<CoreFuture, LegacyFuture>(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    core_attempt: CoreFuture,
    legacy_attempt: LegacyFuture,
) -> Response
where
    CoreFuture: Future<Output = anyhow::Result<Option<Response>>>,
    LegacyFuture: Future<Output = anyhow::Result<Response>>,
{
    match core_attempt.await {
        Ok(Some(response)) => {
            return success_response(state, endpoint, start, response).await;
        }
        Ok(None) => {}
        Err(error) => {
            return error_response(state, endpoint, start, core_error_response(&error)).await;
        }
    }

    match legacy_attempt.await {
        Ok(response) => success_response(state, endpoint, start, response).await,
        Err(error) => error_response(state, endpoint, start, upstream_error_response(&error)).await,
    }
}

fn fallback_api_key(token: &str, env_key: &str) -> String {
    if !token.is_empty() {
        token.to_string()
    } else {
        env_key.to_string()
    }
}

fn core_error_response(error: &anyhow::Error) -> Response {
    error_json(
        status_for_core_error(error),
        &format!("core execution error: {error:#}"),
    )
}

fn legacy_error_response(error: &anyhow::Error) -> Response {
    error_json(
        status_for_legacy_error(error),
        &format!("legacy execution error: {error:#}"),
    )
}

fn upstream_error_response(error: &anyhow::Error) -> Response {
    error_json(
        StatusCode::BAD_GATEWAY,
        &format!("upstream error: {error:#}"),
    )
}

async fn gateway_success_response(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    auth.finalize(state).await;
    success_response(state, endpoint, start, response).await
}

async fn gateway_error_response(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    auth.release(state).await;
    error_response(state, endpoint, start, response).await
}

async fn success_response(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    record_stat(state, endpoint, 200, start).await;
    response
}

async fn error_response(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    record_stat(state, endpoint, 500, start).await;
    response
}

#[cfg(test)]
mod tests {
    use super::fallback_api_key;

    #[test]
    fn fallback_api_key_prefers_request_token() {
        assert_eq!(fallback_api_key("sk-live", "sk-env"), "sk-live");
        assert_eq!(fallback_api_key("", "sk-env"), "sk-env");
    }
}
