use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{
    AnthropicInputJsonDelta, AnthropicToolUseStart, PendingOpenAiToolCall,
    anthropic_content_to_blocks, anthropic_messages_to_openai_messages,
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
    apply_openai_tool_call_delta_update, content_blocks_to_anthropic,
    content_blocks_to_anthropic_request, flush_openai_tool_call_stop_update,
    openai_message_to_anthropic_content_blocks, openai_message_to_content_blocks,
    openai_messages_to_anthropic_messages, openai_tool_choice_to_anthropic_tool_choice,
    openai_tools_to_anthropic_tools, validate_anthropic_request_messages,
};
use crate::request::{ContentBlock, THINKING_SIGNATURE_PLACEHOLDER_VALUE};

#[test]
fn content_block_preserves_thinking_signature() {
    let block = ContentBlock::Thinking {
        thinking: "reasoning".to_string(),
        signature: Some("real-signature".to_string()),
    };

    let value = serde_json::to_value(&block).expect("serialize block");
    assert_eq!(
        value.get("type").and_then(serde_json::Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        value.get("signature").and_then(serde_json::Value::as_str),
        Some("real-signature")
    );

    let parsed: ContentBlock = serde_json::from_value(value).expect("deserialize block");
    assert_eq!(parsed, block);
}

#[test]
fn content_block_preserves_tool_use_input() {
    let block = ContentBlock::ToolUse {
        id: "toolu_1".to_string(),
        name: "search".to_string(),
        input: json!({"query": "hello"}),
    };

    let value = serde_json::to_value(&block).expect("serialize block");
    assert_eq!(
        value.get("type").and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(value.get("input"), Some(&json!({"query": "hello"})));
}

#[test]
fn openai_assistant_tool_calls_parse_to_content_blocks() {
    let blocks = openai_message_to_content_blocks(&json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {
                "name": "search",
                "arguments": "{\"query\":\"rust\"}"
            }
        }]
    }))
    .expect("content blocks");

    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0],
        ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "search".to_string(),
            input: json!({"query": "rust"}),
        }
    );
}

#[test]
fn openai_image_blocks_parse_to_content_blocks() {
    let blocks = openai_message_to_content_blocks(&json!({
        "role": "user",
        "content": [{
            "type": "image_url",
            "image_url": {
                "url": "https://example.com/a.png",
                "detail": "high"
            }
        }]
    }))
    .expect("content blocks");

    assert_eq!(
        blocks,
        vec![ContentBlock::Image {
            source: json!({
                "type": "url",
                "url": "https://example.com/a.png"
            }),
            detail: Some("high".to_string()),
        }]
    );
}

#[test]
fn anthropic_image_blocks_parse_to_content_blocks() {
    let blocks = anthropic_content_to_blocks(&json!([{
        "type": "image",
        "source": {
            "type": "url",
            "url": "https://example.com/a.png"
        }
    }]))
    .expect("content blocks");

    assert_eq!(
        blocks,
        vec![ContentBlock::Image {
            source: json!({
                "type": "url",
                "url": "https://example.com/a.png"
            }),
            detail: None,
        }]
    );
}

#[test]
fn openai_assistant_tool_calls_ignore_empty_string_content() {
    let (system, messages) = openai_messages_to_anthropic_messages(
        &json!([
            {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust\"}"
                    }
                }]
            }
        ]),
        None,
    )
    .expect("anthropic messages");

    assert_eq!(system, None);
    assert_eq!(
        messages
            .pointer("/0/content/0/type")
            .and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        messages.pointer("/0/content/1").and_then(Value::as_object),
        None
    );
}

#[test]
fn openai_tool_message_parses_to_tool_result_block() {
    let blocks = openai_message_to_content_blocks(&json!({
        "role": "tool",
        "tool_call_id": "call_1",
        "content": "search result"
    }))
    .expect("content blocks");

    assert_eq!(
        blocks,
        vec![ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: json!("search result"),
        }]
    );
}

#[test]
fn anthropic_tool_use_serializes_to_openai_tool_call() {
    let block = ContentBlock::ToolUse {
        id: "toolu_1".to_string(),
        name: "search".to_string(),
        input: json!({"query": "rust"}),
    };

    let tool_call = block
        .to_openai_tool_call()
        .expect("tool call")
        .expect("some tool call");

    assert_eq!(
        tool_call.get("id").and_then(serde_json::Value::as_str),
        Some("toolu_1")
    );
    assert_eq!(
        tool_call
            .get("function")
            .and_then(|function| function.get("arguments"))
            .and_then(serde_json::Value::as_str),
        Some("{\"query\":\"rust\"}")
    );
}

#[test]
fn anthropic_tool_result_multiple_blocks_preserve_json_content() {
    let blocks = anthropic_content_to_blocks(&json!([{
        "type": "tool_result",
        "tool_use_id": "toolu_1",
        "content": [{"type": "text", "text": "first"}, {"text": "second"}]
    }]))
    .expect("content blocks");

    assert_eq!(
        blocks
            .first()
            .and_then(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } => Some((tool_use_id, content)),
                _ => None,
            })
            .map(|(tool_use_id, _)| tool_use_id.as_str()),
        Some("toolu_1")
    );
    assert_eq!(
        blocks[0]
            .to_openai_tool_message()
            .and_then(|message| message.get("content").cloned()),
        blocks.first().and_then(|block| match block {
            ContentBlock::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
    );

    let parsed_content = blocks
        .first()
        .and_then(|block| match block {
            ContentBlock::ToolResult { content, .. } => Some(content),
            _ => None,
        })
        .cloned()
        .expect("json tool_result content");
    assert_eq!(
        parsed_content,
        json!([{"type": "text", "text": "first"}, {"text": "second"}])
    );
}

#[test]
fn anthropic_tool_result_single_text_block_preserves_structured_content() {
    let blocks = anthropic_content_to_blocks(&json!([{
        "type": "tool_result",
        "tool_use_id": "toolu_1",
        "content": [{"type": "text", "text": "first"}]
    }]))
    .expect("content blocks");

    assert_eq!(
        blocks,
        vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_1".to_string(),
            content: json!([{"type": "text", "text": "first"}]),
        }]
    );
}

#[test]
fn openai_function_tools_convert_to_anthropic_tools() {
    let tools = openai_tools_to_anthropic_tools(Some(json!([{
        "type": "function",
        "function": {
            "name": "lookup_weather",
            "description": "Look up weather",
            "parameters": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }
    }])))
    .expect("converted tools")
    .expect("tools");

    assert_eq!(
        tools
            .as_array()
            .and_then(|items| items.first())
            .and_then(|tool| tool.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("lookup_weather")
    );
    assert_eq!(
        tools
            .as_array()
            .and_then(|items| items.first())
            .and_then(|tool| tool.get("input_schema"))
            .and_then(|schema| schema.get("required"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn anthropic_tools_convert_to_openai_function_tools() {
    let tools = anthropic_tools_to_openai_tools(Some(json!([{
        "name": "lookup_weather",
        "description": "Look up weather",
        "input_schema": {
            "type": "object",
            "properties": {"city": {"type": "string"}}
        }
    }])))
    .expect("tools");

    assert_eq!(
        tools
            .as_array()
            .and_then(|items| items.first())
            .and_then(|tool| tool.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("function")
    );
    assert_eq!(
        tools
            .as_array()
            .and_then(|items| items.first())
            .and_then(|tool| tool.get("function"))
            .and_then(|function| function.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("lookup_weather")
    );
}

#[test]
fn tool_choice_converts_between_openai_and_anthropic_shapes() {
    let anthropic_choice = openai_tool_choice_to_anthropic_tool_choice(Some(json!({
        "type": "function",
        "function": {"name": "lookup_weather"}
    })))
    .expect("converted to anthropic")
    .expect("choice");
    assert_eq!(
        anthropic_choice
            .get("type")
            .and_then(serde_json::Value::as_str),
        Some("tool")
    );
    assert_eq!(
        anthropic_choice
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("lookup_weather")
    );

    let openai_choice = anthropic_tool_choice_to_openai_tool_choice(Some(anthropic_choice))
        .expect("converted to openai")
        .expect("choice");
    assert_eq!(
        openai_choice
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("lookup_weather")
    );
}

#[test]
fn anthropic_content_blocks_preserve_thinking_signature() {
    let blocks = anthropic_content_to_blocks(&json!([
        {
            "type": "thinking",
            "thinking": "plan",
            "signature": "real-signature"
        },
        {"type": "text", "text": "answer"}
    ]))
    .expect("content blocks");

    assert_eq!(
        blocks,
        vec![
            ContentBlock::Thinking {
                thinking: "plan".to_string(),
                signature: Some("real-signature".to_string()),
            },
            ContentBlock::Text {
                text: "answer".to_string(),
            },
        ]
    );
}

#[test]
fn content_blocks_serialize_to_anthropic_blocks() {
    let blocks = vec![
        ContentBlock::Thinking {
            thinking: "plan".to_string(),
            signature: Some("real-signature".to_string()),
        },
        ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "search".to_string(),
            input: json!({"query": "rust"}),
        },
    ];

    let values = content_blocks_to_anthropic(&blocks);
    assert_eq!(
        values[0].get("type").and_then(serde_json::Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        values[0]
            .get("signature")
            .and_then(serde_json::Value::as_str),
        Some("real-signature")
    );
    assert_eq!(
        values[1].get("type").and_then(serde_json::Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        values[1]
            .get("input")
            .and_then(|input| input.get("query"))
            .and_then(serde_json::Value::as_str),
        Some("rust")
    );
}

#[test]
fn content_blocks_reject_placeholder_signature_for_anthropic_request() {
    let blocks = vec![ContentBlock::Thinking {
        thinking: "plan".to_string(),
        signature: Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE.to_string()),
    }];

    let error = content_blocks_to_anthropic_request(&blocks).expect_err("placeholder rejected");
    assert!(matches!(
        error,
        crate::error::GatewayError::InvalidRequest(_)
    ));
}

#[test]
fn anthropic_request_messages_reject_placeholder_signature() {
    let error = validate_anthropic_request_messages(&json!([{
        "role": "assistant",
        "content": [{
            "type": "thinking",
            "thinking": "plan",
            "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE
        }]
    }]))
    .expect_err("placeholder rejected");

    assert!(matches!(
        error,
        crate::error::GatewayError::InvalidRequest(_)
    ));
}

#[test]
fn openai_messages_convert_to_anthropic_messages() {
    let (system, messages) = openai_messages_to_anthropic_messages(
        &json!([
            {"role": "system", "content": "be precise"},
            {"role": "user", "content": "find rust examples"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust examples\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "result text"
            }
        ]),
        None,
    )
    .expect("anthropic messages");

    assert_eq!(system, Some(Value::String("be precise".to_string())));
    assert_eq!(
        messages
            .pointer("/0/content/0/text")
            .and_then(Value::as_str),
        Some("find rust examples")
    );
    assert_eq!(
        messages
            .pointer("/1/content/0/type")
            .and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        messages
            .pointer("/2/content/0/type")
            .and_then(Value::as_str),
        Some("tool_result")
    );
}

#[test]
fn openai_system_text_blocks_preserve_anthropic_block_array() {
    let (system, messages) = openai_messages_to_anthropic_messages(
        &json!([
            {
                "role": "system",
                "content": [
                    {"type": "text", "text": "be precise"},
                    {"type": "input_text", "text": "cite sources"}
                ]
            },
            {"role": "user", "content": "find rust examples"}
        ]),
        None,
    )
    .expect("anthropic messages");

    assert_eq!(
        system,
        Some(json!([
            {"type": "text", "text": "be precise"},
            {"type": "text", "text": "cite sources"}
        ]))
    );
    assert_eq!(
        messages
            .pointer("/0/content/0/text")
            .and_then(Value::as_str),
        Some("find rust examples")
    );
}

#[test]
fn openai_multiple_system_messages_preserve_order_without_joining() {
    let (system, _) = openai_messages_to_anthropic_messages(
        &json!([
            {"role": "system", "content": "be precise"},
            {"role": "system", "content": "cite sources"},
            {"role": "user", "content": "find rust examples"}
        ]),
        None,
    )
    .expect("anthropic messages");

    assert_eq!(
        system,
        Some(json!([
            {"type": "text", "text": "be precise"},
            {"type": "text", "text": "cite sources"}
        ]))
    );
}

#[test]
fn anthropic_messages_convert_to_openai_messages() {
    let messages = anthropic_messages_to_openai_messages(&json!([
        {
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "plan"},
                {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"query": "rust"}},
                {"type": "text", "text": "done"}
            ]
        },
        {
            "role": "user",
            "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "result text"}
            ]
        }
    ]))
    .expect("openai messages");

    assert_eq!(
        messages[0].get("thinking").and_then(Value::as_str),
        Some("plan")
    );
    assert_eq!(
        messages[0]
            .get("tool_calls")
            .and_then(|tool_calls| tool_calls.as_array())
            .and_then(|items| items.first())
            .and_then(|tool| tool.get("id"))
            .and_then(Value::as_str),
        Some("toolu_1")
    );
    assert_eq!(
        messages[1].get("role").and_then(Value::as_str),
        Some("tool")
    );
}

#[test]
fn openai_message_downstream_blocks_include_thinking_and_tool_use() {
    let blocks = openai_message_to_anthropic_content_blocks(&json!({
        "role": "assistant",
        "reasoning_content": "need weather first",
        "content": "I'll call a tool",
        "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {
                "name": "lookup_weather",
                "arguments": "{\"city\":\"Paris\"}"
            }
        }]
    }));

    assert_eq!(
        blocks[0].get("type").and_then(Value::as_str),
        Some("thinking")
    );
    assert_eq!(
        blocks[0].get("signature").and_then(Value::as_str),
        Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE)
    );
    assert_eq!(
        blocks[1].get("text").and_then(Value::as_str),
        Some("I'll call a tool")
    );
    assert_eq!(
        blocks[2].get("type").and_then(Value::as_str),
        Some("tool_use")
    );
    assert_eq!(
        blocks[2]
            .get("input")
            .and_then(|input| input.get("city"))
            .and_then(Value::as_str),
        Some("Paris")
    );
}

#[test]
fn openai_message_downstream_blocks_preserve_image_url_items() {
    let blocks = openai_message_to_anthropic_content_blocks(&json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "hello"},
            {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
        ]
    }));

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].get("text").and_then(Value::as_str), Some("hello"));
    assert_eq!(blocks[1].get("type").and_then(Value::as_str), Some("image"));
    assert_eq!(
        blocks[1].pointer("/source/type").and_then(Value::as_str),
        Some("url")
    );
    assert_eq!(
        blocks[1].pointer("/source/url").and_then(Value::as_str),
        Some("https://example.com/a.png")
    );
}

#[test]
fn openai_message_downstream_blocks_preserve_input_image_data_urls() {
    let blocks = openai_message_to_anthropic_content_blocks(&json!({
        "role": "user",
        "content": [{
            "type": "input_image",
            "image_url": "data:image/png;base64,ZmFrZQ=="
        }]
    }));

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].get("type").and_then(Value::as_str), Some("image"));
    assert_eq!(
        blocks[0].pointer("/source/type").and_then(Value::as_str),
        Some("base64")
    );
    assert_eq!(
        blocks[0]
            .pointer("/source/media_type")
            .and_then(Value::as_str),
        Some("image/png")
    );
    assert_eq!(
        blocks[0].pointer("/source/data").and_then(Value::as_str),
        Some("ZmFrZQ==")
    );
}

#[test]
fn openai_messages_to_anthropic_messages_preserve_image_content_blocks() {
    let (system, messages) = openai_messages_to_anthropic_messages(
        &json!([
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "describe this"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
                ]
            }
        ]),
        None,
    )
    .expect("anthropic messages");

    assert_eq!(system, None);
    assert_eq!(
        messages
            .pointer("/0/content/0/text")
            .and_then(Value::as_str),
        Some("describe this")
    );
    assert_eq!(
        messages
            .pointer("/0/content/1/type")
            .and_then(Value::as_str),
        Some("image")
    );
    assert_eq!(
        messages
            .pointer("/0/content/1/source/url")
            .and_then(Value::as_str),
        Some("https://example.com/a.png")
    );
}

#[test]
fn tool_call_delta_update_emits_start_and_delta() {
    let mut pending_tool_calls = BTreeMap::new();
    let mut next_content_index = 2;

    let update = apply_openai_tool_call_delta_update(
        &mut pending_tool_calls,
        &mut next_content_index,
        0,
        &json!({
            "id": "call_1",
            "function": {
                "name": "lookup_weather",
                "arguments": "{\"city\":\"Paris\"}"
            }
        }),
    );

    assert_eq!(next_content_index, 3);
    assert_eq!(
        update.start,
        Some(AnthropicToolUseStart {
            anthropic_index: 2,
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
        })
    );
    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "{\"city\":\"Paris\"}".to_string(),
        })
    );
    assert_eq!(
        pending_tool_calls.get(&0),
        Some(&PendingOpenAiToolCall {
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
            raw_arguments: "{\"city\":\"Paris\"}".to_string(),
            arguments: "{\"city\":\"Paris\"}".to_string(),
            emitted_argument_len: 16,
            anthropic_index: Some(2),
            started: true,
            stopped: false,
        })
    );
}

#[test]
fn tool_call_delta_update_only_emits_new_argument_fragment() {
    let mut pending_tool_calls = BTreeMap::from([(
        0,
        PendingOpenAiToolCall {
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
            raw_arguments: "{\"city\":\"".to_string(),
            arguments: "{\"city\":\"".to_string(),
            emitted_argument_len: 9,
            anthropic_index: Some(2),
            started: true,
            stopped: false,
        },
    )]);
    let mut next_content_index = 3;

    let update = apply_openai_tool_call_delta_update(
        &mut pending_tool_calls,
        &mut next_content_index,
        0,
        &json!({
            "function": {
                "arguments": "Paris\"}"
            }
        }),
    );

    assert_eq!(next_content_index, 3);
    assert_eq!(update.start, None);
    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "Paris\"}".to_string(),
        })
    );
}

#[test]
fn tool_call_stop_update_emits_placeholder_start_and_buffered_delta() {
    let mut pending_tool_calls = BTreeMap::from([(
        0,
        PendingOpenAiToolCall {
            id: String::new(),
            name: String::new(),
            raw_arguments: "{\"city\":\"Paris\"}".to_string(),
            arguments: "{\"city\":\"Paris\"}".to_string(),
            emitted_argument_len: 0,
            anthropic_index: Some(2),
            started: false,
            stopped: false,
        },
    )]);

    let update = flush_openai_tool_call_stop_update(&mut pending_tool_calls, 0);

    assert_eq!(
        update.start,
        Some(AnthropicToolUseStart {
            anthropic_index: 2,
            id: "toolu_unknown".to_string(),
            name: "tool".to_string(),
        })
    );
    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "{\"city\":\"Paris\"}".to_string(),
        })
    );
    assert_eq!(update.stop_index, Some(2));
    assert_eq!(
        pending_tool_calls.get(&0),
        Some(&PendingOpenAiToolCall {
            id: String::new(),
            name: String::new(),
            raw_arguments: "{\"city\":\"Paris\"}".to_string(),
            arguments: "{\"city\":\"Paris\"}".to_string(),
            emitted_argument_len: 16,
            anthropic_index: Some(2),
            started: true,
            stopped: true,
        })
    );
}

#[test]
fn tool_call_stop_update_emits_stop_only_for_started_call() {
    let mut pending_tool_calls = BTreeMap::from([(
        0,
        PendingOpenAiToolCall {
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
            raw_arguments: "{\"city\":\"Paris\"}".to_string(),
            arguments: "{\"city\":\"Paris\"}".to_string(),
            emitted_argument_len: 16,
            anthropic_index: Some(2),
            started: true,
            stopped: false,
        },
    )]);

    let update = flush_openai_tool_call_stop_update(&mut pending_tool_calls, 0);

    assert_eq!(update.start, None);
    assert_eq!(update.delta, None);
    assert_eq!(update.stop_index, Some(2));
    assert_eq!(
        pending_tool_calls.get(&0).map(|pending| pending.stopped),
        Some(true)
    );
}

#[test]
fn tool_call_delta_update_deduplicates_cumulative_argument_snapshots() {
    let mut pending_tool_calls = BTreeMap::from([(
        0,
        PendingOpenAiToolCall {
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
            raw_arguments: "{\"city\":\"".to_string(),
            arguments: "{\"city\":\"".to_string(),
            emitted_argument_len: 9,
            anthropic_index: Some(2),
            started: true,
            stopped: false,
        },
    )]);
    let mut next_content_index = 3;

    let update = apply_openai_tool_call_delta_update(
        &mut pending_tool_calls,
        &mut next_content_index,
        0,
        &json!({
            "function": {
                "arguments": "{\"city\":\"Paris\"}"
            }
        }),
    );

    assert_eq!(update.start, None);
    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "Paris\"}".to_string(),
        })
    );
    assert_eq!(
        pending_tool_calls
            .get(&0)
            .map(|pending| pending.arguments.as_str()),
        Some("{\"city\":\"Paris\"}")
    );
}

#[test]
fn tool_call_delta_update_normalizes_double_encoded_json_string() {
    let mut pending_tool_calls = BTreeMap::new();
    let mut next_content_index = 2;

    let update = apply_openai_tool_call_delta_update(
        &mut pending_tool_calls,
        &mut next_content_index,
        0,
        &json!({
            "id": "call_1",
            "function": {
                "name": "lookup_weather",
                "arguments": "\"{\\\"city\\\":\\\"Paris\\\"}\""
            }
        }),
    );

    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "{\"city\":\"Paris\"}".to_string(),
        })
    );
    assert_eq!(
        pending_tool_calls
            .get(&0)
            .map(|pending| pending.arguments.as_str()),
        Some("{\"city\":\"Paris\"}")
    );
}

#[test]
fn tool_call_delta_update_strips_empty_object_prefix() {
    let mut pending_tool_calls = BTreeMap::new();
    let mut next_content_index = 2;

    let update = apply_openai_tool_call_delta_update(
        &mut pending_tool_calls,
        &mut next_content_index,
        0,
        &json!({
            "id": "call_1",
            "function": {
                "name": "lookup_weather",
                "arguments": "{}{\"city\":\"Paris\"}"
            }
        }),
    );

    assert_eq!(
        update.delta,
        Some(AnthropicInputJsonDelta {
            anthropic_index: 2,
            partial_json: "{\"city\":\"Paris\"}".to_string(),
        })
    );
    assert_eq!(
        pending_tool_calls
            .get(&0)
            .map(|pending| pending.arguments.as_str()),
        Some("{\"city\":\"Paris\"}")
    );
}
