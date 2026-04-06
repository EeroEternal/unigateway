use anyhow::{Context, Result, anyhow};
use axum::{
    Json,
    body::Body,
    http::{StatusCode, header, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::{
    ChatResponse, LlmClient,
    types::{
        ChatRequest, ChatStream, EmbedRequest, EmbedResponse, ResponsesRequest, ResponsesResponse,
        ResponsesStream, StreamChunk, StreamFormat, streaming::AnthropicSseAdapter,
    },
};
use serde_json::{Value, json};
use unigateway_runtime::host::{ResolvedProvider, RuntimeContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamProtocol {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownstreamChatFormat {
    OpenAi,
    Anthropic,
}

pub(crate) async fn invoke_openai_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_chat_via_legacy(
        runtime,
        service_id,
        "openai",
        hint,
        request,
        DownstreamChatFormat::OpenAi,
    )
    .await
}

pub(crate) async fn invoke_openai_chat_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_direct_chat(
        UpstreamProtocol::OpenAi,
        base_url,
        api_key,
        request,
        None,
        DownstreamChatFormat::OpenAi,
    )
    .await
}

pub(crate) async fn invoke_openai_responses_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ResponsesRequest,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, "openai", hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        match invoke_legacy_responses_for_provider(provider, &mapped).await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream responses error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

pub(crate) async fn invoke_openai_responses_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
) -> Result<Response> {
    invoke_legacy_responses_for_env(base_url, api_key, request).await
}

pub(crate) async fn invoke_anthropic_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_chat_via_legacy(
        runtime,
        service_id,
        "anthropic",
        hint,
        request,
        DownstreamChatFormat::Anthropic,
    )
    .await
}

pub(crate) async fn invoke_anthropic_chat_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<Response> {
    invoke_direct_chat(
        UpstreamProtocol::Anthropic,
        base_url,
        api_key,
        request,
        None,
        DownstreamChatFormat::Anthropic,
    )
    .await
}

pub(crate) async fn invoke_openai_embeddings_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &EmbedRequest,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, "openai", hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        tracing::debug!(
            provider_name = provider.name.as_str(),
            model = mapped.model.as_str(),
            "routing embeddings request to provider"
        );

        match invoke_embeddings(&provider.base_url, &provider.api_key, &mapped).await {
            Ok(resp) => {
                return Ok(
                    (StatusCode::OK, Json(embed_response_to_openai_json(&resp))).into_response()
                );
            }
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

pub(crate) async fn invoke_openai_embeddings_via_env_legacy(
    base_url: &str,
    api_key: &str,
    request: &EmbedRequest,
) -> Result<Response> {
    invoke_embeddings(base_url, api_key, request)
        .await
        .map(|resp| (StatusCode::OK, Json(embed_response_to_openai_json(&resp))).into_response())
}

async fn invoke_chat_via_legacy(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    protocol_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
    downstream: DownstreamChatFormat,
) -> Result<Response> {
    let providers = resolve_legacy_providers(runtime, service_id, protocol_id, hint).await?;
    let original_model = request.model.clone();
    let mut last_err = String::from("unknown");

    for provider in &providers {
        let mut mapped = request.clone();
        mapped.model = provider.map_model(&original_model);

        let upstream_protocol = match provider.provider_type.as_str() {
            "anthropic" => UpstreamProtocol::Anthropic,
            _ => UpstreamProtocol::OpenAi,
        };

        tracing::debug!(
            provider_name = provider.name.as_str(),
            base_url = provider.base_url.as_str(),
            model = mapped.model.as_str(),
            upstream_protocol = ?upstream_protocol,
            "routing request to provider"
        );

        match invoke_provider_chat(upstream_protocol, provider, &mapped, downstream).await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                last_err = format!("{err:#}");
            }
        }
    }

    Err(anyhow!("all providers failed, last: {last_err}"))
}

async fn resolve_legacy_providers(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    protocol: &str,
    hint: Option<&str>,
) -> Result<Vec<ResolvedProvider>> {
    runtime.resolve_providers(service_id, protocol, hint).await
}

async fn invoke_provider_chat(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &ChatRequest,
    downstream: DownstreamChatFormat,
) -> Result<Response> {
    if request.stream == Some(true) {
        try_chat_stream(
            protocol,
            provider,
            request,
            downstream == DownstreamChatFormat::Anthropic,
        )
        .await
    } else {
        invoke_with_connector(
            protocol,
            &provider.base_url,
            &provider.api_key,
            request,
            provider.family_id.as_deref(),
        )
        .await
        .map(|resp| {
            let body = match downstream {
                DownstreamChatFormat::OpenAi => chat_response_to_openai_json(&resp),
                DownstreamChatFormat::Anthropic => chat_response_to_anthropic_json(&resp),
            };
            (StatusCode::OK, Json(body)).into_response()
        })
    }
}

async fn invoke_direct_chat(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    family_id: Option<&str>,
    downstream: DownstreamChatFormat,
) -> Result<Response> {
    if request.stream == Some(true) {
        try_chat_stream_raw(
            protocol,
            base_url,
            api_key,
            request,
            family_id,
            downstream == DownstreamChatFormat::Anthropic,
        )
        .await
    } else {
        invoke_with_connector(protocol, base_url, api_key, request, family_id)
            .await
            .map(|resp| {
                let body = match downstream {
                    DownstreamChatFormat::OpenAi => chat_response_to_openai_json(&resp),
                    DownstreamChatFormat::Anthropic => chat_response_to_anthropic_json(&resp),
                };
                (StatusCode::OK, Json(body)).into_response()
            })
    }
}

async fn try_chat_stream(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &ChatRequest,
    is_anthropic_downstream: bool,
) -> Result<Response> {
    try_chat_stream_raw(
        protocol,
        &provider.base_url,
        &provider.api_key,
        request,
        provider.family_id.as_deref(),
        is_anthropic_downstream,
    )
    .await
}

async fn try_chat_stream_raw(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    family_id: Option<&str>,
    is_anthropic_downstream: bool,
) -> Result<Response> {
    let stream =
        invoke_with_connector_stream(protocol, base_url, api_key, request, family_id).await?;
    type BoxErr = Box<dyn std::error::Error + Send + Sync>;

    if is_anthropic_downstream {
        let adapter = std::sync::Arc::new(std::sync::Mutex::new(AnthropicSseAdapter::new()));

        let sse_stream = stream.flat_map(
            move |r: Result<_, llm_connector::error::LlmConnectorError>| {
                let adapter = adapter.clone();

                let result: Vec<Result<Bytes, BoxErr>> = match r {
                    Ok(resp) => {
                        let mut guard = adapter.lock().expect("adapter lock");
                        let events = guard.convert(&resp);
                        events
                            .into_iter()
                            .map(|event| Ok(Bytes::from(event)))
                            .collect()
                    }
                    Err(error) => {
                        tracing::error!("llm-connector chat_stream failed: {}", error);
                        vec![Err(Box::new(std::io::Error::other(format!(
                            "llm-connector chat_stream failed: {}",
                            error
                        ))) as BoxErr)]
                    }
                };

                futures_util::stream::iter(result)
            },
        );

        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(sse_stream),
        )
            .into_response())
    } else {
        let sse_stream = stream.map(|r: Result<_, llm_connector::error::LlmConnectorError>| {
            r.map_err(|error| -> BoxErr { Box::new(std::io::Error::other(error.to_string())) })
                .and_then(|resp| {
                    StreamChunk::from_openai(&resp, StreamFormat::SSE)
                        .map(|chunk| Bytes::from(chunk.to_sse()))
                        .map_err(|error: serde_json::Error| -> BoxErr { Box::new(error) })
                })
        });
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(sse_stream),
        )
            .into_response())
    }
}

async fn invoke_legacy_responses_with_compat(
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

async fn invoke_legacy_responses_for_provider(
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

async fn invoke_legacy_responses_for_env(
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

    let created = json!({
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
        let delta = json!({
            "type": "response.output_text.delta",
            "response_id": response_id,
            "delta": text,
        });
        chunks.push(Ok(Bytes::from(format!(
            "event: response.output_text.delta\ndata: {}\n\n",
            delta
        ))));
    }

    let completed = json!({
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
        .map_err(|error| anyhow!("build responses stream fallback: {error}"))
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
                        .or_insert_with(|| Value::String(event.event_type.clone()));
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
                .map_err(|error| anyhow!("build responses stream: {error}"))
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
            .map_err(|error| {
                anyhow!("stream failed: {stream_err}; non-stream fallback failed: {error}")
            })?;

            build_responses_stream_response_from_full(full_resp)
        }
    }
}

fn build_openai_client(
    base_url: &str,
    api_key: &str,
    _family_id: Option<&str>,
) -> Result<LlmClient> {
    if api_key.is_empty() {
        return Err(anyhow!("missing upstream api key"));
    }
    LlmClient::openai(api_key, base_url).context("failed to create openai client")
}

fn build_client(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    family_id: Option<&str>,
) -> Result<LlmClient> {
    match protocol {
        UpstreamProtocol::OpenAi => build_openai_client(base_url, api_key, family_id),
        UpstreamProtocol::Anthropic => {
            if api_key.is_empty() {
                return Err(anyhow!("missing upstream api key"));
            }
            LlmClient::anthropic_with_config(api_key, base_url, None, None)
                .context("failed to create anthropic client")
        }
    }
}

async fn invoke_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<ChatResponse> {
    tracing::debug!(
        protocol = match protocol {
            UpstreamProtocol::OpenAi => "openai",
            UpstreamProtocol::Anthropic => "anthropic",
        },
        base_url,
        family_id = family_id.unwrap_or(""),
        model = req.model.as_str(),
        stream = req.stream.unwrap_or(false),
        message_count = req.messages.len(),
        "invoking llm-connector"
    );
    let client = build_client(protocol, base_url, api_key, family_id)?;
    let resp = client
        .chat(req)
        .await
        .context("llm-connector chat failed")?;
    tracing::debug!(
        response_id = resp.id.as_str(),
        response_model = resp.model.as_str(),
        response_created = resp.created,
        choices = resp.choices.len(),
        usage_present = resp.usage.is_some(),
        first_content_len = resp
            .choices
            .first()
            .map(|choice| choice.message.content_as_text().len())
            .unwrap_or(0),
        "llm-connector chat returned"
    );
    Ok(resp)
}

async fn invoke_with_connector_stream(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<ChatStream> {
    let client = build_client(protocol, base_url, api_key, family_id)?;
    client
        .chat_stream(req)
        .await
        .context("llm-connector chat_stream failed")
}

async fn invoke_embeddings(
    base_url: &str,
    api_key: &str,
    req: &EmbedRequest,
) -> Result<EmbedResponse> {
    tracing::debug!(
        base_url,
        model = req.model.as_str(),
        input_count = req.input.len(),
        "invoking llm-connector embed"
    );
    let client = build_openai_client(base_url, api_key, None)?;
    let resp = client
        .embed(req)
        .await
        .context("llm-connector embed failed")?;
    tracing::debug!(
        model = resp.model.as_str(),
        data_count = resp.data.len(),
        "llm-connector embed returned"
    );
    Ok(resp)
}

async fn invoke_responses_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ResponsesRequest,
    family_id: Option<&str>,
) -> Result<ResponsesResponse> {
    tracing::debug!(
        protocol = match protocol {
            UpstreamProtocol::OpenAi => "openai",
            UpstreamProtocol::Anthropic => "anthropic",
        },
        base_url,
        family_id = family_id.unwrap_or(""),
        model = req.model.as_str(),
        stream = req.stream.unwrap_or(false),
        "invoking llm-connector responses"
    );
    let client = build_client(protocol, base_url, api_key, family_id)?;
    client
        .invoke_responses(req)
        .await
        .context("llm-connector responses failed")
}

async fn invoke_responses_stream_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ResponsesRequest,
    family_id: Option<&str>,
) -> Result<ResponsesStream> {
    let client = build_client(protocol, base_url, api_key, family_id)?;
    client
        .invoke_responses_stream(req)
        .await
        .context("llm-connector responses stream failed")
}

fn chat_response_to_openai_json(resp: &ChatResponse) -> Value {
    let content = resp
        .choices
        .first()
        .map(|choice| choice.message.content_as_text())
        .unwrap_or_default();

    json!({
        "id": resp.id,
        "object": if resp.object.is_empty() { "chat.completion" } else { &resp.object },
        "created": resp.created,
        "model": resp.model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": resp.finish_reason().unwrap_or("stop")
        }],
        "usage": resp.usage.as_ref().map(|usage| json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens
        }))
    })
}

fn chat_response_to_anthropic_json(resp: &ChatResponse) -> Value {
    let content = resp
        .choices
        .first()
        .map(|choice| choice.message.content_as_text())
        .unwrap_or_default();

    json!({
        "id": resp.id,
        "type": "message",
        "role": "assistant",
        "model": resp.model,
        "content": [
            {
                "type": "text",
                "text": content
            }
        ],
        "stop_reason": resp.finish_reason().unwrap_or("end_turn"),
        "usage": {
            "input_tokens": resp.prompt_tokens(),
            "output_tokens": resp.completion_tokens()
        }
    })
}

fn embed_response_to_openai_json(resp: &EmbedResponse) -> Value {
    let data: Vec<Value> = resp
        .data
        .iter()
        .map(|item| {
            json!({
                "object": "embedding",
                "embedding": item.embedding,
                "index": item.index,
            })
        })
        .collect();

    json!({
        "object": "list",
        "data": data,
        "model": resp.model,
        "usage": {
            "prompt_tokens": resp.usage.prompt_tokens,
            "total_tokens": resp.usage.total_tokens
        }
    })
}
