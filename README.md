<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>Unified AI Entry Point for Personal Developers & Power Users.</strong>
  </p>
  <p>
    Connect all your AI tools to any LLM provider through a single, stable local endpoint.
  </p>
  <p>
    🌐 <strong><a href="http://unigate.sh/">Website: unigate.sh</a></strong>
  </p>
  <p>
    <a href="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml"><img src="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway"><img src="https://img.shields.io/crates/v/unigateway.svg" alt="Crate"></a>
    <a href="https://github.com/EeroEternal/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

UniGateway is a lightweight, high-performance LLM gateway designed for developers who use multiple AI tools (Cursor, Zed, Claude Code, etc.) and multiple providers (OpenAI, Anthropic, DeepSeek, Groq, etc.).

## 📦 Install

```bash
curl -fsSL https://unigate.sh/install.sh | sh
```

Or via Homebrew / Cargo:

```bash
brew install EeroEternal/tap/ug          # macOS (Homebrew)
cargo install unigateway                 # Rust toolchain
```

## 🛠️ Usage

### 1. Quick Start
Run the interactive wizard to set up your first provider and generate a default configuration:

```bash
ug guide          # or 'ug quickstart'
ug serve          # Starts in background by default
```

### 2. Managing the Server
UniGateway runs in the background. Use these commands to manage it:

```bash
ug status         # unigateway is running (pid: 1234)
ug stop           # stopped
ug logs           # View or tail the logs
ug serve -f       # Run in the foreground (blocking)
```

### 3. Managing Modes
UniGateway organizes providers into **Modes**. Use the CLI to manage them:

```bash
ug mode list          # See all available modes
ug mode show fast     # mode: fast (Fast) | routing: fallback
ug mode use strong    # set 'default' to 'strong'
```

### 3.5 External Admin API (Headless)
UniGateway does not embed a Web UI. External admin clients (for example UniAdmin)
should call JSON endpoints under `/api/admin/*`.

Authentication:
- If `UNIGATEWAY_ADMIN_TOKEN` is set, include `x-admin-token` on all admin requests.
- If not set, behavior follows existing admin API defaults and is recommended only on trusted local networks.

Useful endpoints for external admin tools:
- `GET /api/admin/modes`: list mode summaries for selector UIs
- `POST /api/admin/preferences/default-mode`: set `preferences.default_mode`
- `PATCH /api/admin/api-keys`: rebind an existing API key to a target `service_id`

Routing semantics:
- Runtime HTTP routing uses the API key's `service_id`.
- `preferences.default_mode` affects CLI defaults and integration guidance.
- To provide a one-click "switch mode" UX, admin clients should typically update default mode and key binding together.

Example curl flow:
```bash
export UG_BASE_URL="http://127.0.0.1:3210"
export UG_ADMIN_TOKEN="your-admin-token"

# 1) List modes (summary)
curl -sS "$UG_BASE_URL/api/admin/modes" \
  -H "x-admin-token: $UG_ADMIN_TOKEN"

# Optional: detailed mode payload (providers + keys)
curl -sS "$UG_BASE_URL/api/admin/modes?detailed=true" \
  -H "x-admin-token: $UG_ADMIN_TOKEN"

# 2) Set default mode
curl -sS -X POST "$UG_BASE_URL/api/admin/preferences/default-mode" \
  -H "content-type: application/json" \
  -H "x-admin-token: $UG_ADMIN_TOKEN" \
  -d '{"mode_id":"strong"}'

# 3) Rebind an existing gateway key to a mode/service
curl -sS -X PATCH "$UG_BASE_URL/api/admin/api-keys" \
  -H "content-type: application/json" \
  -H "x-admin-token: $UG_ADMIN_TOKEN" \
  -d '{"key":"ugk_xxx","service_id":"strong"}'
```

Minimal JSON contract examples:

`GET /api/admin/modes` (summary)
```json
{
  "success": true,
  "data": [
    {
      "id": "fast",
      "name": "Fast",
      "routing_strategy": "round_robin",
      "is_default": true,
      "provider_count": 1,
      "provider_names": ["deepseek-main"]
    }
  ]
}
```

`POST /api/admin/preferences/default-mode`
Request:
```json
{"mode_id":"strong"}
```
Response:
```json
{
  "success": true,
  "data": {
    "mode_id": "strong"
  }
}
```

`PATCH /api/admin/api-keys`
Request:
```json
{"key":"ugk_fast_123","service_id":"strong"}
```
Response:
```json
{
  "success": true,
  "data": {
    "key": "ugk_fast_123",
    "service_id": "strong"
  }
}
```

Development networking notes:
- Preferred: configure a reverse proxy so UniAdmin and UniGateway share one origin.
- Local-only alternative: allow CORS in your dev stack (do not expose permissive CORS on public listeners).

One-click switch pattern for admin clients:
1. `POST /api/admin/preferences/default-mode` to update user-facing default
2. `PATCH /api/admin/api-keys` to update runtime routing for the selected key

Detailed integration guide:
- See [docs/design/admin.md](docs/design/admin.md) for endpoint contracts, suggested frontend workflow, and error handling recommendations.

### 3. Tool Integrations
Get ready-to-use configuration snippets or interactively set up your favorite AI tools:

```bash
ug launch claudecode  # interactive configuration for Claude Code
ug launch             # open interactive tool picker
ug integrations       # list all integration hints
ug integrations --tool aider  # get Aider management skill
```

### 4. Diagnostics & Testing
Understand routing and verify connectivity:

```bash
ug route explain      # Explain how the current mode routes requests
ug status             # Check process status and MCP readiness
ug test               # Send a smoke test request to the gateway
ug doctor             # Run a full diagnostic check on your setup
```

## 🔌 AI Integrations

UniGateway is designed for the modern AI ecosystem.

### Popular Tool Configs

#### 🛠️ Aider
You can now manage UniGateway directly through Aider using the built-in Management Skill:
```bash
# Generate the skill file
ug integrations --tool aider > .aider.conf.md

# Launch Aider with the skill
aider --read .aider.conf.md
```
Once loaded, you can ask Aider to "start the gateway", "switch to fast mode", or "check logs".

#### 🛠️ Claude Code
Configure Claude Code to use UniGateway via the Anthropic-compatible endpoint:
```bash
export ANTHROPIC_BASE_URL="http://127.0.0.1:3210"
export ANTHROPIC_API_KEY="ugk_your_key"
export ANTHROPIC_MODEL="kimi-k2.5"

# launch
ANTHROPIC_BASE_URL=http://127.0.0.1:3210 \
ANTHROPIC_API_KEY=ugk_your_key \
ANTHROPIC_MODEL=kimi-k2.5 \
claude
```

Note: `claude -p` typically honors these env vars directly. In some CLI versions,
interactive `claude` may still show a login onboarding flow on first launch.

### MCP (Model Context Protocol)
Manage your gateway through natural language in Cursor or Claude Desktop:
```bash
ug mcp
```

### AI Agent Skills
Ships with a [Skill file](skills/SKILL.md) and [OpenAPI spec](skills/openapi.yaml) to help AI agents automate your LLM infrastructure.

## 🚀 Key Features

- **Unified Interface**: OpenAI-compatible API for all providers, including Anthropic and local models.
- **Mode-Based Routing**: Group providers into semantic "modes" (e.g., `fast`, `strong`, `backup`) for easy switching.
- **Pre-configured Integrations**: Get instant setup snippets for Cursor, Zed, Claude Code, and more.
- **Failover & Stability**: Built-in fallback strategies to ensure your AI tools keep working even if a provider goes down.
- **Deep Visibility**: Use `ug route explain` and `ug doctor` to understand exactly how requests are routed and debug connection issues.
- **MCP Server**: Built-in Model Context Protocol server for AI assistants to manage the gateway.

## 🤖 Agent / contributor notes

See [`AGENTS.md`](AGENTS.md) for AI-agent and contributor conventions (build, docs layout, safety).

## 📄 License

MIT. See [LICENSE](LICENSE).

