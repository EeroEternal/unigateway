# Session generalization P1: fingerprint, size limits, SessionStore trait

## Summary

Extend `unigateway-session` with **opaque fingerprint consistency**, **configurable size limits**, and a **pluggable `SessionStore` trait** — building on P0 (raw assembly, `SessionKey`, epoch CAS, tail policy).

Depends on: [`04-session-generalization-p0.md`](04-session-generalization-p0.md).

P2 (TTL, lifecycle hooks, external Redis/Postgres stores) remains out of scope for this issue.

## Problem

After P0, embedders like SmartGate still need:

1. **Prefix fingerprint validation** without baking Zene hash rules into UniGateway.
2. **Size guards** on prefix, tail, and assembled requests to prevent unbounded memory growth.
3. **Store abstraction** so hosts can swap in Redis/Postgres implementations without forking middleware.

## Design principles

- UniGateway **stores and compares** opaque fingerprints; hosts **compute** canonicalization.
- Fingerprint policy (`Disabled` / `Optional` / `Required`) is opt-in; default preserves P0 behavior.
- Size limits are optional config; default = no limits (unchanged behavior).
- `SessionStore` trait is sync (`Send + Sync`); async external stores live in separate crates wrapping the trait or providing their own middleware adapter.

## P1 scope

### P1-1 — Opaque fingerprint

```rust
pub struct Fingerprint {
    pub algorithm: String,
    pub value: String,
}
```

- Store `fingerprint` on `SessionPrefix` (optional).
- Parse `fingerprint` from `_session_context`; keep `prefix_hash` as legacy alias mapping to `Fingerprint { algorithm: "", value }`.
- Compare algorithm when both sides non-empty; compare `value` for match.
- Policies: `Disabled` (default), `Optional`, `Required`.
- Error: `SessionError::FingerprintMismatch`.

### P1-2 — Message count

- Add optional `message_count: u64` on `SessionPrefix` (`#[serde(default)]`).
- On publish, default to `messages.len()` when omitted.
- Available for hosts validating tail position independently of byte size.

### P1-3 — Size limits

```rust
pub struct SessionSizeLimits {
    pub max_messages: Option<usize>,
    pub max_prefix_bytes: Option<usize>,
    pub max_tail_bytes: Option<usize>,
    pub max_assembled_bytes: Option<usize>,
}
```

- Byte size via `serde_json::to_vec(messages).len()`.
- Enforce `max_messages` + `max_prefix_bytes` on publish (store).
- Enforce `max_tail_bytes` + `max_assembled_bytes` on delta assembly (middleware).
- Errors: `PrefixTooLarge`, `TailTooLarge`, `AssembledTooLarge` (with limit + actual bytes).

### P1-4 — SessionStore trait

```rust
pub trait SessionStore: Send + Sync {
    fn publish_key(&self, key: &SessionKey, prefix: SessionPrefix) -> Result<PublishResult, SessionError>;
    fn get_key(&self, key: &SessionKey) -> Result<Option<SessionPrefix>, SessionError>;
    fn delete_key(&self, key: &SessionKey) -> Result<(), SessionError>;
}
```

- `MemorySessionStore` implements `SessionStore`.
- Inherent methods remain for backward compatibility.
- `DeltaAssemblyMiddleware<S: SessionStore>` generic over store type.
- `MemorySessionStore::with_config(SessionStoreConfig { size_limits })`.

## Non-goals

- Zene v1 hash implementation, `DefaultHasher`, or message field selection rules.
- TTL, expiration, `touch`, `purge_expired` (P2).
- Redis/Postgres store crate (separate repo/crate later).
- HTTP 413 mapping in store API (optional `http` feature maps reference routes only).

## API compatibility

| Item | Strategy |
| --- | --- |
| `SessionPrefix` without fingerprint/message_count | `#[serde(default)]`; publish fills message_count |
| `_session_context.prefix_hash` | Still parsed; maps to legacy fingerprint |
| `FingerprintPolicy` | Default `Disabled` |
| `SessionSizeLimits` | Default all `None` |
| `DeltaAssemblyMiddleware::new(Arc<MemorySessionStore>)` | Unchanged signature |

## Acceptance criteria

- [x] `Fingerprint { algorithm, value }` stored on prefix and compared on delta per policy.
- [x] Legacy `prefix_hash` string still works with `Optional` policy.
- [x] Algorithm mismatch returns `FingerprintMismatch` when both sides specify algorithm.
- [x] `message_count` defaults to `messages.len()` on publish.
- [x] Prefix over `max_messages` or `max_prefix_bytes` rejected at publish.
- [x] Tail / assembled over limits rejected at delta assembly.
- [x] `SessionStore` trait implemented by `MemorySessionStore`.
- [x] Middleware works with `Arc<dyn SessionStore>` via generic or trait object.
- [x] Default config: no fingerprint check, no size limits (P0 behavior preserved).
- [x] Unit tests for fingerprint policies, size limits, trait dispatch.

## Implementation sketch

1. `store.rs`: `Fingerprint`, `FingerprintPolicy`, `SessionSizeLimits`, `SessionStoreConfig`, limit helpers, trait, extend `SessionPrefix` / `SessionError`.
2. `middleware.rs`: fingerprint validation, assembly size checks, generic `S: SessionStore`.
3. `http.rs`: optional `fingerprint` / `message_count` in publish body; map `*TooLarge` → 413 in reference router.
4. Tests + README + CHANGELOG update.

## References

- P0 spec: [`04-session-generalization-p0.md`](04-session-generalization-p0.md)
- SmartGate alignment: fingerprint compute stays in host; `Optional` for Zene Warm MVP
