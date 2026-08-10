# Gateway-only `_` fields: `gateway_fields` bucket and upstream strip

## Summary

Introduce a **gateway-only field convention** for top-level JSON keys prefixed with a single underscore (`_`), and stop forwarding those keys to upstream providers. Parse them into a dedicated `gateway_fields` bucket on `ProxyChatRequest` (and symmetric paths) so embedder middleware can read gateway-internal metadata without re-parsing raw JSON.

This is a **protocol-layer fix**, not vendor-specific logic. Default behavior for requests without `_`-prefixed fields is unchanged.

## Problem

Today, unknown top-level fields from client JSON are collected into `ProxyChatRequest.extra` during parsing (`openai_chat_extra` / `anthropic_chat_extra` in `unigateway-protocol/src/requests.rs`). OpenAI and Anthropic drivers then merge all of `extra` into the upstream HTTP body (`unigateway-core/src/protocol/openai/requests.rs`, `anthropic/requests.rs`).

Embedders that attach gateway-internal metadata in the request body (for example `_session_context`, `_trace`, `_acme_session`) accidentally forward it to upstream APIs. That is incorrect for OpenAI-compatible providers and can cause rejection or silent pollution of vendor payloads.

## Proposed behavior

### R1 — Do not forward `_`-prefixed keys upstream

At upstream payload construction (OpenAI driver, Anthropic driver, and any shared merge point):

- Skip top-level keys whose names start with `_` (single underscore prefix; common “private extension” convention).
- Apply the same rule when collecting `extra` during parse, so `_` keys never enter the upstream merge path via `extra`.

Known standard OpenAI / Anthropic fields are unaffected.

### R2 — Add `gateway_fields` on proxy request types

In `openai_payload_to_chat_request` and the symmetric Anthropic path:

- Route `_`-prefixed top-level fields into `ProxyChatRequest.gateway_fields: HashMap<String, Value>` (name open to bikeshedding; default empty).
- Do **not** place them in `extra`.
- Expose read-only access for host / embedder middleware.
- Drivers **ignore** `gateway_fields` when building upstream requests.

### Recommended request pipeline (document)

```text
parse client JSON
  -> gateway_fields (read-only, `_` keys)
  -> optional session / delta assembly (embedder or future reference middleware)
  -> optional host middleware (see issue #…)
  -> core.proxy_chat
  -> driver merge (R1 strip is the final safety net)
```

Middleware that mutates `messages` for delta assembly should run **after** parse (so `gateway_fields` is available) and **before** `proxy_chat` / driver merge.

## Where this helps (embedder topology)

| Hop | Benefit |
| --- | --- |
| Embedder HTTP server → upstream (typical gateway) | **Primary.** Parse strips `_` from upstream path; middleware reads `gateway_fields`. |
| Client sends `_` fields in JSON body to gateway | Already workable at ingress; R1+R2 fix the forward-to-upstream leak. |
| Embedder uses in-process `UniGatewayEngine` directly to upstream | R1 also protects driver merge on that hop. |
| Client is a plain HTTP client (no core driver) | Unaffected; no driver merge involved. |

## API / semver

- Additive field on `ProxyChatRequest` (and Responses if applicable): **minor** semver bump.
- **Behavior change:** `_`-prefixed top-level fields are no longer forwarded upstream. Document in changelog. Intentionally forwarding `_foo` to upstream was never valid OpenAI semantics and is unlikely in practice.

## Non-goals

- Do not define semantics for any specific `_` key (no `zene`, `epoch`, `publish` hardcoding).
- Do not change handling of known standard fields.
- Do not require embedders to use `_` fields.

## Acceptance criteria

- [ ] Request `{"model":"…","messages":[…],"_foo":{"bar":1}}` produces an upstream HTTP body **without** `_foo`.
- [ ] `request.gateway_fields["_foo"]` is available after parse for middleware.
- [ ] Requests with no `_` fields: existing tests and embedder behavior unchanged.
- [ ] Anthropic parse path symmetric with OpenAI.
- [ ] Unit tests for parse + driver merge; regression test that `extra` still forwards vendor keys like `reasoning_effort`.
- [ ] Update `docs/design/protocol-conversion.md` (neutral model + preservation channels).

## Implementation sketch

1. `unigateway-protocol/src/requests.rs`: split `openai_chat_extra` into vendor `extra` vs `gateway_fields`.
2. `unigateway-core/src/request.rs`: add `gateway_fields` with `Default`.
3. `unigateway-core/src/protocol/{openai,anthropic}/requests.rs`: defensive skip of `_` keys if any remain in `extra`.
4. Touch call sites / tests that construct `ProxyChatRequest` manually.

## Related (out of scope for this issue)

- Host middleware hooks (separate issue).
- Optional session prefix store / HTTP routes (optional reference implementation).
- Configurable `ProxyChatRequest.metadata` → outbound HTTP headers.
