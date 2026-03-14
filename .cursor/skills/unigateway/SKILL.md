---
name: unigateway
description: One-shot init and manage UniGateway (LLM gateway with OpenAI/Anthropic compatibility). Use when the user wants to set up the gateway, create a service/provider/API key, bind provider to service, run metrics, or automate UniGateway setup from chat or scripts.
---

# UniGateway Skill

Use this skill when the user asks to set up, init, or manage **UniGateway** (the LLM gateway). The CLI binary is **`ug`**. Config is a single TOML file at `~/.config/unigateway/config.toml` (macOS: `~/Library/Application Support/unigateway/config.toml`), auto-created on first write. Override with `--config <path>` or `UNIGATEWAY_CONFIG` env.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/EeroEternal/unigateway/main/install.sh | sh
# or
brew install EeroEternal/tap/ug
# or
cargo install unigateway
```

## Preferred: quickstart (interactive)

Just run `ug quickstart` — it walks the user through provider type, model, base URL, and API key interactively:

```bash
ug quickstart
```

Non-interactive (for scripts):

```bash
ug quickstart \
  --provider-type openai \
  --endpoint-id gpt-4o \
  --api-key "sk-..."
```

For Anthropic: `--provider-type anthropic --endpoint-id claude-sonnet-4-20250514`. Optional: `--base-url`, `--service-id`, `--service-name`, `--provider-name`, `--model-mapping`. Then start the gateway: `ug serve` (or just `ug`).

## Manual setup (when quickstart is not enough)

Run in order. **provider_id** is the 0-based index of the provider (first created = 0).

1. **Create service**:
   ```bash
   ug create-service --id SERVICE_ID --name "Display Name"
   ```

2. **Create provider** (prints `provider_id`):
   ```bash
   ug create-provider \
     --name PROVIDER_NAME \
     --provider-type openai \
     --endpoint-id gpt-4o \
     --base-url https://api.openai.com \
     --api-key "sk-..."
   ```

3. **Bind provider to service**:
   ```bash
   ug bind-provider --service-id SERVICE_ID --provider-id 0
   ```

4. **Create gateway API key** (optional limits):
   ```bash
   ug create-api-key \
     --key "ugk_..." \
     --service-id SERVICE_ID
   ```
   Optional: `--quota-limit 100000` `--qps-limit 20` `--concurrency-limit 8`.

5. **Start gateway**:
   ```bash
   ug serve
   ```

## Config management

```bash
ug config path     # print config file location
ug config show     # print current config
ug config edit     # open in $EDITOR
```

## Other commands

- **Metrics**: `ug metrics`
- **Multi-provider round-robin**: bind multiple providers to the same service.
- **Fallback routing**: set `routing_strategy = "fallback"` on the service, use `priority` on bindings.

## Conventions

- All commands use the same default config path. No need to pass `--config` unless overriding.
- If the user does not specify a key name, `ug quickstart` auto-generates one (`ugk_` + random hex).
- After setup, suggest testing with:
  ```bash
  curl http://127.0.0.1:3210/v1/chat/completions \
    -H "Authorization: Bearer <key>" \
    -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Hi"}]}'
  ```
