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

### 3. Tool Integrations
Get ready-to-use configuration snippets or interactively set up your favorite AI tools:

```bash
ug launch claudecode  # interactive configuration for Claude Code
ug launch             # open interactive tool picker
ug integrations       # list all integration hints
```

### 4. Diagnostics & Testing
Understand routing and verify connectivity:

```bash
ug route explain      # Explain how the current mode routes requests
ug test               # Send a smoke test request to the gateway
ug doctor             # Run a full diagnostic check on your setup
```

## 🔌 AI Integrations

UniGateway is designed for the modern AI ecosystem.

### Popular Tool Configs

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

## 📄 License

MIT. See [LICENSE](LICENSE).

