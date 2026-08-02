#[cfg(feature = "drivers")]
mod driver;
mod parsing;
mod requests;
#[cfg(feature = "drivers")]
mod streaming;

#[cfg(all(test, feature = "drivers"))]
mod tests;

#[cfg(feature = "drivers")]
pub use driver::AnthropicDriver;
pub use parsing::parse_chat_response;
pub use requests::build_chat_request;

pub const DRIVER_ID: &str = "anthropic";
