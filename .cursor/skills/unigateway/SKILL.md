---
name: unigateway
description: One-shot init and manage UniGateway (LLM gateway with OpenAI/Anthropic compatibility). Use when the user wants to set up the gateway, create a service/provider/API key, bind provider to service, run init-admin or metrics, or automate UniGateway setup from chat or scripts.
---

# UniGateway Skill

Use this skill when the user asks to set up, init, or manage **UniGateway** (the LLM gateway): creating services, providers, API keys, bindings, or running the server. Prefer the CLI; use the same `--config` path everywhere (default `unigateway.toml`). No database: config is a single TOML file.

## Preferred: quickstart (single provider)

For one provider behind one gateway key, use one command (key is auto-generated and printed):

```bash
unigateway quickstart \
  --provider-type openai \
  --endpoint-id openai \
  --base-url https://api.openai.com \
  --api-key "sk-..." \
  --config unigateway.toml
```

For Anthropic: `--provider-type anthropic` `--endpoint-id anthropic` `--base-url https://api.anthropic.com` and the user's API key. Optional: `--service-id`, `--service-name`, `--provider-name`, `--model-mapping`. Then start the gateway: `unigateway serve` (or `unigateway`).

## Manual one-shot init (when quickstart is not enough)

Run in order. Use a single config path (e.g. `unigateway.toml` or `--config /path/to/unigateway.toml`). **provider_id** is the 0-based index of the provider (first created = 0).

1. **Create service**:
   ```bash
   unigateway create-service --id SERVICE_ID --name "Display Name" --config unigateway.toml
   ```
   Example: `--id default --name "Default"`.

2. **Create provider** (prints `provider_id`, 0-based index; required: `--name`, `--provider-type`, `--endpoint-id`, `--api-key`):
   ```bash
   unigateway create-provider \
     --name PROVIDER_NAME \
     --provider-type openai \
     --endpoint-id openai \
     --base-url https://api.openai.com \
     --api-key "sk-..." \
     --config unigateway.toml
   ```
   Use the printed `provider_id` (0 for first provider) in the next step. For Anthropic use `--provider-type anthropic`, `--endpoint-id anthropic`, and appropriate `--base-url`.

3. **Bind provider to service**:
   ```bash
   unigateway bind-provider --service-id SERVICE_ID --provider-id 0 --config unigateway.toml
   ```

4. **Create gateway API key** (optional limits):
   ```bash
   unigateway create-api-key \
     --key "ugk_..." \
     --service-id SERVICE_ID \
     --config unigateway.toml
   ```
   Optional: `--quota-limit 100000` `--qps-limit 20` `--concurrency-limit 8`. Tell the user the key for `Authorization: Bearer <key>` or `x-api-key: <key>`.

5. **Start gateway** (if not already running):
   ```bash
   unigateway serve --bind 127.0.0.1:3210
   ```
   Or just `unigateway` (no subcommand = serve).

## Other commands

- **Metrics snapshot**: `unigateway metrics --config unigateway.toml` (in-memory counts; 0 if server not running).
- **Multi-provider**: Create multiple providers, bind them to the same service with multiple `bind-provider` calls; routing is round-robin.

## Conventions

- Use one config path for all commands (default `unigateway.toml`).
- If the user does not specify a key name, generate a safe placeholder (e.g. `ugk_` + random suffix) and remind them to replace it.
- After one-shot init, suggest testing with: `curl -H "Authorization: Bearer <key>" http://127.0.0.1:3210/v1/chat/completions -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hi"}]}'`
