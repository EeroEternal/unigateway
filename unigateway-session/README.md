# unigateway-session

Optional **reference** session prefix store and delta assembly middleware. Default UniGateway builds do not include this crate.

## Features

| Feature | Description |
| --- | --- |
| *(default)* | In-memory store + `DeltaAssemblyMiddleware` |
| `http` | Axum 0.8 routes for publish/delete |

## Quick start

```toml
unigateway-sdk = { version = "2.14", features = ["host", "session"] }
unigateway-session = { version = "2.14", features = ["http"] }
```

```rust
use std::sync::Arc;
use unigateway_session::{
    DeltaAssemblyMiddleware, MemorySessionStore, SessionKey, SessionMiddlewareConfig,
    TailPositionPolicy, SESSION_GATEWAY_FIELD,
};
use unigateway_host::{HostMiddleware, dispatch_request_with_middleware};

let store = Arc::new(MemorySessionStore::new());
let middleware = HostMiddleware::new()
    .with_request(Arc::new(DeltaAssemblyMiddleware::new(store.clone())));

// Client body includes gateway-only field (not forwarded upstream):
// { "messages": [...], "_session_context": {"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1} }
```

### Namespace isolation

Hosts inject tenant boundaries via a key resolver; clients must not supply a trusted namespace:

```rust
use unigateway_session::{SessionKey, SessionMiddlewareConfig};

let config = SessionMiddlewareConfig::default().with_key_resolver(Arc::new(|_host, ctx| {
    SessionKey::new("my-tenant", ctx.session_id.clone())
}));
let middleware = DeltaAssemblyMiddleware::with_store(store.clone(), config);
```

Bare `publish("s1", …)` APIs use `DEFAULT_NAMESPACE` for backward compatibility.

### Fingerprint and size limits (P1)

```rust
use unigateway_session::{
    FingerprintPolicy, SessionMiddlewareConfig, SessionSizeLimits, SessionStoreConfig,
};

let store = Arc::new(MemorySessionStore::with_config(SessionStoreConfig {
    size_limits: SessionSizeLimits {
        max_prefix_bytes: Some(512 * 1024),
        ..Default::default()
    },
}));

let middleware = DeltaAssemblyMiddleware::with_store(
    store,
    SessionMiddlewareConfig {
        fingerprint_policy: FingerprintPolicy::Optional,
        size_limits: SessionSizeLimits {
            max_assembled_bytes: Some(1024 * 1024),
            ..Default::default()
        },
        ..Default::default()
    },
);
```

Hosts compute opaque `Fingerprint { algorithm, value }`; legacy `_session_context.prefix_hash` still maps to a fingerprint with an empty algorithm.

## HTTP routes (`http` feature)

With `SessionHttpConfig::default()` (`/v1/gateway` prefix, `default` namespace):

- `POST /v1/gateway/sessions/{id}/publish` — body `{ "epoch", "messages", "pinned_boundary"? , "fingerprint"? , "message_count"? }`
- `DELETE /v1/gateway/sessions/{id}`

Set `SessionHttpConfig.namespace` for host-level isolation. Merge `session_router(store, config)` into your embedder Axum 0.8 app.

## Publish semantics

| Scenario | Result |
| --- | --- |
| No existing session | `PublishResult::Created` |
| Higher epoch | `PublishResult::Replaced` |
| Lower epoch | `SessionError::StaleEpoch` |
| Same epoch, identical prefix | `PublishResult::AlreadyCurrent` |
| Same epoch, different prefix | `SessionError::EpochConflict` |

## Delta assembly

- Concatenates stored prefix (`Vec<Value>`) + request `raw_messages` tail **without** converting through simplified `Message`.
- Preserves tool calls, multimodal content, and ingress metadata (`client_protocol`, etc.).
- Requires `raw_messages` on delta requests; fails explicitly if missing.
- `tail_start` (message index) validation is controlled by `TailPositionPolicy` (`Ignore` / `Optional` / `ExactPrefixLength`).
- Optional fingerprint validation via `FingerprintPolicy`.
- Optional tail/assembled byte limits via `SessionSizeLimits`.

## Pluggable store

Implement `SessionStore` for custom backends; `DeltaAssemblyMiddleware<S: SessionStore>` accepts `Arc<dyn SessionStore>` or concrete store types. The trait also supports `touch_key` and `purge_expired`.

## Session lifetime (P2)

```rust
use std::time::Duration;
use unigateway_session::{MemorySessionStore, SessionLifetime, SessionStoreConfig};

let store = MemorySessionStore::with_config(SessionStoreConfig {
    lifetime: SessionLifetime {
        idle_ttl: Some(Duration::from_secs(3600)),
        max_lifetime: Some(Duration::from_secs(86_400)),
        touch_on_read: true,
    },
    ..Default::default()
});

// Optional background sweep (hosts schedule this themselves):
let removed = store.purge_expired()?;
```

- Lazy expiration on `get_key` / `publish_key`; expired sessions return `SessionError::Expired`.
- Publish refreshes idle time; epoch replace resets creation time.
- Delta reads refresh idle time when `touch_on_read: true` (default).
- Optional `SessionLifecycleHook` for metrics/audit (no message content in events).

## Pipeline order

1. Protocol parse → `gateway_fields["_session_context"]`
2. Optional `DeltaAssemblyMiddleware` (delta delivery)
3. Host middleware / dispatch → upstream

See [`docs/design/embedder-neutral-extensions.md`](../docs/design/embedder-neutral-extensions.md), [`docs/dev/issues/04-session-generalization-p0.md`](../docs/dev/issues/04-session-generalization-p0.md), [`docs/dev/issues/05-session-generalization-p1.md`](../docs/dev/issues/05-session-generalization-p1.md), and [`docs/dev/issues/06-session-generalization-p2.md`](../docs/dev/issues/06-session-generalization-p2.md).
