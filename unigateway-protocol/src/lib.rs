mod http_response;
mod requests;
mod responses;

pub use http_response::{ProtocolByteStream, ProtocolHttpResponse, ProtocolResponseBody};
pub use requests::{
    ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY, OPENAI_RAW_MESSAGES_KEY,
    anthropic_payload_to_chat_request, anthropic_requested_model_alias,
    openai_payload_to_chat_request, openai_payload_to_embed_request,
    openai_payload_to_responses_request,
};
pub use responses::{
    render_anthropic_chat_session, render_openai_chat_session, render_openai_embeddings_response,
    render_openai_responses_session, render_openai_responses_stream_from_completed,
};

pub mod testing {
    pub use crate::responses::{
        OpenAiChatStreamAdapter, anthropic_completed_chat_body, openai_completed_chat_body,
        openai_sse_chunks_from_chat_chunk,
    };
}
