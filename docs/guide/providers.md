## Provider Configuration Examples

Real-world examples of routing different LLM providers through UniGateway. All examples use the OpenAI-compatible downstream API (`/v1/chat/completions`, `/v1/embeddings`).

---

### DeepSeek

DeepSeek exposes an OpenAI-compatible API at `https://api.deepseek.com`.

```toml
[[providers]]
name = "deepseek"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.deepseek.com"
api_key = "sk-your-deepseek-key"
```

Call via gateway:

```bash
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

---

### Groq

Groq provides an OpenAI-compatible endpoint at `https://api.groq.com/openai`.

```toml
[[providers]]
name = "groq"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.groq.com/openai"
api_key = "gsk_your-groq-key"
```

Call via gateway:

```bash
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -d '{"model": "llama-3.3-70b-versatile", "messages": [{"role": "user", "content": "Hello"}]}'
```

---

### OpenAI

```toml
[[providers]]
name = "openai"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.openai.com"
api_key = "sk-your-openai-key"
```

---

### Anthropic

Anthropic uses a different downstream protocol (`/v1/messages`). Set `provider_type = "anthropic"`.

```toml
[[providers]]
name = "anthropic"
provider_type = "anthropic"
endpoint_id = ""
base_url = "https://api.anthropic.com"
api_key = "sk-ant-your-key"
```

Call via gateway:

```bash
curl -X POST http://localhost:3210/v1/messages \
  -H "x-api-key: ugk_xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

---

### MiniMax (via llm_providers endpoint registry)

When a provider is registered in `llm_providers`, you can use `endpoint_id` instead of `base_url`. The gateway resolves the URL automatically.

```toml
[[providers]]
name = "minimax"
provider_type = "openai"
endpoint_id = "minimax:global"
base_url = ""
api_key = "sk-your-minimax-key"
```

---

### Scenario: DeepSeek + Groq Fallback

Primary traffic goes to DeepSeek (cheap). If DeepSeek is down, requests automatically fall back to Groq.

```toml
[[services]]
id = "chat-svc"
name = "Chat with Fallback"
routing_strategy = "fallback"

[[providers]]
name = "deepseek"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.deepseek.com"
api_key = "sk-deepseek-key"

[[providers]]
name = "groq"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.groq.com/openai"
api_key = "gsk_groq-key"

[[bindings]]
service_id = "chat-svc"
provider_name = "deepseek"
priority = 0

[[bindings]]
service_id = "chat-svc"
provider_name = "groq"
priority = 1

[[api_keys]]
key = "ugk_fallback_demo"
service_id = "chat-svc"
```

A single request to the gateway tries DeepSeek first; on 5xx or connection failure, it retries on Groq transparently.

---

### Scenario: Chat + Embeddings for RAG

One service handles both chat completions and embeddings. Use `model_mapping` to route embedding model names to the upstream model.

```toml
[[services]]
id = "rag-svc"
name = "RAG Service"

[[providers]]
name = "openai-rag"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.openai.com"
api_key = "sk-openai-key"
model_mapping = '{"chat-model": "gpt-4o", "embed-model": "text-embedding-3-small"}'

[[bindings]]
service_id = "rag-svc"
provider_name = "openai-rag"

[[api_keys]]
key = "ugk_rag_demo"
service_id = "rag-svc"
```

```bash
# Chat
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_rag_demo" \
  -d '{"model": "chat-model", "messages": [{"role": "user", "content": "Summarize this document"}]}'

# Embeddings
curl -X POST http://localhost:3210/v1/embeddings \
  -H "Authorization: Bearer ugk_rag_demo" \
  -d '{"model": "embed-model", "input": "document text here"}'
```

Both calls go through the same gateway key, the same service, and the same provider — `model_mapping` translates the logical model name to the actual upstream model.

---

### Scenario: Multi-Provider Round-Robin

Distribute traffic evenly across two OpenAI-compatible providers.

```toml
[[services]]
id = "lb-svc"
name = "Load Balanced"

[[providers]]
name = "provider-a"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.provider-a.com"
api_key = "key-a"

[[providers]]
name = "provider-b"
provider_type = "openai"
endpoint_id = ""
base_url = "https://api.provider-b.com"
api_key = "key-b"

[[bindings]]
service_id = "lb-svc"
provider_name = "provider-a"

[[bindings]]
service_id = "lb-svc"
provider_name = "provider-b"

[[api_keys]]
key = "ugk_lb_demo"
service_id = "lb-svc"
```

No `routing_strategy` needed — `round_robin` is the default. Requests alternate between provider-a and provider-b.
