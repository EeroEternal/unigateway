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
| `unigateway.sglang_lite.backend_mode` | Backend mode: `http` (default), `subprocess`, or `grpc` (requires `sglang-lite-grpc` feature) | `http` |
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

UniGateway forwards `cache_hit_tokens` in two ways:

* In the wire response for OpenAI-compatible chat completions (passthrough of sglang-lite's JSON, including in streaming final usage chunks when emitted by the engine). Clients see the same field.
* Parsed into `RequestReport.usage.cache_hit_tokens` for internal hooks, retry decisions, and metrics collectors.

```rust
if let Some(usage) = &report.usage {
    println!("cache_hit_tokens: {:?}", usage.cache_hit_tokens);
}
```

This enables observing Radix prefix cache efficiency end-to-end without custom parsing.

## gRPC mode (optional / P2)

gRPC is available as an **experimental skeleton** behind the `sglang-lite-grpc` Cargo feature (which implies `sglang-lite`). When the feature is disabled, `backend_mode = "grpc"` returns `not_implemented`.

### Enable the feature

In your `Cargo.toml`:

```toml
[dependencies]
unigateway-sdk = { version = "2.6", default-features = false, features = ["host", "sglang-lite-grpc"] }
# or directly:
unigateway-core = { version = "2.6", features = ["sglang-lite-grpc"] }
```

### Configure a gRPC endpoint

```toml
[[providers]]
name = "my-moe-grpc"
provider_type = "sglang-lite"
base_url = "http://127.0.0.1:50051"
api_key = ""

[providers.model_policy]
default_model = "local-moe"

[providers.metadata]
"unigateway.sglang_lite.backend_mode" = "grpc"
"unigateway.sglang_lite.model_path" = "/path/to/moe"
"unigateway.sglang_lite.device" = "cuda"
```

To spawn the sglang-lite gRPC server as a subprocess, add the `subprocess.*` keys:

```toml
[providers.metadata]
"unigateway.sglang_lite.backend_mode" = "grpc"
"unigateway.sglang_lite.subprocess.command" = "python"
"unigateway.sglang_lite.subprocess.args" = "-m sglang_lite.grpc_server --port 50051"
"unigateway.sglang_lite.subprocess.startup_timeout_ms" = "30000"
```

The driver will wait for the standard `grpc.health.v1.Health` service to report `SERVING` before routing the first request.

### Confirmed contract

The API contract between UniGateway and sglang-lite is defined in the **sglang-lite** repository:

- [proto/sglang_lite.proto](https://github.com/EeroEternal/sglang-lite/blob/main/proto/sglang_lite.proto)
- [docs/sglang-lite-grpc-spec.md](https://github.com/EeroEternal/sglang-lite/blob/main/docs/sglang-lite-grpc-spec.md)

Summary:
- Service: `sglang_lite.SglangLiteService`
  - `ChatCompletions` (unary)
  - `ChatCompletionsStream` (server streaming)
  - `Embeddings`
  - `ListModels`
- Standard `grpc.health.v1.Health` for readiness (SERVING after model load).
- `Usage` includes `cache_hit_tokens`.
- Default listen port: 50051
- Local use: no TLS, empty auth ok.
- Error handling via standard gRPC status codes (UNAVAILABLE, INVALID_ARGUMENT, UNIMPLEMENTED...).

### Current status in UniGateway
- Metadata key `unigateway.sglang_lite.backend_mode=grpc` is parsed.
- With the `sglang-lite-grpc` feature, the driver connects via tonic and supports:
  - unary + full streaming with completion handle
  - basic tools / tool_calls / tool_choice (proto extended)
  - subprocess start + standard gRPC Health RPC wait (when `subprocess.*` metadata present)
  - `cache_hit_tokens` passthrough
- See `unigateway-core/src/protocol/sglang_lite/grpc.rs` for the client skeleton.

For production today, prefer `http` or `subprocess` modes (both go over the OpenAI-compatible HTTP surface). Use gRPC for experiments or local latency-sensitive paths once the sglang-lite server side is fully ready.
