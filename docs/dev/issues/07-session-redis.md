# Issue 07: Redis SessionStore crate

## Goal

Ship an optional `unigateway-session-redis` crate so embedders (e.g. SmartGate/Zene) can persist session prefixes in Redis without pulling product logic into UniGateway core.

## Scope

- [x] New workspace member `unigateway-session-redis`
- [x] Implement `SessionStore` with epoch CAS (Lua), namespace keys, TTL touch/purge
- [x] Export `SessionKey::storage_key`, `SessionPrefix::normalize`, `is_session_expired` from `unigateway-session`
- [x] `#[ignore]` integration tests gated on `REDIS_URL`
- [x] Release workflow publish step
- [x] Crate README

## Out of scope

- Host HTTP error mapping (embedder responsibility)
- Redis Cluster slot migration tooling
- Async store trait (sync `SessionStore` only)

## Acceptance

Embedder can `Arc::new(RedisSessionStore::open(url)?)` and pass it to `DeltaAssemblyMiddleware::with_store` with the same semantics as `MemorySessionStore`.
