# Session generalization P0: raw assembly, SessionKey, epoch CAS, tail policy

## Summary

Evolve `unigateway-session` from a minimal in-memory reference into **namespace-aware session consistency primitives** suitable for agent gateways (SmartGate, coding tools, and other embedders). P0 focuses on correctness and message fidelity; fingerprint, size limits, TTL, and pluggable stores follow in P1/P2.

This is **generic library work**, not Zene- or SmartGate-specific logic. Hosts own auth, tenant namespace construction, HTTP error mapping, and fingerprint canonicalization.

**Cross-repo context:** SmartGate reviewed and accepted this direction; they will keep their own `WarmStore` until a UniGateway release ships P0 + P1 capabilities.

## Problem

Current `unigateway-session` (v2.11 reference) has blocking gaps:

1. **Message loss:** `DeltaAssemblyMiddleware` converts `JSON → Message → JSON`, dropping `tool_calls`, multimodal content, Anthropic blocks, thinking fields, and other extensions.
2. **No tenant isolation:** store keys are bare `session_id` only.
3. **No epoch semantics:** `publish()` unconditionally overwrites; no stale/conflict/idempotent handling.
4. **No tail position validation:** `tail_start` is not parsed or checked.
5. **Minimal errors:** only `NotFound` / lock poison; no stable classification for host HTTP mapping.

## Design principles

- Session crate stores **opaque `Vec<Value>`** message sequences; it does not interpret `role`, `content`, `tool_calls`, or protocol schemas.
- **Namespace** is an opaque host-provided isolation boundary; clients must not supply a trusted namespace in `_session_context`.
- **Strict rules are configurable policies**, not global defaults (e.g. `TailPositionPolicy::ExactPrefixLength` is opt-in).
- **Stable `SessionError`**, not HTTP status codes, in the store/middleware API.
- **Additive semver:** keep bare `session_id` convenience methods via a default namespace.

## P0 scope

### P0-1 — Raw JSON assembly

- Concatenate `stored_prefix.messages` + tail as `Vec<Value>`.
- Write result to `ProxyChatRequest.raw_messages` only.
- Do **not** round-trip through simplified `Message`.
- Preserve existing request metadata (`client_protocol`, `openai_raw_messages` flag, etc.).
- If tail cannot be read losslessly from `raw_messages`, fail explicitly.

### P0-2 — Namespace-aware `SessionKey`

```rust
pub struct SessionKey {
    pub namespace: String,
    pub session_id: String,
}
pub const DEFAULT_NAMESPACE: &str = "default";
```

- All store operations accept `&SessionKey`.
- Bare `publish(session_id, …)` / `get` / `delete` remain as compat wrappers using `DEFAULT_NAMESPACE`.
- Middleware resolves keys via injectable resolver; default uses `DEFAULT_NAMESPACE + session_id`.
- Namespace must **not** appear in client-trusted `_session_context`.

### P0-3 — Atomic epoch publish

| Scenario | Result |
| --- | --- |
| No existing session | `PublishResult::Created` |
| New epoch > stored | `PublishResult::Replaced` |
| Epoch < stored | `SessionError::StaleEpoch` |
| Same epoch, same content | `PublishResult::AlreadyCurrent` |
| Same epoch, different content | `SessionError::EpochConflict` |
| Concurrent publish | Atomic per above rules |

Content equality for idempotency compares full prefix snapshot (`messages`, `pinned_boundary`, `epoch`), not client hash alone.

### P0-4 — Tail position policy

```rust
pub enum TailPositionPolicy {
    Ignore,
    Optional,           // default
    ExactPrefixLength,
}
```

- Parse optional `tail_start` (message array index) from `_session_context`.
- `ExactPrefixLength`: require `tail_start == prefix.messages.len()`.
- Document: `tail_start` is a **message index**, not token/byte offset.

## P1 (follow-up, not this issue)

- Opaque `Fingerprint { algorithm, value }` with `Disabled | Optional | Required` policy
- `message_count` on snapshot
- `max_messages`, `max_prefix_bytes`, `max_assembled_bytes`
- `SessionStore` trait; external Redis/Postgres in separate crates

## P2 (follow-up)

- `idle_ttl`, `max_lifetime`, lazy expiration, `touch`, `purge_expired`
- Lifecycle hooks/metrics (no message content in events)

## Non-goals

- Zene routes, `_zene_context`, SmartGate Project/API Key/Virtual Model semantics
- Fingerprint canonicalization or Zene v1 hash in core
- HTTP auth, quota, budget, compaction, KV cache lifecycle
- Binding HTTP 409/404 in store API (optional `http` feature may map for the reference router only)

## API compatibility

| Item | Strategy |
| --- | --- |
| `MemorySessionStore::new()` | Unchanged |
| `SessionPrefix` fields | Unchanged; new fields use `#[serde(default)]` later |
| `_session_context` | Unchanged; additive `tail_start` |
| `prefix_hash` | Kept; P1 migrates to `fingerprint` with serde alias |
| Bare `session_id` store methods | Kept; delegate to `DEFAULT_NAMESPACE` |
| `session` / `http` features | Unchanged |

## Acceptance criteria

- [x] Different namespaces with the same `session_id` are isolated.
- [x] Lower epoch publish cannot overwrite higher epoch.
- [x] Same epoch + identical prefix is idempotent (`AlreadyCurrent`).
- [x] Same epoch + different prefix returns `EpochConflict`.
- [x] Concurrent publish tests pass without torn state.
- [x] Delta assembly preserves full raw JSON (tool_calls, multimodal, extra fields).
- [x] Missing `raw_messages` on delta fails explicitly.
- [x] `tail_start` policy is configurable; default is not `ExactPrefixLength`.
- [x] Metadata from ingress is preserved after assembly.
- [x] Bare `session_id` API still compiles and behaves via default namespace.
- [x] No SmartGate/Zene/Project/API Key types in `unigateway-session`.
- [x] Store/middleware errors are stable `SessionError`, not HTTP codes.

## Implementation sketch

1. `unigateway-session/src/store.rs`: `SessionKey`, `PublishResult`, `SessionError`, epoch CAS in `MemorySessionStore`.
2. `unigateway-session/src/middleware.rs`: raw assembly, `TailPositionPolicy`, `SessionMiddlewareConfig`, key resolver.
3. `unigateway-session/src/http.rs`: map reference HTTP statuses from `SessionError` / `PublishResult` only in the optional router.
4. Tests: namespace isolation, epoch matrix, concurrent publish, raw fidelity (tool_calls + array content).
5. Update `unigateway-session/README.md` and link from `docs/design/embedder-neutral-extensions.md` R5 section.

## References

- [`docs/design/embedder-neutral-extensions.md`](../../design/embedder-neutral-extensions.md) — R5 session reference
- [`docs/design/protocol-conversion.md`](../../design/protocol-conversion.md) — `raw_messages` preservation channel
- SmartGate ↔ UniGateway alignment review (2026-08): P0 blocked on raw fidelity + epoch + namespace
