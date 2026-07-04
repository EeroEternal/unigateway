# sglang-lite

[sglang-lite](https://github.com/sglang/sglang-lite) is a local Mixture-of-Experts (MoE) inference backend. UniGateway treats it as an OpenAI-compatible provider with extra local-engine metadata and cache-hit metrics.

## HTTP mode

Start sglang-lite as a standalone OpenAI-compatible server:

```bash
python -m sglang_lite.server \
  --port 8000 \
  --model "deepseek-ai/DeepSeek-V2-Lite-Chat" \
  --device cuda \
  --max-batch-size 8
```

Then point UniGateway at it:

```toml
[[providers]]
name = "my-moe"
provider_type = "sglang-lite"
base_url = "http://localhost:8000/v1"
# api_key can be empty for a local backend
api_key = ""

[providers.model_policy]
default_model = "local-moe"
```

Call via gateway:

```bash
curl -X POST http://localhost:3210/v1/chat/completions \
  -H "Authorization: Bearer ugk_xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "local-moe",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Subprocess mode

Instead of running sglang-lite separately, UniGateway can start it as a child process for you.

```toml
[[providers]]
name = "my-moe"
provider_type = "sglang-lite"
base_url = "http://localhost:8000"
api_key = ""

[providers.model_policy]
default_model = "local-moe"

[providers.metadata]
"unigateway.sglang_lite.backend_mode" = "subprocess"
"unigateway.sglang_lite.subprocess.command" = "python"
"unigateway.sglang_lite.subprocess.args" = "-m sglang_lite.server --port 8000"
"unigateway.sglang_lite.subprocess.startup_timeout_ms" = "30000"
"unigateway.sglang_lite.subprocess.health_path" = "health"
```

The first request to the provider will spawn the subprocess and wait for its HTTP health endpoint to become ready. Subsequent requests reuse the same process.

## Metadata keys

| Key | Meaning | Default |
|---|---|---|
| `unigateway.sglang_lite.backend_mode` | Backend mode: `http`, `subprocess`, or `grpc` | `http` |
| `unigateway.sglang_lite.model_path` | Local model path or identifier | none |
| `unigateway.sglang_lite.device` | Target device, e.g. `cuda` or `cpu` | none |
| `unigateway.sglang_lite.max_batch_size` | Maximum batch size for the scheduler | none |
| `unigateway.sglang_lite.python_env` | Python interpreter / environment path | none |
| `unigateway.sglang_lite.subprocess.command` | Executable to spawn in subprocess mode | required in subprocess mode |
| `unigateway.sglang_lite.subprocess.args` | Space-separated arguments for the command | none |
| `unigateway.sglang_lite.subprocess.startup_timeout_ms` | Max wait for the subprocess health check | `30000` |
| `unigateway.sglang_lite.subprocess.health_path` | Health check path relative to `base_url` | `health` |

## sglang-lite environment variables

When running sglang-lite directly, its own `Config.from_env` reads these environment variables:

- `SGLANG_LITE_MODEL`
- `SGLANG_LITE_DEVICE`
- `SGLANG_LITE_PORT`
- `SGLANG_LITE_MAX_BATCH_SIZE`
- `SGLANG_LITE_MAX_CONCURRENT`
- `SGLANG_LITE_REQUEST_TIMEOUT`
- `SGLANG_LITE_LOG_LEVEL`

## Cache hit metrics

sglang-lite returns an OpenAI-compatible `usage` object with an extra `cache_hit_tokens` field:

```json
{
  "usage": {
    "prompt_tokens": 20,
    "completion_tokens": 10,
    "total_tokens": 30,
    "cache_hit_tokens": 12
  }
}
```

UniGateway parses this value into `RequestReport.usage.cache_hit_tokens` so hooks and metrics collectors can observe prefix-cache efficiency without provider-specific parsing.

```rust
if let Some(usage) = &report.usage {
    println!("cache_hit_tokens: {:?}", usage.cache_hit_tokens);
}
```
