mod aggregator;
mod anthropic_stream;
mod openai_chat;
mod reasoning_text;
mod render;

#[cfg(test)]
mod tests;

pub use aggregator::AnthropicStreamAggregator;
pub use openai_chat::{OpenAiChatStreamAdapter, openai_sse_chunks_from_chat_chunk};
pub use reasoning_text::{
    ANTHROPIC_REASONING_TEXT_FORMAT_KEY, ANTHROPIC_REASONING_TEXT_FORMAT_XML_THINK_TAG,
    REASONING_TEXT_ENCODING_KEY, REASONING_TEXT_ENCODING_XML_THINK_TAG,
};
pub use render::{
    anthropic_completed_chat_body, openai_completed_chat_body, render_anthropic_chat_session,
    render_openai_chat_session, render_openai_embeddings_response, render_openai_responses_session,
    render_openai_responses_stream_from_completed,
};
