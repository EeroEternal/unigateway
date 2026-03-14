<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>Scenario-oriented LLM gateway with OpenAI and Anthropic compatibility.</strong>
  </p>
  <p>
    Rich CLI + JSON admin API, single binary. Install as a <strong>Skill</strong> in Codex/Cursor for one-shot init and management. No Web UI.
  </p>
  <p>
    Built with Rust for fast startup and low overhead.
  </p>

  <p>
    <a href="https://github.com/lipish/unigateway/actions/workflows/rust.yml"><img src="https://github.com/lipish/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway"><img src="https://img.shields.io/crates/v/unigateway.svg" alt="Crate"></a>
    <a href="https://github.com/lipish/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

<br />

## Philosophy

**UniGateway** is built around **scenarios**: single-provider proxy, multi-provider round-robin, local multi-model playground, team shared gateway, or cost-aware routing. You manage it via a **rich CLI** and a **JSON admin API**—no Web UI. Install it as a **Skill** in Codex or Cursor and let the AI one-shot create services, providers, bindings, and API keys for your project.

It is a drop-in replacement for OpenAI/Anthropic clients, with request logging, latency tracking, service-based routing, and per-key quota/QPS/concurrency limits.

## Features

- 🎯 **Scenario-Oriented**: Designed for single-provider proxy, multi-provider round-robin, local dev, and team/cost-control use cases; docs and CLI guide you by scenario.
- 🧰 **Rich CLI**: Full management from the shell—`serve`, `init-admin`, `metrics`, `create-service`, `create-provider`, `bind-provider`, `create-api-key`; scriptable and AI-friendly output.
- 🔌 **Skills Integration**: Install as a Skill in Codex/Cursor so the AI can init and manage the gateway (create provider, bind, create key) in one shot from chat or automation.
- 🔄 **Unified Interface**:
  - `POST /v1/chat/completions` (OpenAI compatible)
  - `POST /v1/messages` (Anthropic compatible)
- 📊 **Built-in Analytics**: Tracks request counts, status codes, and latency in a local SQLite database.
- 📈 **Observability**: `GET /health`, `GET /metrics` (Prometheus), `GET /v1/models`.
- 🧭 **Service Routing**: Service → provider binding with round-robin selection.
- 🔐 **API Key Limits**: Per-key quota, QPS, and concurrency limits.
- 📡 **JSON Admin API**: `/api/admin/*` for automation and remote management (optional `x-admin-token`).
- 📦 **Single Binary**: One executable; no Web UI, no lib dependency.

## Installation

### From Source

Ensure you have [Rust installed](https://rustup.rs/).

```bash
git clone https://github.com/lipish/unigateway.git
cd unigateway
cargo build --release
```

### From crates.io

```bash
cargo install unigateway
```

## Usage

### Running the Server

```bash
# Run with default settings (no subcommand = start gateway)
cargo run

# Or explicitly
cargo run -- serve --bind 127.0.0.1:3210 --db sqlite://unigateway.db
```

The server will start on `http://127.0.0.1:3210` by default.

## Management (CLI + JSON API)

UniGateway is managed **only** via the CLI and the JSON Admin API. There is no Web UI.

- **Providers**: Register upstream vendors (OpenAI, Anthropic, DeepSeek, or custom-compatible backends) and bind them to services.
- **Services**: Define the routing layer between API keys and providers (round-robin over bound providers).
- **API Keys**: Create gateway keys tied to a service; each key can have quota, QPS, and concurrency limits.

Use the CLI for one-off or scripted setup; use the Admin API when integrating with other systems or automation.

### Skills (Codex / Cursor)

UniGateway can be installed as a **Skill** in Codex or Cursor. Once installed, the AI assistant can create services, providers, bindings, and API keys in one shot from chat or from automation workflows—no need to run CLI commands by hand. Look for **UniGateway** in the skill catalog or install from this repo’s skill definition when available.

### Configuration

UniGateway is configured via environment variables. You can set these in a `.env` file or export them directly.

| Variable | Default | Description |
|----------|---------|-------------|
| `UNIGATEWAY_BIND` | `127.0.0.1:3210` | The address to bind the server to. |
| `UNIGATEWAY_DB` | `sqlite://unigateway.db` | Path to the SQLite database file. |
| `UNIGATEWAY_ADMIN_TOKEN` | `""` | Optional token for admin APIs (`x-admin-token` header). If set, admin API requests must include it. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL for OpenAI API. |
| `OPENAI_API_KEY` | `""` | Default OpenAI API key (optional). |
| `OPENAI_MODEL` | `gpt-4o-mini` | Default model for OpenAI requests. |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Base URL for Anthropic API. |
| `ANTHROPIC_API_KEY` | `""` | Default Anthropic API key (optional). |
| `ANTHROPIC_MODEL` | `claude-3-5-sonnet-latest` | Default model for Anthropic requests. |

### CLI Operations

```bash
# Start gateway with optional overrides
unigateway serve --bind 127.0.0.1:3210 --db sqlite://unigateway.db

# Initialize/reset admin user (for DB schema; not used by HTTP admin API)
unigateway init-admin --username admin --password 'your-password' --db sqlite://unigateway.db

# Print metrics snapshot to stdout
unigateway metrics --db sqlite://unigateway.db

# Create service
unigateway create-service --id svc_openai --name "OpenAI Service" --db sqlite://unigateway.db

# Create provider (returns provider_id)
unigateway create-provider \
  --name openai-prod \
  --provider-type openai \
  --endpoint-id openai \
  --base-url https://api.openai.com \
  --api-key sk-xxx \
  --db sqlite://unigateway.db

# Bind provider to service
unigateway bind-provider --service-id svc_openai --provider-id 1 --db sqlite://unigateway.db

# Create gateway API key with limits
unigateway create-api-key \
  --key ugk_xxx \
  --service-id svc_openai \
  --qps-limit 20 \
  --concurrency-limit 8 \
  --quota-limit 100000 \
  --db sqlite://unigateway.db
```

## API Endpoints

### OpenAI Compatible
```http
POST /v1/chat/completions
Authorization: Bearer <YOUR_OPENAI_KEY>
Content-Type: application/json

{
  "model": "gpt-4o-mini",
  "messages": [{"role": "user", "content": "Hello!"}]
}
```

### Anthropic Compatible
```http
POST /v1/messages
x-api-key: <YOUR_ANTHROPIC_KEY>
anthropic-version: 2023-06-01
Content-Type: application/json

{
  "model": "claude-3-5-sonnet-latest",
  "messages": [{"role": "user", "content": "Hello!"}],
  "max_tokens": 1024
}
```

### Health & Metrics
```http
GET /health
GET /metrics
GET /v1/models
```

### Admin APIs (JSON only)

Management is done via these endpoints. When `UNIGATEWAY_ADMIN_TOKEN` is set, include:

```http
x-admin-token: <YOUR_ADMIN_TOKEN>
```

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/admin/services` | List services |
| POST | `/api/admin/services` | Create service |
| GET | `/api/admin/providers` | List providers |
| POST | `/api/admin/providers` | Create provider |
| POST | `/api/admin/bindings` | Bind provider to service |
| GET | `/api/admin/api-keys` | List API keys |
| POST | `/api/admin/api-keys` | Create/update API key |

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
