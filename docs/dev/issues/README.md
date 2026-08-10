# Draft GitHub Issues

English issue bodies for embedder-neutral gateway extensions. Copy each file into a new GitHub issue when ready.

**Full RFC (background, design, roadmap):** [`docs/design/embedder-neutral-extensions.md`](../../design/embedder-neutral-extensions.md)

| File | Suggested title | Priority |
| --- | --- | --- |
| [`01-gateway-fields-upstream-strip.md`](01-gateway-fields-upstream-strip.md) | Gateway-only `_` fields: `gateway_fields` bucket and upstream strip | High |
| [`02-host-middleware-hooks.md`](02-host-middleware-hooks.md) | Host middleware hooks for request/response mutation (opt-in) | Medium |
| [`03-passthrough-example.md`](03-passthrough-example.md) | Production-grade OpenAI passthrough example (streaming + render) | Medium |

Related optional work (not listed here): configurable `metadata` → HTTP header forwarding (R3), session prefix middleware as a reference crate (R5).

Suggested landing order: **01 → 03 → 02**.
