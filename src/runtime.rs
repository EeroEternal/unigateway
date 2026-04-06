mod app_host;
pub(crate) mod context;
mod errors;

#[path = "gateway/core_adapter.rs"]
mod adapter;
#[path = "gateway/core_bridge.rs"]
mod bridge;
#[path = "gateway/chat.rs"]
mod chat;
#[path = "gateway/legacy_runtime.rs"]
mod legacy_runtime;
#[path = "gateway/responses_compat.rs"]
mod responses_compat;
#[path = "gateway/streaming.rs"]
mod streaming;

pub(crate) use bridge::{
    try_anthropic_chat_via_core, try_openai_chat_via_core, try_openai_chat_via_env_core,
    try_openai_embeddings_via_core, try_openai_responses_via_core,
    try_openai_responses_via_env_core,
};
pub(crate) use context::RuntimeContext;
pub(crate) use errors::{status_for_core_error, status_for_legacy_error};
pub(crate) use legacy_runtime::{
    invoke_anthropic_chat_via_env_legacy, invoke_anthropic_chat_via_legacy,
    invoke_openai_chat_via_env_legacy, invoke_openai_chat_via_legacy,
    invoke_openai_embeddings_via_env_legacy, invoke_openai_embeddings_via_legacy,
    invoke_openai_responses_via_env_legacy, invoke_openai_responses_via_legacy,
};
