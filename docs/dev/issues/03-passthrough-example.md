# Production-grade OpenAI passthrough example (streaming + render)

## Summary

Upgrade `unigateway-sdk/examples/openai_passthrough.rs` from a **debug stub** into a **copy-paste-ready** minimal HTTP gateway that demonstrates the full embedder path: Axum ingress → protocol parse → host dispatch → core proxy → protocol render → Axum response (JSON and SSE).

No vendor-specific logic. Default features only (`host` + built-in drivers).

## Problem

The current example:

- Parses with `openai_payload_to_chat_request` and calls `engine.proxy_chat` directly.
- Returns a debug JSON blob (`status`, `extra_fields_received`, `streaming`) instead of a real OpenAI Chat Completions response.
- Does not use `unigateway-host` dispatch or `render_openai_chat_session`.
- Does not demonstrate `ProtocolHttpResponse` → Axum conversion for **streaming** (`stream: true`).

New embedders lack a canonical template for production wiring, especially SSE.

## Proposed example behavior

### Routes

- `POST /v1/chat/completions` — OpenAI-compatible chat proxy.

### Flow

```text
Axum handler
  -> Json<Value> body
  -> openai_payload_to_chat_request(&payload, default_model)
  -> HostContext + pool lookup (simple in-memory pool, as today)
  -> unigateway-host openai chat dispatch
       OR engine.proxy_chat + render_openai_chat_session (document chosen path)
  -> ProtocolHttpResponse
  -> axum IntoResponse (JSON or SSE)
```

### Requirements

| Case | Expected |
| --- | --- |
| `stream: false` | `200` + OpenAI-shaped JSON body |
| `stream: true` | `200` + `text/event-stream` SSE |
| Upstream errors | Propagate sensible HTTP status / error JSON (match host error mapping where possible) |
| Env config | Keep `UPSTREAM_BASE_URL`, `UPSTREAM_API_KEY`, `UPSTREAM_MODEL`, `BIND_ADDR` |

### Axum adapter

Add a small, documented helper in the example (or `unigateway-protocol` if already appropriate) to map:

```rust
ProtocolHttpResponse -> axum::response::Response
  ProtocolResponseBody::Json -> Json + status
  ProtocolResponseBody::ServerSentEvents -> SSE stream + status
```

Prefer keeping Axum out of library crates; inline helper in the example is fine.

## Non-goals

- No auth, admin API, config file loading, or multi-pool routing (see `embedder_patterns.md` for production patterns).
- No middleware / session feature (optional follow-up once those issues land).
- No new `[[bin]]` in workspace root; remain `cargo run -p unigateway-sdk --example openai_passthrough`.

## Acceptance criteria

- [ ] `curl` non-streaming chat completion returns valid OpenAI JSON (choices, usage when present).
- [ ] `curl -N` with `"stream": true` receives SSE chunks until `[DONE]` or protocol equivalent.
- [ ] Example uses `render_openai_chat_session` (or host dispatch that renders internally).
- [ ] README / doc comment at top of example: env vars, mock upstream tip (local OpenAI mock + `UPSTREAM_BASE_URL`).
- [ ] `cargo run -p unigateway-sdk --example openai_passthrough` builds with default features.

## Testing

- Manual smoke with mock upstream is sufficient for the example.
- If cheap: extend existing host rendering tests rather than adding network-dependent example tests.

## Related

- Architecture: `docs/design/arch.md`, `docs/design/protocol-conversion.md`
- Embed guide: `docs/guide/embed.md`
- Gateway-only fields (optional enhancement to example comments once landed): `docs/dev/issues/01-gateway-fields-upstream-strip.md`
- Host middleware (optional second example later): `docs/dev/issues/02-host-middleware-hooks.md`

## Priority note

High value for all embedders; can land **in parallel with or shortly after** gateway-fields issue. Does not block R1+R2.
