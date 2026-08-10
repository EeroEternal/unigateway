use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use unigateway_core::{ChatResponseChunk, ChatResponseFinal, ProxyChatRequest, ProxySession};

use crate::error::HostResult;
use crate::host::{HostContext, HostFuture};

/// Mutates a chat request after protocol parse and before core dispatch.
pub trait ChatRequestMiddleware: Send + Sync {
    /// Runs after parse (and optional session assembly), before `proxy_chat`.
    fn on_chat_request<'a>(
        &'a self,
        ctx: &'a HostContext<'_>,
        request: &'a mut ProxyChatRequest,
        gateway_fields: &'a HashMap<String, Value>,
    ) -> HostFuture<'a, HostResult<()>>;
}

/// Observes or mutates a core session before protocol render.
pub trait ChatResponseMiddleware: Send + Sync {
    /// Runs after `proxy_chat`, before client-facing render.
    fn on_chat_response<'a>(
        &'a self,
        ctx: &'a HostContext<'_>,
        request: &'a ProxyChatRequest,
        session: &'a mut ProxySession<ChatResponseChunk, ChatResponseFinal>,
    ) -> HostFuture<'a, HostResult<()>>;
}

/// Opt-in middleware chain; default is empty (no behavior change).
#[derive(Default, Clone)]
pub struct HostMiddleware {
    request: Vec<Arc<dyn ChatRequestMiddleware>>,
    response: Vec<Arc<dyn ChatResponseMiddleware>>,
}

impl HostMiddleware {
    /// Creates an empty middleware chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether any middleware is registered.
    pub fn is_empty(&self) -> bool {
        self.request.is_empty() && self.response.is_empty()
    }

    /// Registers a request middleware handler.
    pub fn with_request(mut self, middleware: Arc<dyn ChatRequestMiddleware>) -> Self {
        self.request.push(middleware);
        self
    }

    /// Registers a response middleware handler.
    pub fn with_response(mut self, middleware: Arc<dyn ChatResponseMiddleware>) -> Self {
        self.response.push(middleware);
        self
    }

    pub(super) async fn run_request(
        &self,
        ctx: &HostContext<'_>,
        request: &mut ProxyChatRequest,
    ) -> HostResult<()> {
        if self.request.is_empty() {
            return Ok(());
        }

        let gateway_fields = request.gateway_fields.clone();
        for middleware in &self.request {
            middleware
                .on_chat_request(ctx, request, &gateway_fields)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn run_response(
        &self,
        ctx: &HostContext<'_>,
        request: &ProxyChatRequest,
        session: &mut ProxySession<ChatResponseChunk, ChatResponseFinal>,
    ) -> HostResult<()> {
        for middleware in &self.response {
            middleware.on_chat_response(ctx, request, session).await?;
        }
        Ok(())
    }
}
