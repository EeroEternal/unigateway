# UniGateway Embedder Guide

This document explains how to embed `unigateway-core` (and optionally `unigateway-runtime`)
into a host application such as OpenHub, a custom proxy, or an internal AI platform.

---

## 1. Dependency setup

Add `unigateway-core` to your `Cargo.toml`.  If you want the protocol translation helpers
and the OpenAI / Anthropic HTTP-response formatters, add `unigateway-runtime` as well.

```toml
[dependencies]
unigateway-core    = { path = "../unigateway-core" }   # or version = "1"
unigateway-runtime = { path = "../unigateway-runtime" } # optional
```

The core crate brings reqwest and tokio as transitive dependencies.  No feature flags are
required for the default HTTP transport.

---

## 2. Building the engine

### 2a. Zero-boilerplate (recommended)

```rust
use unigateway_core::UniGatewayEngine;

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
use unigateway_core::{UniGatewayEngine, InMemoryDriverRegistry};
use unigateway_core::protocol::builtin_drivers;
use unigateway_core::transport::ReqwestHttpTransport;

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
use unigateway_core::{UniGatewayEngine, GatewayHooks, RequestReport,
                      AttemptStartedEvent, AttemptFinishedEvent};
use futures_util::future::BoxFuture;

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
`engine.upsert_pool(pool)` is the authoritative write path.  The engine stores pools
in-memory; `pool_for_service` in the runtime host should read from this in-memory state,
not hit an external datastore on every request.

### 3a. Startup sync

```rust
use unigateway_core::{ProviderPool, Endpoint, ProviderKind, LoadBalancingStrategy,
                      RetryPolicy, SecretString};

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
use unigateway_core::{
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

---

## 4. Proxying requests

### 4a. Chat completion (streaming or non-streaming)

```rust
use unigateway_core::{ExecutionTarget, ProxyChatRequest, ProxySession, Message, MessageRole};
use std::collections::HashMap;

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
        let text = resp.message.content;
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
use unigateway_core::{ProxyEmbeddingsRequest, ExecutionTarget};

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
use unigateway_core::{ProxyResponsesRequest, ExecutionTarget};

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

## 5. Translating HTTP payloads (unigateway-runtime)

When your HTTP handler receives a raw JSON body, use the helpers in
`unigateway_runtime::protocol` to convert it into a typed core request:

```rust
use unigateway_runtime::protocol::{
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

## 6. Implementing the runtime host traits

If you use `unigateway-runtime`'s `RuntimeContext` to drive the built-in request
handlers, implement the four host traits on your application state struct:

```rust
use unigateway_core::{UniGatewayEngine, ProviderPool};
use unigateway_runtime::host::{
    RuntimeConfigHost, RuntimeConfig,
    RuntimeEngineHost,
    RuntimePoolHost, RuntimeFuture,
    RuntimeRoutingHost, ResolvedProvider,
};

struct AppState {
    engine: std::sync::Arc<UniGatewayEngine>,
    // ... other fields
}

impl RuntimeEngineHost for AppState {
    fn core_engine(&self) -> &UniGatewayEngine { &self.engine }
}

impl RuntimePoolHost for AppState {
    fn pool_for_service<'a>(&'a self, service_id: &'a str) -> RuntimeFuture<'a, anyhow::Result<Option<ProviderPool>>> {
        // Fast in-memory read — the pool must already be upserted.
        Box::pin(async move {
            Ok(self.engine.get_pool(service_id).await)
        })
    }
}

// RuntimeConfigHost + RuntimeRoutingHost omitted for brevity.
```

> ⚠️  Do **not** query your database inside `pool_for_service`.  Pools must be loaded on
> startup (or via a background sync task) and kept alive in the engine's in-memory state.

---

## 7. Common pitfalls

| Pitfall | Fix |
|---|---|
| `GatewayError::PoolNotFound` at runtime | Call `engine.upsert_pool()` for every pool before handling requests |
| `pool_for_service` hits DB per request | Return `engine.get_pool()` instead; sync pools at startup |
| Request metadata lost in `RequestReport` | Set `request.metadata` before calling `proxy_chat` / `proxy_embeddings` |
| Using `ProxyChatRequest` directly as HTTP payload | Parse the raw JSON body with `openai_payload_to_chat_request` first |
| Custom driver not found | Register it in `InMemoryDriverRegistry` before building the engine |
