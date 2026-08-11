# unigateway-session-redis

Optional Redis-backed [`SessionStore`](https://docs.rs/unigateway-session/latest/unigateway_session/trait.SessionStore.html) for UniGateway embedders that need shared session prefix state across instances.

## Usage

```toml
unigateway-session-redis = "2.14"
```

```rust
use std::sync::Arc;

use unigateway_session::{DeltaAssemblyMiddleware, SessionMiddlewareConfig, SessionStore};
use unigateway_session_redis::RedisSessionStore;

let store = Arc::new(RedisSessionStore::open("redis://127.0.0.1/")?);
let middleware = DeltaAssemblyMiddleware::with_store(store, SessionMiddlewareConfig::default());
```

`RedisSessionStoreConfig` mirrors `SessionStoreConfig` (size limits, lifetime, lifecycle hooks) and adds a Redis key prefix (default `unigateway:session:`).

## Semantics

- **Epoch CAS**: atomic publish via Redis Lua; same outcomes as `MemorySessionStore` (`Created`, `Replaced`, `AlreadyCurrent`, `StaleEpoch`, `EpochConflict`).
- **Namespace**: encoded in the Redis key using the same `\0`-separated storage key as the in-memory store.
- **Idle TTL**: Redis `EXPIRE` refreshed on publish, touch, and read (when `touch_on_read` is enabled).
- **Max lifetime**: enforced on read/purge via `is_session_expired`, consistent with the memory store.

## Tests

Unit tests compile without Redis. Integration tests are `#[ignore]` and run when `REDIS_URL` is set:

```bash
REDIS_URL=redis://127.0.0.1/ cargo test -p unigateway-session-redis -- --ignored
```
