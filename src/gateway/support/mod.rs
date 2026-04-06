use std::sync::Arc;

use axum::{http::HeaderMap, response::Response};
use serde_json::Value;

use crate::types::AppState;

mod execution_flow;
mod request_flow;
mod response_flow;

use self::execution_flow::{
    execute_prepared_anthropic_chat, execute_prepared_openai_chat,
    execute_prepared_openai_embeddings, execute_prepared_openai_responses,
};
use self::request_flow::{prepare_and_parse_anthropic_request, prepare_and_parse_openai_request};

pub(super) async fn handle_openai_chat_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let (prepared, request) = match prepare_and_parse_openai_request(
        state,
        headers,
        payload,
        crate::protocol::openai_payload_to_chat_request,
    )
    .await
    {
        Ok(parts) => parts,
        Err(resp) => return resp,
    };

    execute_prepared_openai_chat(state, &prepared, &request).await
}

pub(super) async fn handle_openai_responses_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let (prepared, request) = match prepare_and_parse_openai_request(
        state,
        headers,
        payload,
        crate::protocol::openai_payload_to_responses_request,
    )
    .await
    {
        Ok(parts) => parts,
        Err(resp) => return resp,
    };

    execute_prepared_openai_responses(state, &prepared, request, payload).await
}

pub(super) async fn handle_anthropic_messages_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let (prepared, request) = match prepare_and_parse_anthropic_request(
        state,
        headers,
        payload,
        crate::protocol::anthropic_payload_to_chat_request,
    )
    .await
    {
        Ok(parts) => parts,
        Err(resp) => return resp,
    };

    execute_prepared_anthropic_chat(state, &prepared, &request).await
}

pub(super) async fn handle_openai_embeddings_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    let (prepared, request) = match prepare_and_parse_openai_request(
        state,
        headers,
        payload,
        crate::protocol::openai_payload_to_embed_request,
    )
    .await
    {
        Ok(parts) => parts,
        Err(resp) => return resp,
    };

    execute_prepared_openai_embeddings(state, &prepared, &request, payload).await
}
