use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::{
    protocol::{UpstreamProtocol, invoke_with_connector},
    routing::ResolvedProvider,
};

use super::streaming::{try_chat_stream, try_chat_stream_raw};

pub(super) async fn invoke_provider_chat(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &llm_connector::types::ChatRequest,
    response_json: fn(&llm_connector::ChatResponse) -> Value,
) -> Result<Response, anyhow::Error> {
    if request.stream == Some(true) {
        try_chat_stream(protocol, provider, request).await
    } else {
        invoke_with_connector(
            protocol,
            &provider.base_url,
            &provider.api_key,
            request,
            provider.family_id.as_deref(),
        )
        .await
        .map(|resp| (StatusCode::OK, Json(response_json(&resp))).into_response())
    }
}

pub(super) async fn invoke_direct_chat(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &llm_connector::types::ChatRequest,
    family_id: Option<&str>,
    response_json: fn(&llm_connector::ChatResponse) -> Value,
) -> Result<Response, anyhow::Error> {
    if request.stream == Some(true) {
        try_chat_stream_raw(protocol, base_url, api_key, request, family_id).await
    } else {
        invoke_with_connector(protocol, base_url, api_key, request, family_id)
            .await
            .map(|resp| (StatusCode::OK, Json(response_json(&resp))).into_response())
    }
}
