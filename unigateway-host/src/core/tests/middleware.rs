use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use unigateway_core::{Message, MessageRole, ProxyChatRequest};

use super::super::dispatch::{
    HostDispatchTarget, HostProtocol, HostRequest, dispatch_request_with_middleware,
};
use super::support::{NoopPoolHost, StaticTransport, endpoint, pool_with_endpoint, test_engine};
use crate::host::HostContext;
use crate::middleware::{ChatRequestMiddleware, HostMiddleware};

struct InjectSystemMiddleware {
    seen_gateway_fields: Arc<Mutex<Option<serde_json::Value>>>,
}

impl ChatRequestMiddleware for InjectSystemMiddleware {
    fn on_chat_request<'a>(
        &'a self,
        _ctx: &'a HostContext<'_>,
        request: &'a mut ProxyChatRequest,
        gateway_fields: &'a HashMap<String, serde_json::Value>,
    ) -> crate::host::HostFuture<'a, crate::error::HostResult<()>> {
        let seen = self.seen_gateway_fields.clone();
        Box::pin(async move {
            *seen.lock().expect("lock") = gateway_fields.get("_ctx").cloned();
            request.messages.insert(
                0,
                Message::text(MessageRole::System, "injected-by-middleware"),
            );
            Ok(())
        })
    }
}

#[tokio::test]
async fn request_middleware_runs_before_upstream_and_reads_gateway_fields() {
    let seen_upstream = Arc::new(Mutex::new(Vec::new()));
    let transport = Arc::new(StaticTransport {
        response: Some(unigateway_core::transport::TransportResponse {
            status: 200,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            }))
            .expect("body"),
        }),
        stream_chunks: None,
        seen: seen_upstream.clone(),
    });
    let engine = test_engine(transport);
    let pool = pool_with_endpoint("pool-mw", endpoint());
    engine.upsert_pool(pool.clone()).await.expect("upsert");

    let seen_gw = Arc::new(Mutex::new(None));
    let middleware = HostMiddleware::new().with_request(Arc::new(InjectSystemMiddleware {
        seen_gateway_fields: seen_gw.clone(),
    }));

    let host = NoopPoolHost;
    let context = HostContext::from_parts(&engine, &host);
    let request = ProxyChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![Message::text(MessageRole::User, "hello")],
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
        gateway_fields: HashMap::from([("_ctx".to_string(), json!({"epoch": 2}))]),
        extra: HashMap::new(),
        metadata: HashMap::new(),
    };

    let _ = dispatch_request_with_middleware(
        &context,
        HostDispatchTarget::Pool(pool),
        HostProtocol::OpenAiChat,
        None,
        HostRequest::Chat(request),
        Some(&middleware),
    )
    .await
    .expect("dispatch");

    assert_eq!(*seen_gw.lock().expect("lock"), Some(json!({"epoch": 2})));

    let upstream = seen_upstream.lock().expect("lock");
    let body: serde_json::Value =
        serde_json::from_slice(upstream[0].body.as_ref().expect("body")).expect("json");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    assert_eq!(
        messages
            .first()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str()),
        Some("system")
    );
    assert!(!body.as_object().expect("object").contains_key("_ctx"));
}
