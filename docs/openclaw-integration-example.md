# UniGateway and OpenClaw Integration Example

This document provides an OpenClaw integration example for individual developer scenarios, aiming to let OpenClaw treat UniGateway as a unified local OpenAI-compatible entry point.

## 1. Integration Strategy

In the current implementation, OpenClaw connects to UniGateway through a custom provider:

- OpenClaw is responsible for agent interaction and tool use
- UniGateway is responsible for the unified local entry point, mode routing, fallback, and provider switching
- Mode selection is primarily determined by different gateway API keys

In other words, OpenClaw doesn't need to directly understand the differences between upstreams like OpenAI, Anthropic, DeepSeek, Groq, etc.; it only needs to connect to UniGateway.

## 2. Prerequisites

Ensure you have completed the following steps:

1. Start UniGateway

```bash
ug serve
```

2. Prepare a gateway key corresponding to at least one mode

If you have executed `ug quickstart`, a `default` mode and its corresponding key are usually automatically generated.

You can also create one manually:

```bash
ug create-api-key --key ugk_default_example --service-id default
```

3. Confirm the integration template for the current mode

```bash
ug integrations --mode default --tool openclaw
```

## 3. Configuration Example

Add the following to `~/.openclaw/openclaw.json`:

```js
{
  agents: {
    defaults: {
      model: { primary: "unigateway/deepseek-chat" }
    }
  },
  models: {
    mode: "merge",
    providers: {
      unigateway: {
        baseUrl: "http://127.0.0.1:3210/v1",
        apiKey: "${UNIGATEWAY_API_KEY}",
        api: "openai-completions",
        models: [
          { id: "deepseek-chat", name: "UniGateway deepseek-chat" }
        ]
      }
    }
  }
}
```

Then export the environment variable:
```bash
export UNIGATEWAY_API_KEY=ugk_default_example
```

At this point, OpenClaw will make calls through UniGateway.

## 4. More Advanced Configuration

If you want to configure more models or aliases in OpenClaw, you can continue to add correlation to the `models` field in `providers`:

```js
{
  agents: {
    defaults: {
      model: { primary: "ug-fast/deepseek-chat" }
    }
  },
  models: {
      "unigateway": {
        baseUrl: "http://127.0.0.1:3210/v1",
        apiKey: "${UNIGATEWAY_API_KEY}",
        api: "openai-completions",
        models: [
          { id: "deepseek-chat", name: "UniGateway Chat" },
          { id: "gpt-4o", name: "UniGateway Reasoning" }
        ]
      }
    }
  }
}
```

Corresponding environment variable:

```bash
export UNIGATEWAY_API_KEY=ugk_default_example
```

## 5. Relationship Between mode, model, and key

The most important relationships are:
- **Gateway API key** determines the mode used for requests
- **OpenClaw model id** determines the model name intended for use
- **UniGateway** determines which upstream/provider this model name finally maps to

Therefore:

- To switch different access configurations, you usually switch the key
- To switch models under the same key, you usually switch the model id

## 6. Verification Steps

Suggested verification order:

1. Start the gateway

```bash
ug serve
```

2. Check modes

```bash
ug mode list
ug route explain default
```

3. Check the OpenClaw template for the current mode

```bash
ug integrations --mode default --tool openclaw
```

4. Run diagnostics

```bash
ug doctor --mode default
ug test --mode default
```

5. Then initiate a request from OpenClaw

If OpenClaw can receive replies normally, it indicates the access chain has been established.

## 7. Current Limitations and Suggestions

Current OpenClaw integration is already suitable for individual developer scenarios but belongs to the first version of template support:

- Advanced fallback configurations or finer error diagnostics dedicated to OpenClaw have not yet been implemented

Current recommendations:

- Stable connection first via OpenAI-compatible provider method
- Ensure the primary/backup chain under `default` mode works properly
- Continue refining the template based on real OpenClaw usage experience
