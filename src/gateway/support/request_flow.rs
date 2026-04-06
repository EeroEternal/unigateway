use std::fmt::Display;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::Value;
use tracing::info;
use unigateway_runtime::host::RuntimeContext;

use crate::middleware::{GatewayAuth, error_json, extract_openai_api_key, extract_x_api_key};
use crate::routing::target_provider_hint;
use crate::types::AppState;

pub(super) struct PreparedGatewayRequest<'a> {
    pub start: Instant,
    pub runtime: RuntimeContext<'a>,
    pub token: String,
    pub hint: Option<String>,
    pub auth: Option<GatewayAuth>,
}

pub(super) async fn prepare_and_parse_openai_request<'a, Request, Parse, ParseError>(
    state: &'a Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
    parse_request: Parse,
) -> Result<(PreparedGatewayRequest<'a>, Request), Response>
where
    Parse: FnOnce(&Value, &str) -> Result<Request, ParseError>,
    ParseError: Display,
{
    let prepared = prepare_openai_request(state, headers, payload).await?;
    parse_prepared_request(prepared, payload, parse_request, |prepared| {
        prepared.runtime.config.openai_model
    })
    .await
}

pub(super) async fn prepare_and_parse_anthropic_request<'a, Request, Parse, ParseError>(
    state: &'a Arc<AppState>,
    headers: &HeaderMap,
    payload: &Value,
    parse_request: Parse,
) -> Result<(PreparedGatewayRequest<'a>, Request), Response>
where
    Parse: FnOnce(&Value, &str) -> Result<Request, ParseError>,
    ParseError: Display,
{
    let prepared = prepare_anthropic_request(state, headers, payload).await?;
    parse_prepared_request(prepared, payload, parse_request, |prepared| {
        prepared.runtime.config.anthropic_model
    })
    .await
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
    let runtime = RuntimeContext::from_parts(
        state.as_ref(),
        state.as_ref(),
        state.as_ref(),
        state.as_ref(),
    );
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

async fn parse_prepared_request<'a, Request, Parse, ParseError, Model>(
    prepared: PreparedGatewayRequest<'a>,
    payload: &Value,
    parse_request: Parse,
    model: Model,
) -> Result<(PreparedGatewayRequest<'a>, Request), Response>
where
    Parse: FnOnce(&Value, &str) -> Result<Request, ParseError>,
    ParseError: Display,
    Model: FnOnce(&PreparedGatewayRequest<'a>) -> &'a str,
{
    let request = parse_request(payload, model(&prepared)).map_err(invalid_request_response)?;

    Ok((prepared, request))
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

fn invalid_request_response(error: impl Display) -> Response {
    error_json(
        StatusCode::BAD_REQUEST,
        &format!("invalid request: {error}"),
    )
}
