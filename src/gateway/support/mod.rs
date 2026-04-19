use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;

use axum::{http::HeaderMap, response::Response};
use serde_json::Value;
use unigateway_host::core::HostRequest;
use unigateway_protocol::{
    anthropic_payload_to_chat_request, openai_payload_to_chat_request,
    openai_payload_to_embed_request, openai_payload_to_responses_request,
};

use crate::types::GatewayRequestState;

mod execution_flow;
mod request_flow;
mod response_flow;

use self::execution_flow::{
    anthropic_chat_spec, execute_prepared_host_request, openai_chat_spec, openai_embeddings_spec,
    openai_responses_spec,
};
use self::request_flow::{
    PreparedGatewayRequest, prepare_and_parse_anthropic_request, prepare_and_parse_openai_request,
};

pub(super) async fn handle_openai_chat_request(
    state: &Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    handle_openai_host_request(
        state,
        headers,
        payload,
        openai_payload_to_chat_request,
        |prepared, request| async move {
            execute_prepared_host_request(
                state,
                &prepared,
                HostRequest::Chat(request),
                openai_chat_spec(),
            )
            .await
        },
    )
    .await
}

pub(super) async fn handle_openai_responses_request(
    state: &Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    handle_openai_host_request(
        state,
        headers,
        payload,
        openai_payload_to_responses_request,
        |prepared, request| async move {
            execute_prepared_host_request(
                state,
                &prepared,
                HostRequest::Responses(request),
                openai_responses_spec(),
            )
            .await
        },
    )
    .await
}

pub(super) async fn handle_anthropic_messages_request(
    state: &Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    handle_anthropic_host_request(
        state,
        headers,
        payload,
        anthropic_payload_to_chat_request,
        |prepared, request| async move {
            execute_prepared_host_request(
                state,
                &prepared,
                HostRequest::Chat(request),
                anthropic_chat_spec(),
            )
            .await
        },
    )
    .await
}

pub(super) async fn handle_openai_embeddings_request(
    state: &Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
) -> Response {
    handle_openai_host_request(
        state,
        headers,
        payload,
        openai_payload_to_embed_request,
        |prepared, request| async move {
            execute_prepared_host_request(
                state,
                &prepared,
                HostRequest::Embeddings(request),
                openai_embeddings_spec(),
            )
            .await
        },
    )
    .await
}

async fn handle_openai_host_request<'a, Request, Parse, ParseError, Dispatch, DispatchFuture>(
    state: &'a Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
    parse_request: Parse,
    dispatch: Dispatch,
) -> Response
where
    Parse: FnOnce(&Value, &str) -> Result<Request, ParseError>,
    ParseError: Display,
    Dispatch: FnOnce(PreparedGatewayRequest<'a>, Request) -> DispatchFuture,
    DispatchFuture: Future<Output = Response>,
{
    let (prepared, request) =
        match prepare_and_parse_openai_request(state, headers, payload, parse_request).await {
            Ok(parts) => parts,
            Err(response) => return response,
        };

    dispatch(prepared, request).await
}

async fn handle_anthropic_host_request<'a, Request, Parse, ParseError, Dispatch, DispatchFuture>(
    state: &'a Arc<GatewayRequestState>,
    headers: &HeaderMap,
    payload: &Value,
    parse_request: Parse,
    dispatch: Dispatch,
) -> Response
where
    Parse: FnOnce(&Value, &str) -> Result<Request, ParseError>,
    ParseError: Display,
    Dispatch: FnOnce(PreparedGatewayRequest<'a>, Request) -> DispatchFuture,
    DispatchFuture: Future<Output = Response>,
{
    let (prepared, request) =
        match prepare_and_parse_anthropic_request(state, headers, payload, parse_request).await {
            Ok(parts) => parts,
            Err(response) => return response,
        };

    dispatch(prepared, request).await
}
