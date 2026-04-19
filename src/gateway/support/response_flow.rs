use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json,
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};
use unigateway_host::flow::HostResponseResult;
use unigateway_protocol::{RuntimeHttpResponse, RuntimeResponseBody};

use crate::middleware::{GatewayAuth, record_stat};
use crate::types::GatewayRequestState;

use super::request_flow::PreparedGatewayRequest;

pub(super) async fn respond_prepared_host_result(
    state: &Arc<GatewayRequestState>,
    prepared: &PreparedGatewayRequest<'_>,
    endpoint: &str,
    result: HostResponseResult,
) -> Response {
    match prepared.auth.as_ref() {
        Some(auth) => {
            respond_authenticated_host_result(state, auth, endpoint, &prepared.start, result).await
        }
        None => respond_env_host_result(state, endpoint, &prepared.start, result).await,
    }
}

pub(super) async fn respond_authenticated_host_result(
    state: &Arc<GatewayRequestState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    result: HostResponseResult,
) -> Response {
    match result {
        Ok(response) => gateway_success_response(state, auth, endpoint, start, response).await,
        Err(response) => gateway_error_response(state, auth, endpoint, start, response).await,
    }
}

pub(super) async fn respond_env_host_result(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    result: HostResponseResult,
) -> Response {
    match result {
        Ok(response) => success_response(state, endpoint, start, response).await,
        Err(response) => error_response(state, endpoint, start, response).await,
    }
}

async fn gateway_success_response(
    state: &Arc<GatewayRequestState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: RuntimeHttpResponse,
) -> Response {
    auth.finalize(state).await;
    success_response(state, endpoint, start, response).await
}

async fn gateway_error_response(
    state: &Arc<GatewayRequestState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: RuntimeHttpResponse,
) -> Response {
    auth.release(state).await;
    error_response(state, endpoint, start, response).await
}

async fn success_response(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    response: RuntimeHttpResponse,
) -> Response {
    record_stat(state, endpoint, 200, start).await;
    into_axum_response(response)
}

async fn error_response(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    response: RuntimeHttpResponse,
) -> Response {
    record_stat(state, endpoint, 500, start).await;
    into_axum_response(response)
}

fn into_axum_response(response: RuntimeHttpResponse) -> Response {
    let (status, body) = response.into_parts();

    match body {
        RuntimeResponseBody::Json(value) => (status, Json(value)).into_response(),
        RuntimeResponseBody::ServerSentEvents(stream) => (
            status,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response(),
    }
}
