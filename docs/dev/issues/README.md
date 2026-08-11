# Draft GitHub Issues

English issue bodies for embedder-neutral gateway extensions. Copy each file into a new GitHub issue when ready.

**Full RFC (background, design, roadmap):** [`docs/design/embedder-neutral-extensions.md`](../../design/embedder-neutral-extensions.md)

| File | Suggested title | Priority |
| --- | --- | --- |
| [`01-gateway-fields-upstream-strip.md`](01-gateway-fields-upstream-strip.md) | Gateway-only `_` fields: `gateway_fields` bucket and upstream strip | High |
| [`02-host-middleware-hooks.md`](02-host-middleware-hooks.md) | Host middleware hooks for request/response mutation (opt-in) | Medium |
| [`03-passthrough-example.md`](03-passthrough-example.md) | Production-grade OpenAI passthrough example (streaming + render) | Medium |
| [`04-session-generalization-p0.md`](04-session-generalization-p0.md) | Session P0: raw assembly, SessionKey, epoch CAS, tail policy | High |
| [`05-session-generalization-p1.md`](05-session-generalization-p1.md) | Session P1: fingerprint, size limits, SessionStore trait | High |
| [`06-session-generalization-p2.md`](06-session-generalization-p2.md) | Session P2: TTL, touch, purge, lifecycle hooks | Medium |
| [`07-session-redis.md`](07-session-redis.md) | Optional `unigateway-session-redis` SessionStore crate | Medium |

Related optional work (not listed here): configurable `metadata` → HTTP header forwarding (R3), other external session stores (Postgres in separate crates).

Suggested landing order: **01 → 03 → 02**.
