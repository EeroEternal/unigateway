use anyhow::{Context, Result, anyhow};
use llm_connector::{
    ChatResponse, LlmClient,
    types::{
        ChatRequest, ChatStream, EmbedRequest, EmbedResponse, ResponsesRequest, ResponsesResponse,
        ResponsesStream,
    },
};
use tracing::debug;

use super::UpstreamProtocol;

fn build_openai_client(
    base_url: &str,
    api_key: &str,
    _family_id: Option<&str>,
) -> Result<LlmClient, anyhow::Error> {
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
) -> Result<LlmClient, anyhow::Error> {
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

pub(crate) async fn invoke_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<ChatResponse> {
    debug!(
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
    debug!(
        response_id = resp.id.as_str(),
        response_model = resp.model.as_str(),
        response_created = resp.created,
        choices = resp.choices.len(),
        usage_present = resp.usage.is_some(),
        first_content_len = resp
            .choices
            .first()
            .map(|c| c.message.content_as_text().len())
            .unwrap_or(0),
        "llm-connector chat returned"
    );
    Ok(resp)
}

pub(crate) async fn invoke_with_connector_stream(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<ChatStream, anyhow::Error> {
    let client = build_client(protocol, base_url, api_key, family_id)?;
    client
        .chat_stream(req)
        .await
        .context("llm-connector chat_stream failed")
}

pub(crate) async fn invoke_embeddings(
    base_url: &str,
    api_key: &str,
    req: &EmbedRequest,
) -> Result<EmbedResponse> {
    debug!(
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
    debug!(
        model = resp.model.as_str(),
        data_count = resp.data.len(),
        "llm-connector embed returned"
    );
    Ok(resp)
}

pub(crate) async fn invoke_responses_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ResponsesRequest,
    family_id: Option<&str>,
) -> Result<ResponsesResponse> {
    debug!(
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

pub(crate) async fn invoke_responses_stream_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ResponsesRequest,
    family_id: Option<&str>,
) -> Result<ResponsesStream, anyhow::Error> {
    let client = build_client(protocol, base_url, api_key, family_id)?;
    client
        .invoke_responses_stream(req)
        .await
        .context("llm-connector responses stream failed")
}
