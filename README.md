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
- **CLI**: `serve`, `init-admin`, `metrics`, `create-service`, `create-provider`, `bind-provider`, `create-api-key`
- **Service → Provider** binding with round-robin routing; API Key quota / QPS / concurrency limits
- **SQLite stats**: request count, status codes, latency; `GET /health`, `GET /metrics`, `GET /v1/models`
- **Admin API**: `/api/admin/*` (optional `x-admin-token`)

## Install

```bash
git clone https://github.com/EeroEternal/unigateway.git && cd unigateway && cargo build --release
# or
cargo install unigateway
```

## Usage

```bash
# Start (no subcommand = start gateway)
unigateway
# or
unigateway serve --bind 127.0.0.1:3210 --db sqlite://unigateway.db

# Init admin, print metrics
unigateway init-admin --username admin --password 'your-password' --db sqlite://unigateway.db
unigateway metrics --db sqlite://unigateway.db

# Create service → provider → bind → create API key
unigateway create-service --id svc_openai --name "OpenAI" --db sqlite://unigateway.db
unigateway create-provider --name openai-prod --provider-type openai --endpoint-id openai --base-url https://api.openai.com --api-key sk-xxx --db sqlite://unigateway.db
unigateway bind-provider --service-id svc_openai --provider-id 1 --db sqlite://unigateway.db
unigateway create-api-key --key ugk_xxx --service-id svc_openai --qps-limit 20 --concurrency-limit 8 --db sqlite://unigateway.db
```

## Config (env)

| Variable | Default | Description |
|----------|---------|-------------|
| `UNIGATEWAY_BIND` | `127.0.0.1:3210` | Bind address |
| `UNIGATEWAY_DB` | `sqlite://unigateway.db` | Database path |
| `UNIGATEWAY_ADMIN_TOKEN` | `""` | Admin API auth (`x-admin-token`) |

## API overview

- **OpenAI**: `POST /v1/chat/completions`, `Authorization: Bearer <key>`
- **Anthropic**: `POST /v1/messages`, `x-api-key`, `anthropic-version: 2023-06-01`
- **Admin**: `GET/POST /api/admin/services`, `GET/POST /api/admin/providers`, `POST /api/admin/bindings`, `GET/POST /api/admin/api-keys`

## License

MIT. See [LICENSE](LICENSE).

## About

Author: [EeroEternal](https://github.com/EeroEternal) · songmqq@proton.me
