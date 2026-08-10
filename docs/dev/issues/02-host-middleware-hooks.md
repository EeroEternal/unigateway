# Host middleware hooks for request/response mutation (opt-in)

## Summary

Add an **opt-in middleware chain** in `unigateway-host` for embedders to mutate chat requests and observe or transform responses **without** changing `UniGatewayEngine` or baking product logic into core.

Default: **empty chain** — behavior identical to today.

## Motivation

Core already exposes `GatewayHooks` for **lifecycle observation** (request/attempt started/finished, reports). That is the right layer for telemetry, not for:

- Assembling delta messages from stored session prefixes
- Audit logging or redaction
- Rate limiting or policy gates before upstream dispatch

Embedders today must wire parse → mutate → dispatch manually around host helpers. A documented, stable middleware slot reduces duplication and keeps core free of host-product semantics (see `AGENTS.md`).

## Distinction from `GatewayHooks`

| | `GatewayHooks` (core) | Host middleware (this issue) |
| --- | --- | --- |
| Layer | `unigateway-core` | `unigateway-host` |
| Purpose | Observe execution lifecycle | Mutate request / handle response |
| Default | None | Empty chain |
| Engine awareness | Registered on engine | Engine unaware |

## Proposed API (sketch)

Trait-based chain, feature or module gated if needed:

```rust
/// Runs after protocol parse (and optional session assembly), before `proxy_chat`.
pub trait ChatRequestMiddleware: Send + Sync {
    fn on_chat_request(
        &self,
        ctx: &HostContext<'_>,
        request: &mut ProxyChatRequest,
        gateway_fields: &HashMap<String, Value>,
    ) -> HostFuture<'_, HostResult<()>>;
}

/// Runs after core returns `ProxySession`, before protocol render (optional).
pub trait ChatResponseMiddleware: Send + Sync {
    fn on_chat_response(
        &self,
        ctx: &HostContext<'_>,
        request: &ProxyChatRequest,
        session: &mut ProxySession,
    ) -> HostFuture<'_, HostResult<()>>;
}
```

Embedders register `Vec<Arc<dyn ChatRequestMiddleware>>` (and optionally response middleware) on a host dispatcher wrapper or builder. Exact naming and sync vs async error handling can follow existing `HostFuture` patterns in `unigateway-host/src/host.rs`.

## Execution order (must document)

```text
1. Protocol parse
     -> ProxyChatRequest + gateway_fields (`_` keys; see gateway-fields issue)
2. Optional session / delta assembly (embedder or future reference crate)
3. Host request middleware chain (this issue)
4. Host dispatch -> UniGatewayEngine::proxy_chat
5. Driver upstream merge (gateway_fields never merged)
6. Optional host response middleware chain
7. Protocol render -> ProtocolHttpResponse
```

Request middleware must run **after** session assembly (if any) and **before** driver merge, so middleware sees the final `messages` while `gateway_fields` stays read-only metadata.

## Non-goals

- No Zene / agent-product-specific types or routes in core or default host.
- No mandatory middleware; no change to default dispatch when chain is empty.
- No HTTP framework types (Axum, etc.) in `unigateway-host`.
- Session storage and publish/delete HTTP routes belong in an **optional reference crate**, not required for this issue.

## Acceptance criteria

- [ ] Default host dispatch path unchanged when no middleware registered.
- [ ] Middleware can read `gateway_fields` and mutate `ProxyChatRequest` (e.g. replace `messages`).
- [ ] Middleware runs before `proxy_chat`; integration test proves mutation affects upstream payload.
- [ ] Response middleware optional; at least one test for “observe session before render”.
- [ ] Document pattern in `docs/guide/embedder_patterns.md` with a minimal example (audit log, message injection).
- [ ] Clarify vs `GatewayHooks` in docs.

## Dependencies

- **Recommended after** gateway-only `_` fields + `gateway_fields` bucket (parse split makes middleware ergonomic).
- Can ship in parallel with passthrough example if middleware is not required for the example’s first iteration.

## Related

- Gateway-only fields: `docs/dev/issues/01-gateway-fields-upstream-strip.md`
- Production passthrough example: `docs/dev/issues/03-passthrough-example.md`
