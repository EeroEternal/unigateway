## UniGateway Architecture

UniGateway is a lightweight, single-binary LLM gateway designed for individual developers and AI power users. It provides a unified, stable entry point for multiple AI tools (like Cursor, Claude Code, and OpenClaw) and upstream providers.

### Product Positioning

UniGateway is a **local-first, tool-friendly, unified access, and multi-upstream switchable model entry layer**. It prioritizes:

1. **Unified Entry Point**: One base URL for all your tools.
2. **Mode Switching**: Abstracting providers into logical modes like `default`, `fast`, or `strong`.
3. **Stable Fallback**: Automatic upstream failover to minimize workflow interruptions.
4. **Low Friction**: extremely fast installation and configuration.
5. **Observability**: Clear diagnostics on where requests are going and why they fail.

### User Mental Model

While internally UniGateway manages services and providers, the external interaction is simplified:

`tool -> mode -> route -> upstream`

- **Modes**: The core abstraction representing access intent (e.g., `default`).
- **Upstreams**: Real providers (OpenAI, Anthropic, DeepSeek, etc.).
- **Integrations**: Templates for tools like Cursor, Zed, and Claude Code.

### Request Processing Pipeline

```
Client Request (via Mode API Key)
    │
    ▼
┌──────────────────────────────────────────────────────┐
│  server.rs — Route Registration                      │
│  /v1/chat/completions   →  openai_chat               │
│  /v1/embeddings         →  openai_embeddings          │
│  /v1/messages           →  anthropic_messages         │
│  /api/admin/*           →  Admin API handlers         │
│  STDIO (MCP)           →  Admin & Proc tools         │
│  /health, /metrics      │  System endpoints           │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  mcp.rs — Process & Admin Protocol Toolset            │
│  server_start() / server_stop() / server_status()    │
│  get_config() / set_config()                         │
│  → Manage background `ug` daemon via MCP tools       │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  middleware.rs — Authentication Layer                 │
│  GatewayAuth::try_authenticate(token)                │
│  → Find API Key → Resolve Mode (Service)             │
│  → Check active/quota → rate limit                   │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  routing.rs — Provider Routing Layer                 │
│  resolve_providers(service_id, protocol, hint)       │
│    routing_strategy:                                 │
│      "round_robin" → returns 1 provider              │
│      "fallback"    → returns all, sorted by priority │
└───────────────────┬──────────────────────────────────┘
                    ▼
┌──────────────────────────────────────────────────────┐
│  gateway.rs — Thin Handlers                          │
│  Loop over providers:                                │
│    apply model_mapping → call upstream → return      │
│    on error → try next provider (fallback)           │
│                                                      │
│  protocol.rs — Format Conversion                     │
│    OpenAI ↔ ChatRequest, Anthropic ↔ ChatRequest     │
│    Upstream calls via llm-connector                  │
└──────────────────────────────────────────────────────┘
```

### Data Model

Configuration lives in `unigateway.toml`.

- **Service (Mode)**: A logical downstream unit.
- **Provider (Upstream)**: An upstream LLM endpoint.
- **Binding (Route)**: Links a Service to a Provider with a `priority`.
- **API Key**: A credential bound to a Service/Mode.

### Source File Layout

```
src/
  main.rs          CLI entry point & setup logic
  server.rs        HTTP server & route registration
  config/          Configuration schema, store, and admin Handlers
  gateway/         Chat and streaming handlers
  protocol/        Request/response conversion & client logic
  cli/             Modes, integrations, diagnostics, and rendering
  routing.rs       Provider resolution & fallback logic
  system.rs        Health, metrics, and models endpoints
```

### Design Principles

- **No external dependencies**: Single binary, TOML config.
- **Scenario-driven**: Abstractions shaped by real developer needs.
- **Layered separation**: Clear boundaries between auth, routing, and protocol.
- **Explainable Routing**: Users can always know why a specific upstream was chosen.
tp://localhost:3210/v1/chat/completions \
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
