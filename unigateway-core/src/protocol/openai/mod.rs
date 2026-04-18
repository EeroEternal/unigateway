mod driver;
mod parsing;
mod requests;
mod streaming;

#[cfg(test)]
mod tests;

pub use driver::OpenAiCompatibleDriver;
pub use parsing::{parse_chat_response, parse_embeddings_response, parse_responses_response};
pub use requests::{build_chat_request, build_embeddings_request, build_responses_request};

pub const DRIVER_ID: &str = "openai-compatible";
