use serde_json::{Value, json};

use crate::error::GatewayError;

pub fn openai_tools_to_anthropic_tools(
    tools: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
    let Some(Value::Array(items)) = tools else {
        return Ok(None);
    };

    let mut anthropic_tools = Vec::new();
    for tool in items {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }

        let function = tool.get("function").ok_or_else(|| {
            GatewayError::InvalidRequest("openai function tool requires function".to_string())
        })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GatewayError::InvalidRequest("openai function tool requires name".to_string())
            })?;

        let mut anthropic_tool = serde_json::Map::from_iter([
            ("name".to_string(), Value::String(name.to_string())),
            (
                "input_schema".to_string(),
                function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            ),
        ]);
        if let Some(description) = function.get("description").and_then(Value::as_str) {
            anthropic_tool.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        anthropic_tools.push(Value::Object(anthropic_tool));
    }

    Ok(Some(Value::Array(anthropic_tools)))
}

pub fn anthropic_tools_to_openai_tools(tools: Option<Value>) -> Option<Value> {
    let Value::Array(items) = tools? else {
        return None;
    };

    Some(Value::Array(
        items
            .into_iter()
            .map(|tool| {
                if tool.get("type").and_then(Value::as_str) == Some("function") {
                    return tool;
                }

                json!({
                    "type": "function",
                    "function": {
                        "name": tool.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        "description": tool.get("description").and_then(Value::as_str),
                        "parameters": tool
                            .get("input_schema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
                    }
                })
            })
            .collect(),
    ))
}

pub fn openai_tool_choice_to_anthropic_tool_choice(
    tool_choice: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
    match tool_choice {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" | "none" => Ok(Some(json!({ "type": mode }))),
            "required" => Ok(Some(json!({ "type": "any" }))),
            other => Err(GatewayError::InvalidRequest(format!(
                "unsupported openai tool_choice mode for anthropic request: {other}",
            ))),
        },
        Some(Value::Object(obj)) => match obj.get("type").and_then(Value::as_str) {
            Some("function") => obj
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .map(|name| json!({ "type": "tool", "name": name }))
                .map(Some)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "openai tool_choice function requires function.name".to_string(),
                    )
                }),
            Some("auto" | "none" | "any" | "tool") => Ok(Some(Value::Object(obj))),
            Some(other) => Err(GatewayError::InvalidRequest(format!(
                "unsupported openai tool_choice type for anthropic request: {other}",
            ))),
            None => Err(GatewayError::InvalidRequest(
                "openai tool_choice object is missing type".to_string(),
            )),
        },
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "openai tool_choice must be a string or object, got: {other}",
        ))),
        None => Ok(None),
    }
}

pub fn anthropic_tool_choice_to_openai_tool_choice(
    tool_choice: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
    match tool_choice {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" | "none" | "required" => Ok(Some(Value::String(mode))),
            "any" => Ok(Some(Value::String("required".to_string()))),
            other => Err(GatewayError::InvalidRequest(format!(
                "unsupported anthropic tool_choice mode: {other}",
            ))),
        },
        Some(Value::Object(obj)) => match obj.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(Value::String("auto".to_string()))),
            Some("any") => Ok(Some(Value::String("required".to_string()))),
            Some("none") => Ok(Some(Value::String("none".to_string()))),
            Some("tool") => obj
                .get("name")
                .and_then(Value::as_str)
                .map(|name| {
                    Value::Object(serde_json::Map::from_iter([
                        ("type".to_string(), Value::String("function".to_string())),
                        (
                            "function".to_string(),
                            Value::Object(serde_json::Map::from_iter([(
                                "name".to_string(),
                                Value::String(name.to_string()),
                            )])),
                        ),
                    ]))
                })
                .map(Some)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "anthropic tool_choice.type=tool requires a name".to_string(),
                    )
                }),
            Some("function") => Ok(Some(Value::Object(obj))),
            Some(other) => Err(GatewayError::InvalidRequest(format!(
                "unsupported anthropic tool_choice type: {other}",
            ))),
            None => Err(GatewayError::InvalidRequest(
                "anthropic tool_choice object is missing a type".to_string(),
            )),
        },
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "anthropic tool_choice must be a string or object, got: {other}",
        ))),
        None => Ok(None),
    }
}
