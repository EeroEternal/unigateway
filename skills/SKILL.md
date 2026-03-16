---
name: unigateway
version: 0.7.7
description: >
  Set up and manage UniGateway — a unified LLM gateway that proxies
  OpenAI, Anthropic, DeepSeek, Groq, MiniMax, and any OpenAI-compatible
  provider behind a single API with routing, fallback, rate limiting,
  and embeddings support.
---

# UniGateway Skill

UniGateway (`ug`) is a single-binary LLM gateway. It sits between your
application and upstream LLM providers, exposing unified API endpoints:

| Endpoint | Protocol | Description |
|----------|----------|-------------|
| `POST /v1/chat/completions` | OpenAI | Chat completions (streaming supported) |
| `POST /v1/messages` | Anthropic | Messages API |
| `POST /v1/embeddings` | OpenAI | Text embeddings |
| `GET /v1/models` | OpenAI | List available models |

Supported upstream providers (any OpenAI-compatible API works):
**OpenAI**, **Anthropic**, **DeepSeek**, **Groq**, **MiniMax**, **Ollama**,
**Azure OpenAI**, **Together AI**, **Fireworks**, **OpenRouter**, and more.

---

## 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/EeroEternal/unigateway/main/install.sh | sh
```

Alternatives:
```bash
brew install EeroEternal/tap/ug
cargo install unigateway
```

## 2. Quickstart (interactive)

```bash
ug guide
```

Walks through: provider type → model → base URL → API key.
Creates config, service, provider, binding, and gateway API key automatically.
`ug guide` is an alias for `ug quickstart`.
Then start:

```bash
ug serve
```

### Non-interactive (for scripts / AI agents)

```bash
ug quickstart \
  --provider-type openai \
  --endpoint-id gpt-4o \
  --api-key "sk-..."
```

## 3. Core Concepts

- **Service**: A logical unit your app talks to. Has a `routing_strategy` (`round_robin` or `fallback`).
- **Provider**: An upstream LLM endpoint (type, URL, API key, optional `model_mapping`).
- **Binding**: Links a Provider to a Service. Has `priority` (for fallback ordering).
- **API Key**: A gateway credential bound to a Service. Has `quota_limit`, `qps_limit`, `concurrency_limit`.

## 4. Provider Examples

### OpenAI

```bash
ug quickstart --provider-type openai --endpoint-id gpt-4o --api-key sk-xxx
```

### Anthropic

```bash
ug quickstart --provider-type anthropic --endpoint-id claude-sonnet-4-20250514 --api-key sk-ant-xxx
```

### DeepSeek

```bash
ug quickstart --provider-type openai --endpoint-id deepseek-chat \
  --base-url https://api.deepseek.com --api-key sk-deepseek-xxx
```

### Groq

```bash
ug quickstart --provider-type openai --endpoint-id llama-3.3-70b-versatile \
  --base-url https://api.groq.com/openai --api-key gsk_xxx
```

### Ollama (local)

```bash
ug quickstart --provider-type openai --endpoint-id llama3 \
  --base-url http://localhost:11434 --api-key unused
```

## 5. Advanced Scenarios

### Multi-provider round-robin

Distribute traffic across providers:

```bash
ug create-service --id lb-svc --name "Load Balanced"
ug create-provider --name provider-a --provider-type openai --endpoint-id gpt-4o \
  --base-url https://api.openai.com --api-key sk-a
ug create-provider --name provider-b --provider-type openai --endpoint-id gpt-4o \
  --base-url https://api.openai.com --api-key sk-b
ug bind-provider --service-id lb-svc --provider-id 0
ug bind-provider --service-id lb-svc --provider-id 1
ug create-api-key --key ugk_lb --service-id lb-svc
```

### Fallback routing

Primary provider with automatic failover:

Edit config (`ug config edit`):

```toml
[[services]]
id = "ha-svc"
name = "High Availability"
routing_strategy = "fallback"

[[providers]]
name = "deepseek"
provider_type = "openai"
base_url = "https://api.deepseek.com"
api_key = "sk-deepseek"

[[providers]]
name = "groq"
provider_type = "openai"
base_url = "https://api.groq.com/openai"
api_key = "gsk_groq"

[[bindings]]
service_id = "ha-svc"
provider_name = "deepseek"
priority = 0

[[bindings]]
service_id = "ha-svc"
provider_name = "groq"
priority = 1
```

On 5xx or connection failure, the request automatically retries on the next provider.

### Embeddings (RAG)

Use `model_mapping` to route both chat and embedding models:

```toml
[[providers]]
name = "openai-rag"
provider_type = "openai"
base_url = "https://api.openai.com"
api_key = "sk-xxx"
model_mapping = '{"chat": "gpt-4o", "embed": "text-embedding-3-small"}'
```

```bash
# Chat
curl http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -d '{"model":"chat","messages":[{"role":"user","content":"Hi"}]}'

# Embeddings
curl http://localhost:3210/v1/embeddings \
  -H "Authorization: Bearer ugk_xxx" \
  -d '{"model":"embed","input":"document text"}'
```

### Provider pinning

Route a specific request to a specific provider by header:

```bash
curl http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -H "x-unigateway-provider: deepseek" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Hi"}]}'
```

### Rate limiting

```bash
ug create-api-key --key ugk_limited --service-id default \
  --quota-limit 10000 --qps-limit 20 --concurrency-limit 5
```

## 6. Config Management

```bash
ug config path     # print config file location
ug config show     # print current config
ug config edit     # open in $EDITOR
```

Default location: `~/.config/unigateway/config.toml` (macOS: `~/Library/Application Support/unigateway/config.toml`).

## 7. CLI Reference

| Command | Description |
|---------|-------------|
| `ug` | Start gateway in background (alias for `ug serve`) |
| `ug serve` | Start gateway in background with options (`--bind`, `--config`) |
| `ug status` | Check if the gateway is running |
| `ug stop` | Stop the background gateway |
| `ug logs` | View the background gateway logs (`-f` to tail) |
| `ug guide` | Interactive setup wizard (alias for `quickstart`) |
| `ug quickstart` | Interactive setup wizard |
| `ug create-service` | Create or update a service |
| `ug create-provider` | Create or update a provider |
| `ug bind-provider` | Bind provider to service |
| `ug create-api-key` | Create or update an API key |
| `ug metrics` | Print request counters |
| `ug config path/show/edit` | Manage config file |
| `ug mcp` | Start MCP server over stdio |

## 8. Admin API

All admin endpoints require `x-admin-token` header if `UNIGATEWAY_ADMIN_TOKEN` is set.

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/admin/services` | List services |
| POST | `/api/admin/services` | Create service |
| GET | `/api/admin/providers` | List providers |
| POST | `/api/admin/providers` | Create provider |
| POST | `/api/admin/bindings` | Bind provider to service |
| GET | `/api/admin/api-keys` | List API keys |
| POST | `/api/admin/api-keys` | Create API key |

## 9. Observability

```bash
# Health check
curl http://localhost:3210/health

# Prometheus-style metrics
curl http://localhost:3210/metrics
```

## 10. MCP Server

`ug mcp` starts a Model Context Protocol server over stdio. AI assistants (Cursor, Claude Desktop, etc.) can manage the gateway via natural language.

```bash
ug mcp                           # default config
ug mcp --config /path/to/config  # custom config
```

Exposed tools: `list_services`, `create_service`, `list_providers`, `create_provider`, `bind_provider`, `list_api_keys`, `create_api_key`, `show_config`, `get_metrics`.

Cursor / Claude Desktop config:

```json
{
  "mcpServers": {
    "unigateway": {
      "command": "ug",
      "args": ["mcp"]
    }
  }
}
```
