use std::collections::HashMap;

use serde_json::Value;

/// Metadata key that declares a text encoding used for reasoning-like content.
pub const REASONING_TEXT_ENCODING_KEY: &str = "unigateway.reasoning_text_encoding";
/// Built-in encoding value for prefixed `<think>...</think>` text content.
pub const REASONING_TEXT_ENCODING_XML_THINK_TAG: &str = "xml_think_tag";
/// Legacy compatibility alias for the old Anthropic-oriented metadata key.
pub const ANTHROPIC_REASONING_TEXT_FORMAT_KEY: &str = "unigateway.anthropic_reasoning_text_format";
/// Legacy compatibility alias for the old Anthropic-oriented metadata value.
pub const ANTHROPIC_REASONING_TEXT_FORMAT_XML_THINK_TAG: &str =
    REASONING_TEXT_ENCODING_XML_THINK_TAG;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReasoningTextEncoding {
    XmlThinkTag,
}

pub(crate) fn reasoning_text_encoding(
    metadata: &HashMap<String, String>,
) -> Option<ReasoningTextEncoding> {
    match metadata
        .get(REASONING_TEXT_ENCODING_KEY)
        .or_else(|| metadata.get(ANTHROPIC_REASONING_TEXT_FORMAT_KEY))
        .map(String::as_str)
    {
        Some(REASONING_TEXT_ENCODING_XML_THINK_TAG) => Some(ReasoningTextEncoding::XmlThinkTag),
        _ => None,
    }
}

pub(crate) fn normalize_openai_message_reasoning_text(
    message: &Value,
    encoding: Option<ReasoningTextEncoding>,
) -> Value {
    let Some(encoding) = encoding else {
        return message.clone();
    };

    if message.get("reasoning_content").is_some() || message.get("thinking").is_some() {
        return message.clone();
    }

    let Some(content) = message.get("content").and_then(Value::as_str) else {
        return message.clone();
    };
    let Some((thinking, text)) = split_reasoning_text(content, encoding) else {
        return message.clone();
    };
    let Some(mut normalized) = message.as_object().cloned() else {
        return message.clone();
    };

    normalized.insert(
        "reasoning_content".to_string(),
        Value::String(thinking.clone()),
    );
    normalized.insert("thinking".to_string(), Value::String(thinking));
    normalized.insert("content".to_string(), Value::String(text));
    Value::Object(normalized)
}

pub(crate) fn normalize_openai_chat_completion_reasoning_text(
    body: &Value,
    encoding: Option<ReasoningTextEncoding>,
) -> Value {
    let Some(encoding) = encoding else {
        return body.clone();
    };
    let Some(mut normalized) = body.as_object().cloned() else {
        return body.clone();
    };
    let Some(choices) = normalized.get("choices").and_then(Value::as_array).cloned() else {
        return body.clone();
    };

    let normalized_choices = choices
        .into_iter()
        .map(|choice| {
            let Some(mut normalized_choice) = choice.as_object().cloned() else {
                return choice;
            };
            let Some(message) = normalized_choice.get("message") else {
                return Value::Object(normalized_choice);
            };

            normalized_choice.insert(
                "message".to_string(),
                normalize_openai_message_reasoning_text(message, Some(encoding)),
            );
            Value::Object(normalized_choice)
        })
        .collect();

    normalized.insert("choices".to_string(), Value::Array(normalized_choices));
    Value::Object(normalized)
}

pub(crate) fn split_reasoning_text(
    content: &str,
    encoding: ReasoningTextEncoding,
) -> Option<(String, String)> {
    match encoding {
        ReasoningTextEncoding::XmlThinkTag => {
            split_prefixed_xml_tag(content, "<think>", "</think>")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReasoningTextChunk {
    Thinking(String),
    Text(String),
}

pub(crate) struct ReasoningTextStreamParser {
    state: StreamParseState,
}

enum StreamParseState {
    DetectingOpen { buffer: String },
    CollectingThinking { buffer: String },
    Passthrough,
}

impl Default for StreamParseState {
    fn default() -> Self {
        Self::DetectingOpen {
            buffer: String::new(),
        }
    }
}

impl ReasoningTextStreamParser {
    pub(crate) fn new(encoding: ReasoningTextEncoding) -> Self {
        debug_assert_eq!(encoding, ReasoningTextEncoding::XmlThinkTag);
        Self {
            state: StreamParseState::default(),
        }
    }

    pub(crate) fn push(&mut self, text: &str) -> Vec<ReasoningTextChunk> {
        self.push_xml_think_tag(text)
    }

    pub(crate) fn finish(&mut self) -> Vec<ReasoningTextChunk> {
        self.finish_xml_think_tag()
    }

    fn push_xml_think_tag(&mut self, text: &str) -> Vec<ReasoningTextChunk> {
        match self.state {
            StreamParseState::DetectingOpen { .. } => self.push_detecting_open(text),
            StreamParseState::CollectingThinking { .. } => self.push_collecting_thinking(text),
            StreamParseState::Passthrough => vec![ReasoningTextChunk::Text(text.to_string())],
        }
    }

    fn finish_xml_think_tag(&mut self) -> Vec<ReasoningTextChunk> {
        match std::mem::take(&mut self.state) {
            StreamParseState::DetectingOpen { buffer } => {
                if buffer.is_empty() {
                    Vec::new()
                } else {
                    vec![ReasoningTextChunk::Text(buffer)]
                }
            }
            StreamParseState::CollectingThinking { buffer } => {
                vec![ReasoningTextChunk::Text(format!("<think>{buffer}"))]
            }
            StreamParseState::Passthrough => Vec::new(),
        }
    }

    fn push_detecting_open(&mut self, text: &str) -> Vec<ReasoningTextChunk> {
        let open_tag = "<think>";
        let combined = match std::mem::take(&mut self.state) {
            StreamParseState::DetectingOpen { mut buffer } => {
                buffer.push_str(text);
                buffer
            }
            _ => unreachable!(),
        };

        let trimmed = combined.trim_start();

        if open_tag.starts_with(trimmed) {
            self.state = StreamParseState::DetectingOpen { buffer: combined };
            return Vec::new();
        }

        if let Some(after_open) = trimmed.strip_prefix(open_tag) {
            self.state = StreamParseState::CollectingThinking {
                buffer: String::new(),
            };
            return self.push_collecting_thinking(after_open);
        }

        self.state = StreamParseState::Passthrough;
        vec![ReasoningTextChunk::Text(combined)]
    }

    fn push_collecting_thinking(&mut self, text: &str) -> Vec<ReasoningTextChunk> {
        let close_tag = "</think>";
        let combined = match std::mem::take(&mut self.state) {
            StreamParseState::CollectingThinking { mut buffer } => {
                buffer.push_str(text);
                buffer
            }
            _ => unreachable!(),
        };

        if let Some(close_index) = combined.find(close_tag) {
            let thinking = combined[..close_index].to_string();
            let remainder = combined[(close_index + close_tag.len())..].to_string();
            self.state = StreamParseState::Passthrough;

            let mut chunks = Vec::new();
            if !thinking.is_empty() {
                chunks.push(ReasoningTextChunk::Thinking(thinking));
            }
            if !remainder.is_empty() {
                chunks.push(ReasoningTextChunk::Text(remainder));
            }
            return chunks;
        }

        self.state = StreamParseState::CollectingThinking { buffer: combined };
        Vec::new()
    }
}

fn split_prefixed_xml_tag(
    content: &str,
    open_tag: &str,
    close_tag: &str,
) -> Option<(String, String)> {
    let remainder = content.trim_start().strip_prefix(open_tag)?;
    let close_index = remainder.find(close_tag)?;

    Some((
        remainder[..close_index].to_string(),
        remainder[(close_index + close_tag.len())..].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefixed_xml_tag_extracts_reasoning() {
        let (reasoning, remainder) = split_prefixed_xml_tag(
            "<think>some reasoning</think>and then text",
            "<think>",
            "</think>",
        )
        .unwrap();
        assert_eq!(reasoning, "some reasoning");
        assert_eq!(remainder, "and then text");
    }

    #[test]
    fn split_prefixed_xml_tag_ignores_non_prefixed_text() {
        assert_eq!(
            split_prefixed_xml_tag("no think tag", "<think>", "</think>"),
            None
        );
        assert_eq!(
            split_prefixed_xml_tag(" <not_think>foo", "<think>", "</think>"),
            None
        );
    }

    #[test]
    fn stream_parser_handles_split_open_tag() {
        let mut parser = ReasoningTextStreamParser::new(ReasoningTextEncoding::XmlThinkTag);
        let mut chunks = Vec::new();
        chunks.extend(parser.push("<thi"));
        chunks.extend(parser.push("nk>"));
        chunks.extend(parser.push("reasoning"));
        chunks.extend(parser.push("</think>text"));
        chunks.extend(parser.finish());

        assert_eq!(
            chunks,
            vec![
                ReasoningTextChunk::Thinking("reasoning".to_string()),
                ReasoningTextChunk::Text("text".to_string())
            ]
        );
    }

    #[test]
    fn stream_parser_handles_split_close_tag() {
        let mut parser = ReasoningTextStreamParser::new(ReasoningTextEncoding::XmlThinkTag);
        let mut chunks = Vec::new();
        chunks.extend(parser.push("<think>reasoning</th"));
        chunks.extend(parser.push("ink>text"));
        chunks.extend(parser.finish());

        assert_eq!(
            chunks,
            vec![
                ReasoningTextChunk::Thinking("reasoning".to_string()),
                ReasoningTextChunk::Text("text".to_string())
            ]
        );
    }

    #[test]
    fn stream_parser_flushes_incomplete_open_tag_as_text() {
        let mut parser = ReasoningTextStreamParser::new(ReasoningTextEncoding::XmlThinkTag);
        let mut chunks = Vec::new();
        chunks.extend(parser.push("<thi"));
        chunks.extend(parser.finish());

        assert_eq!(chunks, vec![ReasoningTextChunk::Text("<thi".to_string())]);
    }

    #[test]
    fn stream_parser_flushes_incomplete_thinking_as_text() {
        let mut parser = ReasoningTextStreamParser::new(ReasoningTextEncoding::XmlThinkTag);
        let mut chunks = Vec::new();
        chunks.extend(parser.push("<think>reasoning"));
        chunks.extend(parser.finish());

        assert_eq!(
            chunks,
            vec![ReasoningTextChunk::Text("<think>reasoning".to_string())]
        );
    }

    #[test]
    fn stream_parser_allows_leading_whitespace_before_think_tag() {
        let mut parser = ReasoningTextStreamParser::new(ReasoningTextEncoding::XmlThinkTag);
        let mut chunks = Vec::new();
        chunks.extend(parser.push(" \n  <think>reasoning</think>text"));
        chunks.extend(parser.finish());

        assert_eq!(
            chunks,
            vec![
                ReasoningTextChunk::Thinking("reasoning".to_string()),
                ReasoningTextChunk::Text("text".to_string())
            ]
        );

        let (reasoning, remainder) =
            split_prefixed_xml_tag(" \n  <think>reasoning</think>text", "<think>", "</think>")
                .unwrap();
        assert_eq!(reasoning, "reasoning");
        assert_eq!(remainder, "text");
    }
}
