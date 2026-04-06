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

pub(crate) async fn invoke_provider_chat(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &llm_connector::types::ChatRequest,
    response_json: fn(&llm_connector::ChatResponse) -> Value,
) -> Result<Response, anyhow::Error> {
    if request.stream == Some(true) {
        // HACK: Infer downstream protocol from the response_json function pointer.
        // This is not ideal but avoids changing the `invoke_provider_chat` signature too much
        // or passing explicit flags from `gateway.rs`.
        // However, `gateway.rs` calls this with specific function pointers.
        // `chat_response_to_anthropic_json` is used for Anthropic downstream.
        // `chat_response_to_openai_json` is used for OpenAI downstream.

        use llm_connector::ChatResponse;
        let is_anthropic = std::ptr::fn_addr_eq(
            response_json,
            crate::protocol::chat_response_to_anthropic_json
                as for<'a> fn(&'a ChatResponse) -> serde_json::Value,
        );

        try_chat_stream(protocol, provider, request, is_anthropic).await
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

pub(crate) async fn invoke_direct_chat(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &llm_connector::types::ChatRequest,
    family_id: Option<&str>,
    response_json: fn(&llm_connector::ChatResponse) -> Value,
) -> Result<Response, anyhow::Error> {
    if request.stream == Some(true) {
        use llm_connector::ChatResponse;
        let is_anthropic = std::ptr::fn_addr_eq(
            response_json,
            crate::protocol::chat_response_to_anthropic_json
                as for<'a> fn(&'a ChatResponse) -> serde_json::Value,
        );
        try_chat_stream_raw(
            protocol,
            base_url,
            api_key,
            request,
            family_id,
            is_anthropic,
        )
        .await
    } else {
        invoke_with_connector(protocol, base_url, api_key, request, family_id)
            .await
            .map(|resp| (StatusCode::OK, Json(response_json(&resp))).into_response())
    }
}
