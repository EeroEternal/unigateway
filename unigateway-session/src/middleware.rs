use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use unigateway_core::ProxyChatRequest;
use unigateway_host::{ChatRequestMiddleware, HostContext, HostFuture, HostResult};

use crate::SESSION_GATEWAY_FIELD;
use crate::lifecycle::{SessionLifecycleEvent, SessionLifecycleHook, SessionSizeRejectKind};
use crate::store::{
    DEFAULT_NAMESPACE, Fingerprint, FingerprintPolicy, SessionError, SessionKey, SessionSizeLimits,
    SessionStore, fingerprints_match,
};

/// Delivery mode carried in gateway session context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionDelivery {
    #[default]
    Full,
    Delta,
}

/// How delta requests validate `tail_start` (message array index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TailPositionPolicy {
    Ignore,
    #[default]
    Optional,
    ExactPrefixLength,
}

/// Parsed session hints from `gateway_fields["_session_context"]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionGatewayContext {
    pub session_id: String,
    pub epoch: u64,
    #[serde(default)]
    pub delivery: SessionDelivery,
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
    #[serde(default)]
    pub prefix_hash: Option<String>,
    #[serde(default)]
    pub tail_start: Option<u64>,
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

    /// Effective request fingerprint, mapping legacy `prefix_hash` when needed.
    pub fn request_fingerprint(&self) -> Option<Fingerprint> {
        if let Some(fingerprint) = &self.fingerprint {
            return Some(fingerprint.clone());
        }
        self.prefix_hash.as_ref().map(|value| Fingerprint {
            algorithm: String::new(),
            value: value.clone(),
        })
    }
}

/// Resolves the store key for a session request. Namespace must come from the host.
pub type SessionKeyResolver =
    Arc<dyn Fn(&HostContext<'_>, &SessionGatewayContext) -> SessionKey + Send + Sync>;

fn default_key(_host: &HostContext<'_>, ctx: &SessionGatewayContext) -> SessionKey {
    SessionKey::new(DEFAULT_NAMESPACE, ctx.session_id.clone())
}

/// Configurable delta assembly behavior.
#[derive(Clone)]
pub struct SessionMiddlewareConfig {
    pub tail_position_policy: TailPositionPolicy,
    pub fingerprint_policy: FingerprintPolicy,
    pub size_limits: SessionSizeLimits,
    /// Explicit idle refresh after delta assembly when the store has `touch_on_read: false`.
    pub touch_on_delta: bool,
    pub lifecycle_hook: Option<Arc<dyn SessionLifecycleHook>>,
    pub key_resolver: SessionKeyResolver,
}

impl Default for SessionMiddlewareConfig {
    fn default() -> Self {
        Self {
            tail_position_policy: TailPositionPolicy::Optional,
            fingerprint_policy: FingerprintPolicy::Disabled,
            size_limits: SessionSizeLimits::default(),
            touch_on_delta: false,
            lifecycle_hook: None,
            key_resolver: Arc::new(default_key),
        }
    }
}

impl SessionMiddlewareConfig {
    pub fn with_key_resolver(mut self, resolver: SessionKeyResolver) -> Self {
        self.key_resolver = resolver;
        self
    }
}

/// Assembles `stored_prefix || tail` when `delivery=delta`.
pub struct DeltaAssemblyMiddleware<S: SessionStore + ?Sized = MemorySessionStore> {
    store: Arc<S>,
    config: SessionMiddlewareConfig,
}

use crate::store::MemorySessionStore;

impl DeltaAssemblyMiddleware<MemorySessionStore> {
    pub fn new(store: Arc<MemorySessionStore>) -> Self {
        Self::with_store(store, SessionMiddlewareConfig::default())
    }
}

impl<S: SessionStore + ?Sized> DeltaAssemblyMiddleware<S> {
    pub fn with_store(store: Arc<S>, config: SessionMiddlewareConfig) -> Self {
        Self { store, config }
    }

    fn emit(&self, event: SessionLifecycleEvent) {
        if let Some(hook) = &self.config.lifecycle_hook {
            hook.on_event(event);
        }
    }

    fn assemble_delta(
        &self,
        host: &HostContext<'_>,
        ctx: &SessionGatewayContext,
        request: &mut ProxyChatRequest,
    ) -> Result<(), SessionError> {
        let key = (self.config.key_resolver)(host, ctx);
        let stored = match self.store.get_key(&key) {
            Ok(Some(prefix)) => prefix,
            Ok(None) => {
                self.emit(SessionLifecycleEvent::DeltaMiss { key: key.clone() });
                return Err(SessionError::NotFound(key));
            }
            Err(error) => return Err(error),
        };
        if stored.epoch != ctx.epoch {
            return Err(SessionError::EpochMismatch {
                key: key.clone(),
                expected_epoch: stored.epoch,
                actual_epoch: ctx.epoch,
            });
        }

        if let Err(error) = validate_fingerprint(
            self.config.fingerprint_policy,
            &key,
            stored.fingerprint.as_ref(),
            ctx.request_fingerprint(),
        ) {
            if matches!(error, SessionError::FingerprintMismatch { .. }) {
                self.emit(SessionLifecycleEvent::FingerprintMismatch { key: key.clone() });
            }
            return Err(error);
        }

        if let Err(error) =
            self.validate_tail_start(&key, stored.messages.len() as u64, ctx.tail_start)
        {
            if matches!(error, SessionError::TailStartMismatch { .. }) {
                self.emit(SessionLifecycleEvent::TailMismatch { key: key.clone() });
            }
            return Err(error);
        }

        let tail = raw_tail_messages(request)
            .map_err(|error| SessionError::InvalidContext(error.to_string()))?;
        if let Err(error) = self.config.size_limits.validate_tail(&key, &tail) {
            if matches!(error, SessionError::TailTooLarge { .. }) {
                self.emit(SessionLifecycleEvent::SizeRejected {
                    key: key.clone(),
                    kind: SessionSizeRejectKind::Tail,
                });
            }
            return Err(error);
        }

        let merged = assemble_raw_messages(&stored.messages, &tail);
        if let Err(error) = self.config.size_limits.validate_assembled(&key, &merged) {
            if matches!(error, SessionError::AssembledTooLarge { .. }) {
                self.emit(SessionLifecycleEvent::SizeRejected {
                    key: key.clone(),
                    kind: SessionSizeRejectKind::Assembled,
                });
            }
            return Err(error);
        }
        request.raw_messages = Some(Value::Array(merged));

        self.emit(SessionLifecycleEvent::DeltaHit {
            key: key.clone(),
            epoch: ctx.epoch,
        });
        if self.config.touch_on_delta {
            self.store.touch_key(&key)?;
        }
        Ok(())
    }

    fn validate_tail_start(
        &self,
        key: &SessionKey,
        prefix_len: u64,
        tail_start: Option<u64>,
    ) -> Result<(), SessionError> {
        match self.config.tail_position_policy {
            TailPositionPolicy::Ignore => Ok(()),
            TailPositionPolicy::Optional => {
                if let Some(actual) = tail_start
                    && actual != prefix_len
                {
                    return Err(SessionError::TailStartMismatch {
                        key: key.clone(),
                        expected: prefix_len,
                        actual,
                    });
                }
                Ok(())
            }
            TailPositionPolicy::ExactPrefixLength => {
                let Some(actual) = tail_start else {
                    return Err(SessionError::TailStartMismatch {
                        key: key.clone(),
                        expected: prefix_len,
                        actual: u64::MAX,
                    });
                };
                if actual != prefix_len {
                    return Err(SessionError::TailStartMismatch {
                        key: key.clone(),
                        expected: prefix_len,
                        actual,
                    });
                }
                Ok(())
            }
        }
    }
}

impl<S: SessionStore + ?Sized + 'static> ChatRequestMiddleware for DeltaAssemblyMiddleware<S> {
    fn on_chat_request<'a>(
        &'a self,
        host: &'a HostContext<'_>,
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

            self.assemble_delta(host, &ctx, request)
                .map_err(|error| unigateway_host::HostError::CoreInvalidRequest(error.to_string()))
        })
    }
}

fn validate_fingerprint(
    policy: FingerprintPolicy,
    key: &SessionKey,
    stored: Option<&Fingerprint>,
    request: Option<Fingerprint>,
) -> Result<(), SessionError> {
    match policy {
        FingerprintPolicy::Disabled => Ok(()),
        FingerprintPolicy::Optional => {
            let (Some(stored), Some(request)) = (stored, request) else {
                return Ok(());
            };
            if fingerprints_match(stored, &request) {
                Ok(())
            } else {
                Err(SessionError::FingerprintMismatch { key: key.clone() })
            }
        }
        FingerprintPolicy::Required => {
            let Some(request) = request else {
                return Err(SessionError::InvalidContext(
                    "fingerprint required".to_string(),
                ));
            };
            let Some(stored) = stored else {
                return Err(SessionError::InvalidContext(
                    "stored fingerprint missing".to_string(),
                ));
            };
            if fingerprints_match(stored, &request) {
                Ok(())
            } else {
                Err(SessionError::FingerprintMismatch { key: key.clone() })
            }
        }
    }
}

fn assemble_raw_messages(prefix: &[Value], tail: &[Value]) -> Vec<Value> {
    let mut merged = prefix.to_vec();
    merged.extend_from_slice(tail);
    merged
}

fn raw_tail_messages(request: &ProxyChatRequest) -> Result<Vec<Value>> {
    let Some(raw) = request.raw_messages.as_ref().and_then(Value::as_array) else {
        return Err(anyhow::anyhow!(
            "delta assembly requires raw_messages; simplified messages cannot be preserved losslessly"
        ));
    };
    Ok(raw.clone())
}

impl From<SessionError> for unigateway_host::HostError {
    fn from(error: SessionError) -> Self {
        Self::CoreInvalidRequest(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::{Value, json};
    use unigateway_core::{ClientProtocol, ProxyChatRequest, UniGatewayEngine};
    use unigateway_host::{
        ChatRequestMiddleware, HostContext, PoolHost, PoolLookupOutcome, PoolLookupResult,
    };

    use super::{
        DeltaAssemblyMiddleware, FingerprintPolicy, SessionDelivery, SessionGatewayContext,
        SessionMiddlewareConfig, TailPositionPolicy,
    };
    use crate::SESSION_GATEWAY_FIELD;
    use crate::store::{
        Fingerprint, MemorySessionStore, SessionKey, SessionPrefix, SessionSizeLimits, SessionStore,
    };

    #[test]
    fn parses_session_gateway_context() {
        let fields = HashMap::from([(
            SESSION_GATEWAY_FIELD.to_string(),
            json!({"session_id":"s1","epoch":2,"delivery":"delta","tail_start":1}),
        )]);
        let ctx = SessionGatewayContext::from_gateway_fields(&fields)
            .expect("parse")
            .expect("ctx");
        assert_eq!(ctx.session_id, "s1");
        assert_eq!(ctx.epoch, 2);
        assert_eq!(ctx.delivery, SessionDelivery::Delta);
        assert_eq!(ctx.tail_start, Some(1));
    }

    #[test]
    fn legacy_prefix_hash_maps_to_fingerprint() {
        let ctx = SessionGatewayContext {
            session_id: "s1".into(),
            epoch: 1,
            delivery: SessionDelivery::Delta,
            fingerprint: None,
            prefix_hash: Some("abc123".into()),
            tail_start: None,
        };
        let fp = ctx.request_fingerprint().expect("fp");
        assert_eq!(fp.value, "abc123");
        assert!(fp.algorithm.is_empty());
    }

    #[tokio::test]
    async fn delta_middleware_assembles_raw_messages() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::new(store.clone());
        let host = TestHost::new();
        let request = assembled_request(
            json!([{"role":"user","content":"tail"}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );

        run_middleware(&middleware, &host, request, |request| {
            let raw = request
                .raw_messages
                .as_ref()
                .expect("raw")
                .as_array()
                .expect("array");
            assert_eq!(raw.len(), 2);
            assert_eq!(
                raw[0].get("content").and_then(Value::as_str),
                Some("prefix")
            );
            assert_eq!(raw[1].get("content").and_then(Value::as_str), Some("tail"));
        })
        .await;
    }

    #[tokio::test]
    async fn delta_middleware_preserves_tool_calls_and_multimodal_content() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{"id":"call_1","type":"function","function":{"name":"search","arguments":"{}"}}]
                    })],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::new(store);
        let host = TestHost::new();
        let tail = json!([{
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "result"
        }, {
            "role": "user",
            "content": [{"type":"text","text":"follow up"}]
        }]);

        let request = assembled_request(
            tail,
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );
        run_middleware(&middleware, &host, request, |request| {
            let raw = request
                .raw_messages
                .as_ref()
                .expect("raw")
                .as_array()
                .expect("array");
            assert_eq!(raw.len(), 3);
            assert!(raw[0].get("tool_calls").is_some());
            assert_eq!(
                raw[1].get("tool_call_id").and_then(Value::as_str),
                Some("call_1")
            );
            assert!(raw[2].get("content").and_then(Value::as_array).is_some());
        })
        .await;
    }

    #[tokio::test]
    async fn delta_middleware_preserves_client_protocol_metadata() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":[{"type":"text","text":"hi"}]})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::new(store);
        let host = TestHost::new();
        let mut request = assembled_request(
            json!([{"role":"user","content":[{"type":"text","text":"tail"}]}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );
        request.set_client_protocol(ClientProtocol::AnthropicMessages);

        run_middleware(&middleware, &host, request, |request| {
            assert_eq!(
                request.client_protocol(),
                Some(ClientProtocol::AnthropicMessages)
            );
            assert!(!request.has_openai_raw_messages());
        })
        .await;
    }

    #[tokio::test]
    async fn delta_middleware_fails_without_raw_messages() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::new(store);
        let mut request = ProxyChatRequest {
            model: String::new(),
            messages: Vec::new(),
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
        let host = TestHost::new();
        let error = middleware
            .on_chat_request(&host.context(), &mut request, &gateway_fields)
            .await
            .expect_err("missing raw_messages");
        assert!(error.to_string().contains("raw_messages"));
    }

    #[tokio::test]
    async fn exact_tail_start_policy_rejects_missing_tail_start() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::with_store(
            store,
            SessionMiddlewareConfig {
                tail_position_policy: TailPositionPolicy::ExactPrefixLength,
                ..SessionMiddlewareConfig::default()
            },
        );

        let request = assembled_request(
            json!([{"role":"user","content":"tail"}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta"}),
        );

        let gateway_fields = request.gateway_fields.clone();
        let host = TestHost::new();
        let mut request = request;
        let error = middleware
            .on_chat_request(&host.context(), &mut request, &gateway_fields)
            .await
            .expect_err("missing tail_start");
        assert!(error.to_string().contains("tail_start"));
    }

    #[tokio::test]
    async fn custom_namespace_resolver_is_used() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish_key(
                &SessionKey::new("tenant-a", "s1"),
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"tenant"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::with_store(
            store,
            SessionMiddlewareConfig::default().with_key_resolver(Arc::new(|_host, ctx| {
                SessionKey::new("tenant-a", ctx.session_id.clone())
            })),
        );

        let host = TestHost::new();
        let request = assembled_request(
            json!([{"role":"user","content":"tail"}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );

        run_middleware(&middleware, &host, request, |request| {
            let raw = request
                .raw_messages
                .as_ref()
                .expect("raw")
                .as_array()
                .expect("array");
            assert_eq!(
                raw[0].get("content").and_then(Value::as_str),
                Some("tenant")
            );
        })
        .await;
    }

    #[tokio::test]
    async fn optional_fingerprint_mismatch_is_rejected() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: Some(Fingerprint {
                        algorithm: "test-v1".into(),
                        value: "stored".into(),
                    }),
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::with_store(
            store,
            SessionMiddlewareConfig {
                fingerprint_policy: FingerprintPolicy::Optional,
                ..SessionMiddlewareConfig::default()
            },
        );

        let host = TestHost::new();
        let request = assembled_request(
            json!([{"role":"user","content":"tail"}]),
            json!({
                "session_id":"s1",
                "epoch":1,
                "delivery":"delta",
                "tail_start":1,
                "fingerprint":{"algorithm":"test-v1","value":"different"}
            }),
        );

        let gateway_fields = request.gateway_fields.clone();
        let mut request = request;
        let error = middleware
            .on_chat_request(&host.context(), &mut request, &gateway_fields)
            .await
            .expect_err("fingerprint mismatch");
        assert!(error.to_string().contains("fingerprint mismatch"));
    }

    #[tokio::test]
    async fn assembled_size_limit_is_enforced() {
        let store = Arc::new(MemorySessionStore::new());
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware = DeltaAssemblyMiddleware::with_store(
            store,
            SessionMiddlewareConfig {
                size_limits: SessionSizeLimits {
                    max_messages: None,
                    max_prefix_bytes: None,
                    max_tail_bytes: None,
                    max_assembled_bytes: Some(10),
                },
                ..SessionMiddlewareConfig::default()
            },
        );

        let host = TestHost::new();
        let request = assembled_request(
            json!([{"role":"user","content":"this tail is too long for the limit"}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );

        let gateway_fields = request.gateway_fields.clone();
        let mut request = request;
        let error = middleware
            .on_chat_request(&host.context(), &mut request, &gateway_fields)
            .await
            .expect_err("assembled too large");
        assert!(error.to_string().contains("assembled request too large"));
    }

    #[tokio::test]
    async fn dyn_session_store_trait_object_works() {
        let store: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::new());
        store
            .publish_key(
                &SessionKey::default_namespace("s1"),
                SessionPrefix {
                    epoch: 1,
                    messages: vec![json!({"role":"user","content":"prefix"})],
                    pinned_boundary: None,
                    fingerprint: None,
                    message_count: None,
                },
            )
            .expect("publish");

        let middleware =
            DeltaAssemblyMiddleware::with_store(store, SessionMiddlewareConfig::default());
        let host = TestHost::new();
        let request = assembled_request(
            json!([{"role":"user","content":"tail"}]),
            json!({"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}),
        );

        run_middleware(&middleware, &host, request, |request| {
            assert_eq!(
                request
                    .raw_messages
                    .as_ref()
                    .and_then(Value::as_array)
                    .map(Vec::len),
                Some(2)
            );
        })
        .await;
    }

    fn assembled_request(raw_tail: Value, session_context: Value) -> ProxyChatRequest {
        ProxyChatRequest {
            model: String::new(),
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: Some(raw_tail),
            gateway_fields: HashMap::from([(SESSION_GATEWAY_FIELD.to_string(), session_context)]),
            extra: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    struct TestHost {
        engine: UniGatewayEngine,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                engine: UniGatewayEngine::builder()
                    .with_builtin_http_drivers()
                    .build()
                    .expect("engine"),
            }
        }

        fn context(&self) -> HostContext<'_> {
            struct EmptyPoolHost;
            impl PoolHost for EmptyPoolHost {
                fn pool_for_service<'a>(
                    &'a self,
                    _service_id: &'a str,
                ) -> unigateway_host::HostFuture<'a, PoolLookupResult<PoolLookupOutcome>>
                {
                    Box::pin(
                        async move { Err(unigateway_host::PoolLookupError::unavailable("unused")) },
                    )
                }
            }

            HostContext::from_parts(&self.engine, &EmptyPoolHost)
        }
    }

    async fn run_middleware<S: SessionStore + ?Sized + 'static>(
        middleware: &DeltaAssemblyMiddleware<S>,
        host: &TestHost,
        mut request: ProxyChatRequest,
        assert_fn: impl FnOnce(&ProxyChatRequest),
    ) {
        let gateway_fields = request.gateway_fields.clone();
        middleware
            .on_chat_request(&host.context(), &mut request, &gateway_fields)
            .await
            .expect("middleware");
        assert_fn(&request);
    }
}
