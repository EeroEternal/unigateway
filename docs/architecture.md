## UniGateway Architecture

UniGateway is a lightweight, single-binary LLM gateway. No database, no Redis, no Kubernetes — just a TOML config file and one process.

### Request Processing Pipeline

```
Client Request
    │
    ▼
┌──────────────────────────────────────────────────────┐
│  server.rs — Route Registration                      │
│  /v1/chat/completions   →  openai_chat               │
│  /v1/embeddings         →  openai_embeddings          │
│  /v1/messages           →  anthropic_messages         │
│  /api/admin/*           →  Admin API handlers         │
│  /health, /metrics      →  System endpoints           │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  middleware.rs — Authentication Layer                 │
│  GatewayAuth::try_authenticate(token)                │
│  → Find API Key → check active/quota → rate limit    │
│  → Returns Some(auth) or None (fall through to env)  │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  routing.rs — Provider Routing Layer                 │
│  resolve_providers(service_id, protocol, hint)       │
│    routing_strategy:                                 │
│      "round_robin" → returns 1 provider              │
│      "fallback"    → returns all, sorted by priority │
│      target_hint   → returns the named provider      │
│  Each provider is fully resolved (base_url, api_key) │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  gateway.rs — Thin Handlers                          │
│  Loop over providers:                                │
│    apply model_mapping → call upstream → return      │
│    on error → try next provider (fallback)           │
│                                                      │
│  protocol.rs — Request/response format conversion    │
│    OpenAI ↔ ChatRequest, Anthropic ↔ ChatRequest     │
│    EmbedRequest ↔ OpenAI embeddings format            │
│    Upstream calls via llm-connector                  │
└──────────────────────────────────────────────────────┘
```

### Data Model

All configuration lives in a single TOML file (`unigateway.toml`), loaded into memory at startup, persisted on change.

```
Service ──1:N── Binding(priority) ──N:1── Provider
   │                                        │
   └── routing_strategy                     ├── provider_type (openai, anthropic, …)
       (round_robin | fallback)             ├── endpoint_id, base_url, api_key
                                            └── model_mapping
API Key ──N:1── Service
   └── quota_limit, qps_limit, concurrency_limit
```

- **Service**: A logical downstream-facing unit. Clients don't interact with providers directly; they call a Service via an API Key.
- **Provider**: An upstream LLM endpoint (OpenAI, Anthropic, DeepSeek, etc.) with credentials and optional model mapping.
- **Binding**: Links a Service to a Provider. The `priority` field controls fallback order (lower = tried first).
- **API Key**: A gateway-issued credential bound to a Service, with per-key quota, QPS, and concurrency limits.

### Source File Layout

```
src/
  main.rs          CLI entry point (clap)
  server.rs        HTTP server startup, route registration, background tasks
  config.rs        TOML config file loading, in-memory state, persistence
  types.rs         AppConfig, AppState, shared types

  middleware.rs     Auth lifecycle (GatewayAuth), token extraction, error helpers
  routing.rs        Provider resolution, routing strategy, upstream URL resolution
  gateway.rs        HTTP handlers (openai_chat, anthropic_messages, openai_embeddings)
  protocol.rs       Request/response conversion, llm-connector integration

  cli.rs            CLI subcommands (quickstart, create-service, metrics, …)
  service.rs        Admin API: list/create services
  provider.rs       Admin API: list/create providers, bind to service
  api_key.rs        Admin API: list/create API keys
  system.rs         /health, /metrics, /v1/models endpoints
  storage.rs        Utility functions (hash, model name mapping)
  authz.rs          Admin token authorization
  sdk.rs            Rust SDK client for testing
```

### How the Architecture Adapts to Multiple Scenarios

The same four concepts (Service, Provider, Binding, API Key) cover a wide range of use cases without any code changes — only configuration differs.

#### Single-Provider Direct Proxy

One Service, one Provider, one Binding, one API Key. The simplest setup: put any LLM behind the gateway.

```toml
[[services]]
id = "my-svc"
name = "My LLM"

[[providers]]
name = "openai-main"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.openai.com"
api_key = "sk-..."

[[bindings]]
service_id = "my-svc"
provider_name = "openai-main"

[[api_keys]]
key = "ugk_xxx"
service_id = "my-svc"
```

#### Multi-Provider Load Balancing (Round-Robin)

Bind multiple providers to one service. Requests are distributed across them automatically.

```toml
[[bindings]]
service_id = "my-svc"
provider_name = "openai-a"

[[bindings]]
service_id = "my-svc"
provider_name = "openai-b"
```

#### Primary + Automatic Fallback

Set `routing_strategy = "fallback"` on the service. Providers are tried in `priority` order. If the primary fails (5xx / connection error), the next one is tried automatically within the same request.

```toml
[[services]]
id = "my-svc"
name = "Resilient LLM"
routing_strategy = "fallback"

[[bindings]]
service_id = "my-svc"
provider_name = "primary-openai"
priority = 0

[[bindings]]
service_id = "my-svc"
provider_name = "backup-openai"
priority = 1
```

#### Chat + Embeddings (RAG)

One gateway, one API key, two endpoints. Both `/v1/chat/completions` and `/v1/embeddings` route through the same Service → Provider chain. Use `model_mapping` to map downstream model names to upstream models.

```bash
# Chat
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Hello"}]}'

# Embeddings
curl -X POST http://localhost:3210/v1/embeddings \
  -H "Authorization: Bearer ugk_xxx" \
  -d '{"model":"text-embedding-3-small","input":"Hello world"}'
```

#### Multi-Tenant with Per-Key Limits

Multiple API keys on the same service, each with independent quotas. Useful for team gateways.

```toml
[[api_keys]]
key = "ugk_team_alice"
service_id = "shared-svc"
quota_limit = 1000
qps_limit = 10.0

[[api_keys]]
key = "ugk_team_bob"
service_id = "shared-svc"
quota_limit = 500
qps_limit = 5.0
```

#### Pinning a Specific Provider

Override routing for a single request by specifying the provider via header or body field:

```bash
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -H "x-unigateway-provider: deepseek" \
  -d '{"model":"deepseek-chat","messages":[...]}'
```

### Adding a New Endpoint

To add a new endpoint type (e.g. `/v1/images/generations`):

1. **protocol.rs**: Add request parsing and response formatting functions.
2. **gateway.rs**: Write a handler (~60 lines). Auth, routing, and fallback are inherited for free.
3. **server.rs**: Register one route.

No changes needed in middleware, routing, or config layers.

### Design Principles

- **No external dependencies**: Single binary, TOML config, in-memory state. No database, no message queue, no service mesh.
- **Scenario-driven**: Abstractions are shaped by real use cases (direct proxy, load balancing, fallback, RAG, multi-tenant), not by speculative generality.
- **Layered separation**: Each layer (auth → routing → handler → protocol) has a single responsibility and can be understood independently.
- **Extension without modification**: New endpoints only add code in protocol + gateway + server layers; existing layers remain untouched.
