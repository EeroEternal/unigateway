<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>Unified LLM gateway for OpenAI, Anthropic, DeepSeek, Groq, MiniMax, and any OpenAI-compatible provider.</strong>
  </p>
  <p>
    Single binary, interactive CLI, JSON admin API, MCP server. Routing, fallback, rate limiting, embeddings.
  </p>
  <p>
    <a href="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml"><img src="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway"><img src="https://img.shields.io/crates/v/unigateway.svg" alt="Crate"></a>
    <a href="https://github.com/EeroEternal/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

## Features

- **Unified API**: `POST /v1/chat/completions` (OpenAI), `POST /v1/messages` (Anthropic), `POST /v1/embeddings`
- **Multi-provider**: OpenAI, Anthropic, DeepSeek, Groq, MiniMax, Ollama, Azure OpenAI, Together AI, OpenRouter — anything OpenAI-compatible
- **Routing**: round-robin load balancing, fallback with priority, provider pinning via header
- **Interactive CLI**: `ug quickstart` wizard, `ug config show/edit`, and full management commands
- **Rate limiting**: quota / QPS / concurrency limits per API key
- **Model mapping**: translate downstream model names to upstream provider models
- **Observability**: `GET /health`, `GET /metrics` (Prometheus), `GET /v1/models`
- **Admin API**: `/api/admin/*` for programmatic management
- **MCP server**: `ug mcp` exposes gateway management as MCP tools for Cursor, Claude Desktop, and other AI assistants
- **AI-ready**: ships with [Skill file](skills/SKILL.md) and [OpenAPI spec](skills/openapi.yaml) for AI agent integration

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

### Quick start

Run the interactive wizard — select provider, enter model and API key:

```bash
ug quickstart
ug serve
```

Or non-interactive:

```bash
ug quickstart --provider-type openai --endpoint-id gpt-4o --api-key "sk-..."
ug serve
```

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

## MCP Server

UniGateway can run as an [MCP](https://modelcontextprotocol.io/) (Model Context Protocol) server, letting AI assistants manage the gateway through natural language.

```bash
ug mcp                           # start MCP server over stdio
ug mcp --config /path/to/config  # custom config path
```

**Available tools**: `list_services`, `create_service`, `list_providers`, `create_provider`, `bind_provider`, `list_api_keys`, `create_api_key`, `show_config`, `get_metrics`

### Cursor / Claude Desktop config

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

## AI Integration

UniGateway ships with ready-to-use files for AI agents in the [`skills/`](skills/) directory:

| File | Purpose |
|------|---------|
| [`SKILL.md`](skills/SKILL.md) | Full operational guide for AI agents — install, configure, manage, and use all features |
| [`openapi.yaml`](skills/openapi.yaml) | OpenAPI 3.1 spec covering all gateway and admin endpoints |

Any AI tool (Codex, Cursor, ChatGPT, Claude, custom agents) can read these files to automate UniGateway setup and interact with the API programmatically.

## License

MIT. See [LICENSE](LICENSE).

## About

Author: [EeroEternal](https://github.com/EeroEternal) · songmqq@proton.me
