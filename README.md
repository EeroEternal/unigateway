<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>Scenario-oriented LLM gateway with OpenAI and Anthropic compatibility.</strong>
  </p>
  <p>
    Rich CLI + JSON admin API, single binary. No Web UI. Install as a <strong>Skill</strong> in Codex/Cursor for one-shot init.
  </p>
  <p>
    <a href="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml"><img src="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway"><img src="https://img.shields.io/crates/v/unigateway.svg" alt="Crate"></a>
    <a href="https://github.com/EeroEternal/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

## Features

- **Unified API**: `POST /v1/chat/completions` (OpenAI), `POST /v1/messages` (Anthropic)
- **CLI**: `ug serve`, `ug quickstart`, `ug metrics`, `ug create-service`, `ug create-provider`, `ug bind-provider`, `ug create-api-key`
- **Config file**: single TOML (`unigateway.toml`); in-memory state, persisted on change (no DB)
- **Routing**: round-robin load balancing, fallback (priority-based retry on upstream failure); optional `x-target-vendor` / `x-unigateway-provider` header to pin a provider
- **Embeddings**: `POST /v1/embeddings` (OpenAI-compatible)
- **API Key**: quota / QPS / concurrency limits per key
- **In-memory stats**: request counts; `GET /health`, `GET /metrics`, `GET /v1/models`
- **Admin API**: `/api/admin/*` (optional `x-admin-token`)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/EeroEternal/unigateway/main/install.sh | sh
```

Or via Homebrew / Cargo / source:

```bash
brew install EeroEternal/tap/ug          # macOS (Homebrew)
cargo install unigateway                 # Rust toolchain
git clone https://github.com/EeroEternal/unigateway.git && cd unigateway && cargo build --release  # from source
```

## Usage

### Quick start (single provider)

One command creates a service, provider, binding, and API key; prints the key. Then start the gateway. Config defaults to `unigateway.toml` (override with `UNIGATEWAY_CONFIG` or `--config`).

```bash
ug quickstart --provider-type openai --endpoint-id openai --base-url https://api.openai.com --api-key "sk-..."
# Copy the printed key (ugk_...), then:
ug serve
```

Optional: `--service-id`, `--service-name`, `--provider-name`, `--config`.

### Manual setup

All commands default to config file `unigateway.toml`; use `--config <path>` or `UNIGATEWAY_CONFIG` to override.

```bash
# Start gateway (no subcommand = serve)
ug
# or with options:
ug serve --bind 127.0.0.1:3210

# Print metrics (in-memory counts; 0 if server not running)
ug metrics

# Create service → provider → bind → create API key (use provider_id from create-provider output)
ug create-service --id svc_openai --name "OpenAI"
ug create-provider --name openai-prod --provider-type openai --endpoint-id openai --base-url https://api.openai.com --api-key sk-xxx
ug bind-provider --service-id svc_openai --provider-id 0
ug create-api-key --key ugk_xxx --service-id svc_openai --qps-limit 20 --concurrency-limit 8
```

**Multi-provider round-robin**: bind multiple providers to the same service; traffic is round-robin across them.

## Config

- **File**: `~/.config/unigateway/config.toml` (auto-created on first write). Override with `--config <path>` or `UNIGATEWAY_CONFIG` env.
- **Env**:

| Variable | Default | Description |
|----------|---------|-------------|
| `UNIGATEWAY_BIND` | `127.0.0.1:3210` | Bind address |
| `UNIGATEWAY_CONFIG` | `~/.config/unigateway/config.toml` | Config file path |
| `UNIGATEWAY_ADMIN_TOKEN` | `""` | Admin API auth (`x-admin-token`) |

## API overview

- **OpenAI**: `POST /v1/chat/completions`, `Authorization: Bearer <key>`. Optional: `x-target-vendor` or `x-unigateway-provider` (e.g. `minimax`) to route to a specific provider.
- **Anthropic**: `POST /v1/messages`, `x-api-key`, `anthropic-version: 2023-06-01`
- **Admin**: `GET/POST /api/admin/services`, `GET/POST /api/admin/providers`, `POST /api/admin/bindings`, `GET/POST /api/admin/api-keys`

## License

MIT. See [LICENSE](LICENSE).

## About

Author: [EeroEternal](https://github.com/EeroEternal) · songmqq@proton.me
