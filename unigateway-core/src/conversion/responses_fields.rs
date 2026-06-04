use serde_json::{Value, json};

use crate::request::ProxyResponsesRequest;

/// Normalizes Chat Completions–style fields on a Responses request before upstream rendering.
///
/// Maps `reasoning_effort` (string or object) to OpenAI Responses `reasoning` and removes
/// chat-only keys from `extra` so they are not sent verbatim to `/v1/responses`.
pub fn normalize_proxy_responses_request(request: &mut ProxyResponsesRequest) {
    if request.reasoning.is_none() {
        if let Some(reasoning) = request.extra.remove("reasoning") {
            request.reasoning = Some(reasoning);
        } else if let Some(effort) = request.extra.remove("reasoning_effort") {
            request.reasoning = Some(reasoning_value_from_effort(effort));
        }
    } else {
        request.extra.remove("reasoning");
        request.extra.remove("reasoning_effort");
    }
}

/// Returns true when the request carries both tool declarations and reasoning configuration.
pub fn proxy_responses_request_uses_tools_and_reasoning(request: &ProxyResponsesRequest) -> bool {
    let has_tools = request.tools.is_some() || request.tool_choice.is_some();
    let has_reasoning = request.reasoning.is_some()
        || request.extra.contains_key("reasoning")
        || request.extra.contains_key("reasoning_effort");
    has_tools && has_reasoning
}

fn reasoning_value_from_effort(effort: Value) -> Value {
    match effort {
        Value::String(effort) => json!({ "effort": effort }),
        Value::Object(map) if map.contains_key("effort") => Value::Object(map),
        other => json!({ "effort": other }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use serde_json::json;

    #[test]
    fn maps_reasoning_effort_string_to_reasoning_object() {
        let mut request = ProxyResponsesRequest {
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
            extra: HashMap::from([("reasoning_effort".to_string(), json!("low"))]),
            metadata: HashMap::new(),
        };

        normalize_proxy_responses_request(&mut request);

        assert_eq!(request.reasoning, Some(json!({"effort": "low"})));
        assert!(!request.extra.contains_key("reasoning_effort"));
    }

    #[test]
    fn explicit_reasoning_field_wins_over_extra() {
        let mut request = ProxyResponsesRequest {
            model: "gpt-5.5".to_string(),
            input: None,
            instructions: None,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stream: false,
            tools: None,
            tool_choice: None,
            reasoning: Some(json!({"effort": "high"})),
            previous_response_id: None,
            request_metadata: None,
            extra: HashMap::from([("reasoning_effort".to_string(), json!("low"))]),
            metadata: HashMap::new(),
        };

        normalize_proxy_responses_request(&mut request);

        assert_eq!(request.reasoning, Some(json!({"effort": "high"})));
        assert!(!request.extra.contains_key("reasoning_effort"));
    }

    #[test]
    fn detects_tools_and_reasoning_combination() {
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
            reasoning: Some(json!({"effort": "medium"})),
            previous_response_id: None,
            request_metadata: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };

        assert!(proxy_responses_request_uses_tools_and_reasoning(&request));
    }
}
