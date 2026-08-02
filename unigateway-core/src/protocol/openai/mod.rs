#[cfg(feature = "drivers")]
mod driver;
mod parsing;
mod requests;
#[cfg(feature = "drivers")]
mod streaming;

#[cfg(all(test, feature = "drivers"))]
mod responses_tool_loop_tests;
#[cfg(all(test, feature = "drivers"))]
mod tests;

#[cfg(feature = "drivers")]
pub use driver::OpenAiCompatibleDriver;
pub use parsing::{parse_chat_response, parse_embeddings_response, parse_responses_response};
pub use requests::{build_chat_request, build_embeddings_request, build_responses_request};

pub const DRIVER_ID: &str = "openai-compatible";
