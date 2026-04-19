use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json,
    body::Body,
    http::StatusCode,
    http::header,
    response::{IntoResponse, Response},
};
use serde_json::json;
use unigateway_host::{HostDispatchOutcome, HostError, HostResult, status::status_for_host_error};
use unigateway_protocol::{ProtocolHttpResponse, ProtocolResponseBody};

use crate::middleware::{GatewayAuth, record_stat};
use crate::types::GatewayRequestState;

use super::request_flow::PreparedGatewayRequest;

pub(super) type HostResponseResult =
    std::result::Result<ProtocolHttpResponse, ProtocolHttpResponse>;

pub(super) async fn resolve_core_only_host_flow<CoreFuture>(
    core_attempt: CoreFuture,
    unavailable_message: &str,
) -> HostResponseResult
where
    CoreFuture: std::future::Future<Output = HostResult<HostDispatchOutcome>>,
{
    match core_attempt.await {
        Ok(HostDispatchOutcome::Response(response)) => Ok(response),
        Ok(HostDispatchOutcome::PoolNotFound) => Err(error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            unavailable_message,
        )),
        Ok(_) => Err(error_json(
            StatusCode::BAD_GATEWAY,
            "unsupported host dispatch outcome",
        )),
        Err(error) => Err(host_error_response(&error)),
    }
}

pub(super) fn missing_upstream_api_key_response() -> ProtocolHttpResponse {
    error_json(StatusCode::BAD_REQUEST, "missing upstream api key")
}

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
    response: ProtocolHttpResponse,
) -> Response {
    auth.finalize(state).await;
    success_response(state, endpoint, start, response).await
}

async fn gateway_error_response(
    state: &Arc<GatewayRequestState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: ProtocolHttpResponse,
) -> Response {
    auth.release(state).await;
    error_response(state, endpoint, start, response).await
}

async fn success_response(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    response: ProtocolHttpResponse,
) -> Response {
    recorded_response(state, endpoint, start, response).await
}

async fn error_response(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    response: ProtocolHttpResponse,
) -> Response {
    recorded_response(state, endpoint, start, response).await
}

async fn recorded_response(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    start: &Instant,
    response: ProtocolHttpResponse,
) -> Response {
    let (status, body) = response.into_parts();
    record_stat(state, endpoint, status.as_u16(), start).await;
    into_axum_response(status, body)
}

fn into_axum_response(status: StatusCode, body: ProtocolResponseBody) -> Response {
    match body {
        ProtocolResponseBody::Json(value) => (status, Json(value)).into_response(),
        ProtocolResponseBody::ServerSentEvents(stream) => (
            status,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response(),
    }
}

fn host_error_response(error: &HostError) -> ProtocolHttpResponse {
    error_json(
        status_for_host_error(error),
        &format!("host execution error: {error}"),
    )
}

fn error_json(status: StatusCode, message: &str) -> ProtocolHttpResponse {
    ProtocolHttpResponse::json(status, json!({"error": {"message": message}}))
}
