# unigateway-session

Optional **reference** session prefix store and delta assembly middleware. Default UniGateway builds do not include this crate.

## Features

| Feature | Description |
| --- | --- |
| *(default)* | In-memory store + `DeltaAssemblyMiddleware` |
| `http` | Axum routes for publish/delete |

## Quick start

```toml
unigateway-sdk = { version = "2.10", features = ["host", "session"] }
unigateway-session = { version = "2.10", features = ["http"] }
```

```rust
use std::sync::Arc;
use unigateway_session::{
    DeltaAssemblyMiddleware, MemorySessionStore, SESSION_GATEWAY_FIELD,
};
use unigateway_host::{HostMiddleware, dispatch_request_with_middleware};

let store = Arc::new(MemorySessionStore::new());
let middleware = HostMiddleware::new()
    .with_request(Arc::new(DeltaAssemblyMiddleware::new(store.clone())));

// Client body includes gateway-only field (not forwarded upstream):
// { "messages": [...], "_session_context": {"session_id":"s1","epoch":1,"delivery":"delta"} }
```

## HTTP routes (`http` feature)

With `SessionHttpConfig::default()` (`/v1/gateway` prefix):

- `POST /v1/gateway/sessions/{id}/publish` — body `{ "epoch", "messages", "pinned_boundary"? }`
- `DELETE /v1/gateway/sessions/{id}`

Merge `session_router(store, config)` into your embedder Axum app.

## Pipeline order

1. Protocol parse → `gateway_fields["_session_context"]`
2. Optional `DeltaAssemblyMiddleware` (delta delivery)
3. Host middleware / dispatch → upstream

See [`docs/design/embedder-neutral-extensions.md`](../docs/design/embedder-neutral-extensions.md).
