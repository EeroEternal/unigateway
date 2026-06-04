use crate::capabilities::OpenAiApiSurfaceCapabilities;
use crate::conversion::proxy_responses_request_uses_tools_and_reasoning;
use crate::error::GatewayError;
use crate::request::ProxyResponsesRequest;

/// Whether a failed Responses attempt may be retried with `tools` / `tool_choice` removed.
pub fn should_retry_responses_without_tools(
    request: &ProxyResponsesRequest,
    error: &GatewayError,
    surface: &OpenAiApiSurfaceCapabilities,
) -> bool {
    let has_tools = request.tools.is_some() || request.tool_choice.is_some();
    if !has_tools {
        return false;
    }
    if proxy_responses_request_uses_tools_and_reasoning(request) {
        return false;
    }
    if upstream_error_forbids_tool_strip_retry(error) {
        return false;
    }

    match surface.optional_tools_on_responses_failure_retry {
        Some(true) => true,
        Some(false) | None => responses_tool_strip_retry_whitelisted(error),
    }
}

fn upstream_error_forbids_tool_strip_retry(error: &GatewayError) -> bool {
    let Some(body) = upstream_error_body(error) else {
        return false;
    };
    let lower = body.to_ascii_lowercase();
    lower.contains("/v1/responses")
        || lower.contains("please use /v1/responses")
        || lower.contains("not supported for gpt-5.5 in /v1/chat/completions")
}

fn responses_tool_strip_retry_whitelisted(error: &GatewayError) -> bool {
    if upstream_error_forbids_tool_strip_retry(error) {
        return false;
    }

    let (status, body) = match error {
        GatewayError::UpstreamHttp { status, body, .. } => (*status, body.as_deref()),
        _ => return false,
    };

    let body = body.unwrap_or("");
    let lower = body.to_ascii_lowercase();

    if lower.contains("function tools with reasoning_effort") {
        return false;
    }

    (lower.contains("does not support") && lower.contains("tool"))
        || lower.contains("tools are not supported")
        || lower.contains("unsupported tool")
        || (matches!(status, 400 | 422) && lower.contains("tool"))
}

fn upstream_error_body(error: &GatewayError) -> Option<&str> {
    match error {
        GatewayError::UpstreamHttp {
            body: Some(body), ..
        } => Some(body.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::capabilities::OpenAiApiSurfaceCapabilities;
    use serde_json::json;

    fn gpt55_surface() -> OpenAiApiSurfaceCapabilities {
        OpenAiApiSurfaceCapabilities::resolve_for_model("gpt-5.5", None)
    }

    #[test]
    fn does_not_strip_tools_when_reasoning_and_tools_present() {
        let request = ProxyResponsesRequest {
            model: "gpt-5.5".to_string(),
            input: None,
            instructions: None,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stream: false,
            tools: Some(json!([])),
            tool_choice: None,
            reasoning: Some(json!({"effort": "low"})),
            previous_response_id: None,
            request_metadata: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };
        let error = GatewayError::UpstreamHttp {
            status: 500,
            body: Some("upstream failed".to_string()),
            endpoint_id: "ep".to_string(),
        };

        assert!(!should_retry_responses_without_tools(
            &request,
            &error,
            &gpt55_surface()
        ));
    }

    #[test]
    fn does_not_strip_tools_when_upstream_requires_responses_api() {
        let request = ProxyResponsesRequest {
            model: "gpt-5.5".to_string(),
            input: None,
            instructions: None,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stream: false,
            tools: Some(json!([])),
            tool_choice: None,
            reasoning: None,
            previous_response_id: None,
            request_metadata: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };
        let error = GatewayError::UpstreamHttp {
            status: 400,
            body: Some(
                "Function tools with reasoning_effort are not supported for gpt-5.5 in /v1/chat/completions. Please use /v1/responses instead.".to_string(),
            ),
            endpoint_id: "ep".to_string(),
        };

        assert!(!should_retry_responses_without_tools(
            &request,
            &error,
            &gpt55_surface()
        ));
    }
}
