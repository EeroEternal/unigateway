# UniGateway Embedder Guide

This document explains how to embed UniGateway into a host application such as OpenHub,
a custom proxy, or an internal AI platform.

The recommended path is to depend on `unigateway-sdk` and use its namespaced re-exports:

- `unigateway_sdk::core`
- `unigateway_sdk::protocol`
- `unigateway_sdk::host`

If you need finer-grained control, you can still depend on `unigateway-core`,
`unigateway-protocol`, and `unigateway-host` directly.

---

## 1. Dependency setup

Recommended path: add `unigateway-sdk` and use its namespaced re-exports.
If you want finer-grained control, you can still depend on the individual crates
directly.

```toml
[dependencies]
unigateway-sdk = "1.6"

# Or depend on individual crates directly:
# unigateway-core = { path = "../unigateway-core" }
# unigateway-protocol = { path = "../unigateway-protocol" }
# unigateway-host = { path = "../unigateway-host" }
```

`unigateway-sdk` is intentionally a thin facade. It re-exports the underlying crates as
`unigateway_sdk::core`, `unigateway_sdk::protocol`, and `unigateway_sdk::host` instead of
introducing a second abstraction layer.

Recommended dependency policy:

- Prefer depending on `unigateway-sdk` only.
- Only mix direct `unigateway-core` / `unigateway-protocol` / `unigateway-host` dependencies if
    you need explicit lower-level control.
- If you do mix them, keep them on the same release line as `unigateway-sdk`.

The core crate brings reqwest and tokio as transitive dependencies. No feature flags are
required for the default HTTP transport.

For `unigateway-sdk`, no extra feature flags are required for the default full embedder stack.
If you disable default features, prefer `features = ["host"]`; `embed` remains available as a
1.x compatibility alias. If you want reusable host fixtures for integration tests, enable
`features = ["testing"]`.

---

## 2. Building the engine

### 2a. Zero-boilerplate (recommended)

```rust
use unigateway_sdk::core::UniGatewayEngine;

let engine = UniGatewayEngine::builder()
    .with_builtin_http_drivers()   // registers OpenAI + Anthropic drivers
    .build();
```

`with_builtin_http_drivers()` creates an `InMemoryDriverRegistry`, instantiates the
default `ReqwestHttpTransport`, and registers both the `openai-compatible` and
`anthropic` drivers automatically.

### 2b. Custom driver registry

```rust
use std::sync::Arc;
use unigateway_sdk::core::{InMemoryDriverRegistry, UniGatewayEngine};
use unigateway_sdk::core::protocol::builtin_drivers;
use unigateway_sdk::core::transport::ReqwestHttpTransport;

let transport = Arc::new(ReqwestHttpTransport::default());
let registry  = Arc::new(InMemoryDriverRegistry::new());
for driver in builtin_drivers(transport) {
    registry.register(driver);
}

let engine = UniGatewayEngine::builder()
    .with_driver_registry(registry)
    .build();
```

Use this path when you need to add custom drivers or replace the HTTP transport.

### 2c. Observability hooks

```rust
use std::sync::Arc;
use futures_util::future::BoxFuture;
use unigateway_sdk::core::{
    AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestReport, UniGatewayEngine,
};

struct MyHooks;

impl GatewayHooks for MyHooks {
    fn on_attempt_started<'a>(&'a self, _event: AttemptStartedEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_attempt_finished<'a>(&'a self, _event: AttemptFinishedEvent) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn on_request_finished<'a>(&'a self, report: RequestReport) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // report.usage — token counts
            // report.latency_ms — end-to-end wall time
            // report.metadata — merged pool/endpoint/request tags
            // report.attempts — per-attempt status + latency
            println!("request {} finished in {}ms", report.request_id, report.latency_ms);
        })
    }
}

let engine = UniGatewayEngine::builder()
    .with_builtin_http_drivers()
    .with_hooks(Arc::new(MyHooks))
    .build();
```

---

## 3. Pool lifecycle

### Key rule

**Pools must be registered in the engine before any request is proxied.**
`engine.upsert_pool(pool)` is the authoritative write path. The engine stores pools
in-memory; `pool_for_service` in the host layer should read from this in-memory state,
not hit an external datastore on every request.

### 3a. Startup sync

```rust
use unigateway_sdk::core::{
    Endpoint, LoadBalancingStrategy, ProviderKind, ProviderPool, RetryPolicy, SecretString,
};

// Fetch pools from your datastore once at startup.
let pools: Vec<ProviderPool> = load_from_db().await?;
for pool in pools {
    engine.upsert_pool(pool).await?;
}
```

### 3b. Live updates (hot-reload)

When your pool configuration changes at runtime:

```rust
// Add or update a pool:
engine.upsert_pool(updated_pool).await?;

// Remove a pool:
engine.remove_pool("pool-id").await?;
```

### 3c. Minimal pool construction example

```rust
use unigateway_sdk::core::{
    Endpoint, ProviderKind, ProviderPool, LoadBalancingStrategy,
    RetryPolicy, SecretString, ModelPolicy,
};
use std::collections::HashMap;

let pool = ProviderPool {
    pool_id:        "my-service".to_string(),
    load_balancing: LoadBalancingStrategy::RoundRobin,
    retry_policy:   RetryPolicy::default(),
    metadata:       HashMap::new(),
    endpoints: vec![
        Endpoint {
            endpoint_id:   "ep-openai-1".to_string(),
            // Used by provider-hint matching and often shown in operator-facing output.
            provider_name: Some("openai-main".to_string()),
            // Keeps the original upstream/source id available to hint matching.
            source_endpoint_id: Some("openai-main".to_string()),
            // Enables family-level hints such as "openai" or "deepseek".
            provider_family: Some("openai".to_string()),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id:     "openai-compatible".to_string(),
            base_url:      "https://api.openai.com".to_string(),
            api_key:       SecretString::new("sk-...".to_string()),
            model_policy:  ModelPolicy::default(),
            enabled:       true,
            metadata:      HashMap::new(),
        },
    ],
};

engine.upsert_pool(pool).await?;
```

Endpoint hint fields matter more than they look:

- `provider_name`: stable operator-facing label used by provider-hint matching.
- `source_endpoint_id`: original upstream or domain id retained across display renames.
- `provider_family`: coarse vendor family such as `openai`, `anthropic`, or `deepseek`.

When possible, fill all three and keep them stable across restarts so routing hints do not drift.

---

## 4. Proxying requests

### 4a. Chat completion (streaming or non-streaming)

```rust
use std::collections::HashMap;
use unigateway_sdk::core::{ExecutionTarget, Message, MessageRole, ProxyChatRequest, ProxySession};

let request = ProxyChatRequest {
    model:       "gpt-4o-mini".to_string(),
    messages:    vec![Message { role: MessageRole::User, content: "Hello".to_string() }],
    temperature: Some(0.7),
    top_p:       None,
    max_tokens:  None,
    stream:      false,
    metadata:    HashMap::from([
        ("user_id".to_string(),    "u-123".to_string()),
        ("trace_id".to_string(),   "t-abc".to_string()),
    ]),
};

let target = ExecutionTarget::Pool { pool_id: "my-service".to_string() };

match engine.proxy_chat(request, target).await? {
    ProxySession::Completed(resp) => {
        let text = resp.response.output_text.unwrap_or_default();
        let report = resp.report;   // usage, latency, metadata
    }
    ProxySession::Streaming(streaming) => {
        // consume streaming.stream (Stream<ChatResponseChunk>)
        // await streaming.completion for the final RequestReport
    }
}
```

The `metadata` map on the request is merged into `RequestReport.metadata` with the
highest priority — useful for attaching per-call tags (user id, tenant id, trace id)
that flow through to hooks without any pool-level configuration.

### 4b. Embeddings

```rust
use unigateway_sdk::core::{ExecutionTarget, ProxyEmbeddingsRequest};

let request = ProxyEmbeddingsRequest {
    model:           "text-embedding-3-small".to_string(),
    input:           vec!["hello world".to_string()],
    encoding_format: None,
    metadata:        std::collections::HashMap::new(),
};

let target = ExecutionTarget::Pool { pool_id: "embed-service".to_string() };
let response = engine.proxy_embeddings(request, target).await?;
// response.embeddings: Vec<Vec<f32>>
// response.report:     RequestReport
```

### 4c. OpenAI Responses API

```rust
use unigateway_sdk::core::{ExecutionTarget, ProxyResponsesRequest};

let request = ProxyResponsesRequest {
    model:    "gpt-4.1-mini".to_string(),
    input:    Some(serde_json::json!("What is the capital of France?")),
    stream:   false,
    // ... other fields
    ..Default::default()
};

let target = ExecutionTarget::Pool { pool_id: "my-service".to_string() };
let session = engine.proxy_responses(request, target).await?;
```

---

## 5. Translating HTTP payloads (unigateway-protocol)

When your HTTP handler receives a raw JSON body, use the helpers in
`unigateway_sdk::protocol` to convert it into a typed core request:

```rust
use unigateway_sdk::protocol::{
    openai_payload_to_chat_request,
    anthropic_payload_to_chat_request,
    openai_payload_to_embed_request,
    openai_payload_to_responses_request,
};

async fn handle_chat(body: serde_json::Value) -> axum::response::Response {
    let request = openai_payload_to_chat_request(&body, "gpt-4o-mini")
        .expect("parse request");

    // ... engine.proxy_chat(request, target) ...
}
```

These converters are lenient: unknown fields are ignored, role spellings are
normalised, and content can be either a string or an array of content blocks.

---

## 6. Implementing the host contract

If you use `unigateway_sdk::host`'s `HostContext` to drive the built-in request
handlers, you only need to implement `PoolHost` on your application state and
pass the engine reference separately when building `HostContext`:

```rust
use unigateway_sdk::core::{ProviderPool, UniGatewayEngine};
use unigateway_sdk::host::{
    EnvPoolHost, EnvProvider, HostContext, HostFuture, PoolHost, PoolLookupError,
    PoolLookupOutcome, PoolLookupResult, build_env_pool,
};

struct AppState {
    engine: std::sync::Arc<UniGatewayEngine>,
    openai_base_url: String,
    openai_api_key: String,
    openai_model: String,
    // ... other fields
}

impl PoolHost for AppState {
    fn pool_for_service<'a>(&'a self, service_id: &'a str) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        // Fast in-memory read — the pool must already be upserted.
        Box::pin(async move {
            Ok(match self.engine.get_pool(service_id).await {
                Some(pool) => PoolLookupOutcome::found(pool),
                None => PoolLookupOutcome::not_found(),
            })
        })
    }
}

impl EnvPoolHost for AppState {
    fn env_pool<'a>(
        &'a self,
        provider: EnvProvider,
        api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            let api_key = api_key_override.unwrap_or(self.openai_api_key.as_str());
            if api_key.is_empty() {
                return Ok(PoolLookupOutcome::not_found());
            }

            let pool = build_env_pool(
                provider,
                &self.openai_model,
                &self.openai_base_url,
                api_key,
            );

            self.engine
                .upsert_pool(pool.clone())
                .await
                .map_err(PoolLookupError::other)?;

            Ok(PoolLookupOutcome::found(pool))
        })
    }
}

let host = HostContext::from_parts(&app_state.engine, &app_state);
```

> ⚠️  Do **not** query your database inside `pool_for_service`.  Pools must be loaded on
> startup (or via a background sync task) and kept alive in the engine's in-memory state.
>
> `EnvPoolHost::env_pool` is optional. If your embedder does not support env-backed fallback
> pools, you can omit it and inherit the default implementation, which returns
> `Ok(PoolLookupOutcome::NotFound)`.
>
> If you do support env fallback, `EnvPoolHost::env_pool` is the only place where on-demand
> synthetic pools should be created. The host dispatch API now receives either a service id or a
> concrete pool target and should not reconstruct provider config on its own.

> `PoolLookupOutcome` is `#[non_exhaustive]`. External embedders should keep a fallback arm when
> matching so future host versions can add richer states without forcing another immediate rewrite.

For reusable integration-test fixtures, enable `unigateway-host`'s `testing` feature or
`unigateway-sdk`'s `testing` feature and use `unigateway_host::testing::MockHost` together with
`unigateway_host::testing::build_context`.

Version compatibility:

- Keep `unigateway-sdk`, `unigateway-host`, `unigateway-core`, and `unigateway-protocol` on the same minor version.
- When in doubt, pin all of them to the exact same release.

### 6a. Adapting `ProtocolHttpResponse` to axum

`unigateway-sdk` deliberately does not depend on a specific web framework, but the neutral
response types are straightforward to adapt. For axum, a minimal adapter looks like this:

```rust
use axum::{
    Json,
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};
use unigateway_sdk::protocol::{ProtocolHttpResponse, ProtocolResponseBody};

fn into_axum_response(response: ProtocolHttpResponse) -> Response {
    let (status, body) = response.into_parts();

    match body {
        ProtocolResponseBody::Json(value) => (status, Json(value)).into_response(),
        ProtocolResponseBody::ServerSentEvents(stream) => (
            status,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response(),
    }
}
```

That is intentionally the last adapter step. Parsing, dispatch, and neutral response rendering
should stay in `core` / `protocol` / `host`; only the framework conversion should live in your
application.

Minimal stack example:

```rust
use unigateway_sdk::core::{ExecutionTarget, ProxySession, UniGatewayEngine};
use unigateway_sdk::protocol::openai_payload_to_chat_request;

async fn handle_chat(body: serde_json::Value) -> anyhow::Result<String> {
    let engine = UniGatewayEngine::builder()
        .with_builtin_http_drivers()
        .build()?;

    let request = openai_payload_to_chat_request(&body, "gpt-4o-mini")?;
    let target = ExecutionTarget::Pool {
        pool_id: "my-service".to_string(),
    };

    match engine.proxy_chat(request, target).await? {
        ProxySession::Completed(response) => Ok(response.response.output_text.unwrap_or_default()),
        ProxySession::Streaming(_streaming) => anyhow::bail!("example expects non-streaming chat"),
    }
}
```

---

## 7. Common pitfalls

| Pitfall | Fix |
|---|---|
| `GatewayError::PoolNotFound` at runtime | Call `engine.upsert_pool()` for every pool before handling requests |
| `pool_for_service` hits DB per request | Return `engine.get_pool()` instead; sync pools at startup |
| Request metadata lost in `RequestReport` | Set `request.metadata` before calling `proxy_chat` / `proxy_embeddings` |
| Using `ProxyChatRequest` directly as HTTP payload | Parse the raw JSON body with `openai_payload_to_chat_request` first |
| Custom driver not found | Register it in `InMemoryDriverRegistry` before building the engine |
