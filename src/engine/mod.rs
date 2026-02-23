use anyhow::Result;
use llm_connector::LlmClient;

#[derive(Clone)]
pub struct Engine {
    pub client: LlmClient,
}

impl Engine {
    pub fn new() -> Result<Self> {
        // Initialize a generic client. 
        // We use "openai" as the base protocol, but it's just a placeholder.
        // The actual API key and Base URL will be overridden per request.
        let client = LlmClient::openai("placeholder-key")?;
        Ok(Self { client })
    }
}
