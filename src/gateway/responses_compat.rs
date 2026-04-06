use anyhow::{Result, anyhow};
use axum::{
    Json,
    body::Body,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{ResponsesRequest, ResponsesResponse};

use crate::protocol::{
    UpstreamProtocol, invoke_responses_stream_with_connector, invoke_responses_with_connector,
};
use crate::routing::ResolvedProvider;

pub(super) async fn invoke_legacy_responses_with_compat(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
    provider_family: Option<&str>,
    retry_without_tools_on_any_error: bool,
) -> Result<Response> {
    let stream = request.stream.unwrap_or(false);
    let mut result = if stream {
        invoke_legacy_responses_stream_with_fallback(
            protocol,
            base_url,
            api_key,
            request,
            provider_family,
        )
        .await
    } else {
        invoke_responses_with_connector(protocol, base_url, api_key, request, provider_family)
            .await
            .map(|resp| (StatusCode::OK, Json(resp)).into_response())
    };

    if let Err(error) = &result {
        let should_retry_without_tools = if retry_without_tools_on_any_error {
            request
                .tools
                .as_ref()
                .is_some_and(|tools| tools.as_array().is_some_and(|items| !items.is_empty()))
                || request.tool_choice.is_some()
        } else {
            let err_msg = format!("{error:#}");
            err_msg.contains("Failed to map responses.tools")
                || err_msg.contains("Failed to map responses.tool_choice")
        };

        if should_retry_without_tools {
            let mut req_compat = request.clone();
            req_compat.tools = None;
            req_compat.tool_choice = None;

            result = if stream {
                invoke_legacy_responses_stream_with_fallback(
                    protocol,
                    base_url,
                    api_key,
                    &req_compat,
                    provider_family,
                )
                .await
            } else {
                invoke_responses_with_connector(
                    protocol,
                    base_url,
                    api_key,
                    &req_compat,
                    provider_family,
                )
                .await
                .map(|resp| (StatusCode::OK, Json(resp)).into_response())
            };
        }
    }

    result
}

pub(crate) async fn invoke_legacy_responses_for_provider(
    provider: &ResolvedProvider,
    request: &ResponsesRequest,
) -> Result<Response> {
    let upstream_protocol = match provider.provider_type.as_str() {
        "anthropic" => UpstreamProtocol::Anthropic,
        _ => UpstreamProtocol::OpenAi,
    };

    invoke_legacy_responses_with_compat(
        upstream_protocol,
        &provider.base_url,
        &provider.api_key,
        request,
        provider.family_id.as_deref(),
        true,
    )
    .await
}

pub(crate) async fn invoke_legacy_responses_for_env(
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
) -> Result<Response> {
    invoke_legacy_responses_with_compat(
        UpstreamProtocol::OpenAi,
        base_url,
        api_key,
        request,
        None,
        false,
    )
    .await
}

fn response_text(resp: &ResponsesResponse) -> String {
    if !resp.output_text.is_empty() {
        return resp.output_text.clone();
    }

    resp.output
        .as_ref()
        .map(|items| {
            items
                .iter()
                .flat_map(|item| item.content.as_ref().into_iter().flatten())
                .filter_map(|content| content.text.clone())
                .collect::<Vec<String>>()
                .join("")
        })
        .unwrap_or_default()
}

fn build_responses_stream_response_from_full(resp: ResponsesResponse) -> Result<Response> {
    let response_id = resp.id.clone();
    let model = resp.model.clone();
    let text = response_text(&resp);
    let usage = resp.usage.clone();

    let mut chunks: Vec<Result<Bytes, std::io::Error>> = Vec::new();

    let created = serde_json::json!({
        "type": "response.created",
        "response": {
            "id": response_id,
            "object": "response",
            "model": model,
            "status": "in_progress"
        }
    });
    chunks.push(Ok(Bytes::from(format!(
        "event: response.created\ndata: {}\n\n",
        created
    ))));

    if !text.is_empty() {
        let delta = serde_json::json!({
            "type": "response.output_text.delta",
            "response_id": response_id,
            "delta": text,
        });
        chunks.push(Ok(Bytes::from(format!(
            "event: response.output_text.delta\ndata: {}\n\n",
            delta
        ))));
    }

    let completed = serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": response_id,
            "object": "response",
            "model": model,
            "status": "completed",
            "usage": usage,
        }
    });
    chunks.push(Ok(Bytes::from(format!(
        "event: response.completed\ndata: {}\n\n",
        completed
    ))));
    chunks.push(Ok(Bytes::from("data: [DONE]\n\n")));

    let sse_stream = futures_util::stream::iter(chunks);
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(sse_stream))
        .map_err(|e| anyhow!("build responses stream fallback: {e}"))
}

async fn invoke_legacy_responses_stream_with_fallback(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
    provider_family: Option<&str>,
) -> Result<Response> {
    match invoke_responses_stream_with_connector(
        protocol,
        base_url,
        api_key,
        request,
        provider_family,
    )
    .await
    {
        Ok(stream) => {
            let sse_stream = stream.map(|event| match event {
                Ok(event) => {
                    let mut event_data = event.data;
                    event_data
                        .entry("type".to_string())
                        .or_insert_with(|| serde_json::Value::String(event.event_type.clone()));
                    let data =
                        serde_json::to_string(&event_data).unwrap_or_else(|_| String::from("{}"));
                    let chunk = format!("event: {}\ndata: {}\n\n", event.event_type, data);
                    Ok::<Bytes, std::io::Error>(Bytes::from(chunk))
                }
                Err(err) => Err(std::io::Error::other(err.to_string())),
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(sse_stream))
                .map_err(|e| anyhow!("build responses stream: {e}"))
        }
        Err(stream_err) => {
            tracing::warn!(error = %stream_err, "responses streaming failed, fallback to non-stream -> sse");
            let full_resp = invoke_responses_with_connector(
                protocol,
                base_url,
                api_key,
                request,
                provider_family,
            )
            .await
            .map_err(|e| anyhow!("stream failed: {stream_err}; non-stream fallback failed: {e}"))?;

            build_responses_stream_response_from_full(full_resp)
        }
    }
}
