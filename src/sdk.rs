use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct UniGatewayClient {
    base_url: String,
    api_key: Option<String>,
    http: Client,
}

impl UniGatewayClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            http: Client::new(),
        }
    }

    pub async fn openai_chat(&self, payload: &Value) -> Result<Value> {
        let mut req = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(payload);

        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let value = req
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(value)
    }

    pub async fn anthropic_messages(&self, payload: &Value) -> Result<Value> {
        let mut req = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("anthropic-version", "2023-06-01")
            .json(payload);

        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }

        let value = req
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(value)
    }
}
