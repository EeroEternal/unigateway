<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>A lightweight, open-source LLM gateway with OpenAI & Anthropic compatibility.</strong>
  </p>
  <p>
    Built with Rust. Blazing fast, memory-safe, and zero dependencies.
  </p>

  <p>
    <a href="https://github.com/lipish/unigateway/actions/workflows/rust.yml"><img src="https://github.com/lipish/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway"><img src="https://img.shields.io/crates/v/unigateway.svg" alt="Crate"></a>
    <a href="https://github.com/lipish/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

<br />

## Philosophy

Managing multiple LLM providers (OpenAI, Anthropic, etc.) in production can be complex. **UniGateway** solves this by providing a unified, lightweight proxy layer that sits between your application and the LLM providers.

It offers a **drop-in replacement** for standard OpenAI and Anthropic clients, while adding essential features like request logging, latency tracking, and a built-in admin dashboard—all without the overhead of heavy API gateway solutions.

## Features

- 🚀 **High Performance**: Built on Rust and Axum for minimal latency and resource usage.
- 🔄 **Unified Interface**:
  - `POST /v1/chat/completions` (OpenAI compatible)
  - `POST /v1/messages` (Anthropic compatible)
- 📊 **Built-in Analytics**: Tracks request counts, status codes, and latency in a local SQLite database.
- 📈 **Minimal Observability**: Exposes `GET /metrics` (Prometheus text format) for external observability integration.
- 🧭 **Service Routing**: Supports `service -> provider` binding with round-robin selection.
- 🔐 **API Key Limits (MVP)**: Supports per-key quota, QPS, and concurrency limits.
- 🛡️ **Lightweight Admin UI**: A zero-dependency admin dashboard built with HTMX + DaisyUI (templates separated from Rust handlers).
- 🧰 **CLI First Operations**: Supports no-UI/headless runtime and admin operations from CLI.
- 📦 **Flexible Deployment**: Run as a standalone binary or embed it as a library in your Rust application.

## Installation

### From Source

Ensure you have [Rust installed](https://rustup.rs/).

```bash
git clone https://github.com/mac-m4/unigateway.git
cd unigateway
cargo build --release
```

## Usage

### Running the Server

```bash
# Run with default settings
cargo run --bin unigateway

# Headless mode (no admin UI routes)
cargo run --bin unigateway -- serve --no-ui
```

The server will start on `http://127.0.0.1:3210` by default.

### Configuration

UniGateway is configured via environment variables. You can set these in a `.env` file or export them directly.

| Variable | Default | Description |
|----------|---------|-------------|
| `UNIGATEWAY_BIND` | `127.0.0.1:3210` | The address to bind the server to. |
| `UNIGATEWAY_DB` | `sqlite://unigateway.db` | Path to the SQLite database file. |
| `UNIGATEWAY_ENABLE_UI` | `true` | Enable/disable web admin UI routes. |
| `UNIGATEWAY_ADMIN_TOKEN` | `""` | Optional token for admin APIs (`x-admin-token` header). |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL for OpenAI API. |
| `OPENAI_API_KEY` | `""` | Default OpenAI API key (optional). |
| `OPENAI_MODEL` | `gpt-4o-mini` | Default model for OpenAI requests. |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Base URL for Anthropic API. |
| `ANTHROPIC_API_KEY` | `""` | Default Anthropic API key (optional). |
| `ANTHROPIC_MODEL` | `claude-3-5-sonnet-latest` | Default model for Anthropic requests. |

### Admin Dashboard

Access the admin dashboard at `http://127.0.0.1:3210/admin`.

- **Username**: `admin`
- **Password**: `admin123` (Default)

> **Note**: The dashboard provides real-time statistics on request volume and distribution across providers.

### CLI Operations

```bash
# Start service with optional overrides
unigateway serve --bind 127.0.0.1:3210 --db sqlite://unigateway.db

# Initialize/reset admin account in DB
unigateway init-admin --username admin --password 'your-password' --db sqlite://unigateway.db

# Print metrics snapshot to stdout
unigateway metrics --db sqlite://unigateway.db

# Create service
unigateway create-service --id svc_openai --name "OpenAI Service" --db sqlite://unigateway.db

# Create provider (returns provider_id)
unigateway create-provider \
  --name openai-prod \
  --provider-type openai \
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

### Metrics
```http
GET /metrics
```

### Admin APIs (Headless)
```http
GET  /api/admin/services
POST /api/admin/services
GET  /api/admin/providers
POST /api/admin/providers
POST /api/admin/bindings
GET  /api/admin/api-keys
POST /api/admin/api-keys
```

When `UNIGATEWAY_ADMIN_TOKEN` is set, send header:

```http
x-admin-token: <YOUR_ADMIN_TOKEN>
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
