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

    normalized.insert("thinking".to_string(), Value::String(thinking));
    normalized.insert("content".to_string(), Value::String(text));
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

        if open_tag.starts_with(combined.as_str()) {
            self.state = StreamParseState::DetectingOpen { buffer: combined };
            return Vec::new();
        }

        if let Some(after_open) = combined.strip_prefix(open_tag) {
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
    let remainder = content.strip_prefix(open_tag)?;
    let close_index = remainder.find(close_tag)?;

    Some((
        remainder[..close_index].to_string(),
        remainder[(close_index + close_tag.len())..].to_string(),
    ))
}
