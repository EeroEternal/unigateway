use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use unigateway_core::{Message, MessageRole, ProxyChatRequest};
use unigateway_host::{ChatRequestMiddleware, HostContext, HostFuture, HostResult};

use crate::SESSION_GATEWAY_FIELD;
use crate::store::{MemorySessionStore, SessionStoreError};

/// Delivery mode carried in gateway session context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionDelivery {
    #[default]
    Full,
    Delta,
}

/// Parsed session hints from `gateway_fields["_session_context"]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionGatewayContext {
    pub session_id: String,
    pub epoch: u64,
    #[serde(default)]
    pub delivery: SessionDelivery,
    #[serde(default)]
    pub prefix_hash: Option<String>,
}

impl SessionGatewayContext {
    pub fn from_gateway_fields(fields: &HashMap<String, Value>) -> Result<Option<Self>> {
        let Some(raw) = fields.get(SESSION_GATEWAY_FIELD) else {
            return Ok(None);
        };
        serde_json::from_value(raw.clone())
            .map(Some)
            .with_context(|| format!("invalid {SESSION_GATEWAY_FIELD} payload"))
    }
}

/// Assembles `stored_prefix || tail` when `delivery=delta`.
pub struct DeltaAssemblyMiddleware {
    store: Arc<MemorySessionStore>,
}

impl DeltaAssemblyMiddleware {
    pub fn new(store: Arc<MemorySessionStore>) -> Self {
        Self { store }
    }

    fn assemble_delta(
        &self,
        ctx: &SessionGatewayContext,
        request: &mut ProxyChatRequest,
    ) -> Result<()> {
        let Some(stored) = self.store.get(&ctx.session_id)? else {
            return Err(anyhow!("session not found: {}", ctx.session_id));
        };
        if stored.epoch != ctx.epoch {
            return Err(anyhow!(
                "epoch mismatch for session {}: expected {}, got {}",
                ctx.session_id,
                stored.epoch,
                ctx.epoch
            ));
        }

        let mut merged = stored.messages.clone();
        merged.extend(raw_tail_messages(request)?);
        request.messages = merged
            .into_iter()
            .filter_map(|value| openai_value_to_message(&value))
            .collect();
        request.raw_messages = Some(Value::Array(
            request
                .messages
                .iter()
                .map(message_to_openai_value)
                .collect(),
        ));
        request.mark_openai_raw_messages();
        Ok(())
    }
}

impl ChatRequestMiddleware for DeltaAssemblyMiddleware {
    fn on_chat_request<'a>(
        &'a self,
        _host: &'a HostContext<'_>,
        request: &'a mut ProxyChatRequest,
        gateway_fields: &'a HashMap<String, Value>,
    ) -> HostFuture<'a, HostResult<()>> {
        Box::pin(async move {
            let Some(ctx) =
                SessionGatewayContext::from_gateway_fields(gateway_fields).map_err(|error| {
                    unigateway_host::HostError::CoreInvalidRequest(error.to_string())
                })?
            else {
                return Ok(());
            };

            if ctx.delivery == SessionDelivery::Full {
                return Ok(());
            }

            self.assemble_delta(&ctx, request)
                .map_err(|error| unigateway_host::HostError::CoreInvalidRequest(error.to_string()))
        })
    }
}

fn raw_tail_messages(request: &ProxyChatRequest) -> Result<Vec<Value>> {
    if let Some(raw) = request.raw_messages.as_ref().and_then(Value::as_array) {
        return Ok(raw.clone());
    }
    Ok(request
        .messages
        .iter()
        .map(message_to_openai_value)
        .collect())
}

fn openai_value_to_message(value: &Value) -> Option<Message> {
    let role = match value.get("role")?.as_str()? {
        "system" => MessageRole::System,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    };
    let content = value.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(_) => return None,
        _ => return None,
    };
    Some(Message::text(role, text))
}

fn message_to_openai_value(message: &Message) -> Value {
    serde_json::json!({
        "role": match message.role {
            MessageRole::System => "system",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
            MessageRole::User => "user",
        },
        "content": message.text_content(),
    })
}

impl From<SessionStoreError> for unigateway_host::HostError {
    fn from(error: SessionStoreError) -> Self {
        Self::CoreInvalidRequest(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::json;
    use unigateway_core::{Message, MessageRole, ProxyChatRequest, UniGatewayEngine};
    use unigateway_host::{
        ChatRequestMiddleware, HostContext, PoolHost, PoolLookupOutcome, PoolLookupResult,
    };

    use super::{DeltaAssemblyMiddleware, SessionDelivery, SessionGatewayContext};
    use crate::SESSION_GATEWAY_FIELD;
    use crate::store::{MemorySessionStore, SessionPrefix};

    #[test]
    fn parses_session_gateway_context() {
        let fields = HashMap::from([(
            SESSION_GATEWAY_FIELD.to_string(),
            json!({"session_id":"s1","epoch":2,"delivery":"delta"}),
        )]);
        let ctx = SessionGatewayContext::from_gateway_fields(&fields)
            .expect("parse")
            .expect("ctx");
        assert_eq!(ctx.session_id, "s1");
        assert_eq!(ctx.epoch, 2);
        assert_eq!(ctx.delivery, SessionDelivery::Delta);
    }

    #[tokio::test]
    async fn delta_middleware_prepends_stored_prefix() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::new(store.clone());

        let engine = UniGatewayEngine::builder()
            .with_builtin_http_drivers()
            .build()
            .expect("engine");

        struct EmptyPoolHost;
        impl PoolHost for EmptyPoolHost {
            fn pool_for_service<'a>(
                &'a self,
                _service_id: &'a str,
            ) -> unigateway_host::HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
                Box::pin(
                    async move { Err(unigateway_host::PoolLookupError::unavailable("unused")) },
                )
            }
        }

        let context = HostContext::from_parts(&engine, &EmptyPoolHost);

        let mut request = ProxyChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![Message::text(MessageRole::User, "tail")],
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            gateway_fields: HashMap::from([(
                SESSION_GATEWAY_FIELD.to_string(),
                json!({"session_id":"s1","epoch":1,"delivery":"delta"}),
            )]),
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };

        let gateway_fields = request.gateway_fields.clone();
        middleware
            .on_chat_request(&context, &mut request, &gateway_fields)
            .await
            .expect("mw");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].text_content(), "prefix");
        assert_eq!(request.messages[1].text_content(), "tail");
    }
}
