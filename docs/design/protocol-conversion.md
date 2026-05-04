---
title: Protocol Conversion Architecture
---

# Protocol Conversion Architecture

UniGateway supports cases where the client-facing protocol and the upstream provider protocol are not the same. A client can send OpenAI Chat Completions or Anthropic Messages payloads, while a selected upstream endpoint can be OpenAI-compatible or Anthropic-native.

The conversion model is intentionally layered. UniGateway does not treat OpenAI and Anthropic as identical protocols. Instead, it uses a neutral core request model for shared chat concepts, preserves provider-native payloads where needed, and performs protocol-specific rendering at the driver and response boundaries.

## Layered Flow

```text
Embedder HTTP layer
  -> unigateway-protocol parses client JSON
  -> ProxyChatRequest / ProxyResponsesRequest / ProxyEmbeddingsRequest
  -> unigateway-host resolves the pool, target, protocol, and provider hint
  -> unigateway-core selects endpoints, retries, falls back, and calls drivers
  -> OpenAI / Anthropic driver builds the upstream-native request
  -> driver parses upstream response or stream into ProxySession
  -> unigateway-protocol renders the client-facing JSON or SSE response
  -> Embedder HTTP layer returns the response
```

Important code entry points:

- `unigateway-protocol/src/requests.rs`: client JSON to proxy request types.
- `unigateway-core/src/request.rs`: neutral request types, protocol metadata, and content blocks.
- `unigateway-host/src/core/dispatch.rs`: host-level protocol dispatch.
- `unigateway-core/src/protocol/openai/requests.rs`: proxy requests to OpenAI-compatible upstream requests.
- `unigateway-core/src/protocol/anthropic.rs`: proxy requests to Anthropic upstream requests.
- `unigateway-protocol/src/responses/render.rs`: proxy sessions to OpenAI or Anthropic client responses.
- `unigateway-protocol/src/responses/aggregator.rs`: Anthropic stream aggregation helper.

## Neutral Chat Model

The central chat request shape is `ProxyChatRequest`:

```text
ProxyChatRequest
  model
  messages: Vec<Message>
  system
  tools
  tool_choice
  raw_messages
  extra
  metadata
```

Messages are block-first rather than plain strings:

```text
Message
  role: System | User | Assistant | Tool
  content: Vec<ContentBlock>

ContentBlock
  Text { text }
  Thinking { thinking, signature }
  ToolUse { id, name, input }
  ToolResult { tool_use_id, content }
```

This structure can represent the shared chat concepts from both protocols:

```text
OpenAI message.content string or text blocks -> ContentBlock::Text
OpenAI assistant.tool_calls                -> ContentBlock::ToolUse
OpenAI tool role message                   -> ContentBlock::ToolResult
Anthropic content[].text                   -> ContentBlock::Text
Anthropic content[].thinking               -> ContentBlock::Thinking
Anthropic content[].tool_use               -> ContentBlock::ToolUse
Anthropic content[].tool_result            -> ContentBlock::ToolResult
```

The neutral model is not a promise that the two protocols are fully equivalent. It is a structured common layer plus preservation channels for provider-native information.

## Preservation Channels

Some protocol features do not fit into a fully symmetric common model. UniGateway keeps those fields explicit instead of forcing lossy normalization too early.

`raw_messages` preserves the original client `messages` array. This allows OpenAI client messages to be sent to OpenAI-compatible upstreams without unnecessary reconstruction, and Anthropic client messages to be sent to Anthropic upstreams with their original content block order intact.

`system` preserves Anthropic's top-level `system` field. Anthropic does not encode system instructions inside the `messages` array, so this field must stay separate when converting Anthropic requests to OpenAI-compatible requests and back.

`tools` and `tool_choice` are currently preserved as JSON values. Driver-side conversion maps the OpenAI function-tool shape to the Anthropic tool shape, or the reverse, only when the selected upstream protocol requires it.

`extra` carries protocol-specific request fields that UniGateway does not model as first-class neutral fields. Core fields win when `extra` overlaps with built-in payload keys.

`metadata` records protocol semantics that should not be inferred from shape alone. The important metadata includes:

```text
unigateway.client_protocol
unigateway.openai_raw_messages
unigateway.thinking_signature_status
```

## Ingress Parsing

OpenAI Chat requests are parsed by `openai_payload_to_chat_request`.

The parser extracts common fields such as `model`, `messages`, `temperature`, `top_p`, `max_tokens`, `tools`, `tool_choice`, `stop`, and `stream`. It also preserves the original OpenAI `messages` array as `raw_messages`, marks it with `unigateway.openai_raw_messages`, and sets `ClientProtocol::OpenAiChat`.

Anthropic Messages requests are parsed by `anthropic_payload_to_chat_request`.

The parser extracts top-level `system`, `messages`, `tools`, `tool_choice`, `stop_sequences`, `max_tokens`, and `stream`. It preserves the original Anthropic `messages` array, sets `ClientProtocol::AnthropicMessages`, and records the semantic status of thinking signatures.

Thinking signature status has three meanings:

```text
Absent      no thinking signature is present
Placeholder a compatibility placeholder is present
Verbatim    a real Anthropic signature was preserved verbatim
```

## Upstream Request Construction

The host layer does not build provider-native request bodies. It resolves the execution target and calls `UniGatewayEngine`. Provider drivers perform upstream protocol construction.

### OpenAI-Compatible Upstream

The OpenAI-compatible driver builds `/chat/completions`, `/responses`, and `/embeddings` requests.

For chat requests:

- If the request originated as OpenAI Chat and carries OpenAI raw messages, the driver reuses those raw messages.
- If the request originated as Anthropic Messages, the driver converts Anthropic messages to OpenAI messages.
- Anthropic top-level `system` is inserted as an OpenAI `system` message.
- Anthropic `tool_use` blocks become OpenAI assistant `tool_calls`.
- Anthropic `tool_result` blocks become OpenAI `tool` role messages.
- Anthropic tools and tool choice are converted to OpenAI function-tool shapes where possible.

### Anthropic Upstream

The Anthropic driver builds `/v1/messages` requests.

For chat requests:

- If the request originated as Anthropic Messages, the driver validates and preserves the raw Anthropic `messages` array.
- If the request originated as OpenAI Chat, the driver explicitly converts OpenAI messages into Anthropic Messages format.
- OpenAI `system` messages become Anthropic top-level `system` content.
- OpenAI text content becomes Anthropic `text` blocks.
- OpenAI assistant `tool_calls` become Anthropic `tool_use` blocks.
- OpenAI `tool` role messages become Anthropic user `tool_result` blocks.
- OpenAI tools and tool choice are converted to Anthropic tool shapes where possible.

Anthropic `responses` and `embeddings` are not implemented by the Anthropic driver. They currently return a not-implemented gateway error.

## Response Rendering

Drivers parse upstream responses into `ProxySession`:

```text
ProxySession::Completed(CompletedResponse)
ProxySession::Streaming(StreamingResponse)
```

Completed responses keep the upstream raw JSON, extracted output text, model, usage, latency, selected provider, request id, and attempt reports.

Streaming responses expose chunks plus a completion future. Each chunk can carry a simple text delta and the original raw event or raw chunk.

The protocol layer then renders the final client-facing shape.

### OpenAI Client Rendering

`render_openai_chat_session` renders OpenAI Chat responses.

- If the selected provider is OpenAI-compatible and the raw response already has OpenAI `choices`, the raw response is returned.
- Otherwise UniGateway constructs an OpenAI `chat.completion` response from the neutral final response.
- For streaming, `OpenAiChatStreamAdapter` emits OpenAI SSE chunks and a final `[DONE]` frame.

### Anthropic Client Rendering

`render_anthropic_chat_session` renders Anthropic Messages responses.

- If the selected provider is Anthropic and the raw response is already an Anthropic `message`, the raw response is returned.
- If the selected provider is OpenAI-compatible, UniGateway converts OpenAI message content, reasoning content, tool calls, usage, and finish reason into Anthropic message fields.
- For streaming, native Anthropic stream events are passed through. OpenAI-style stream chunks are converted into an Anthropic SSE lifecycle.

The synthesized Anthropic stream lifecycle is:

```text
message_start
ping
content_block_start
content_block_delta
content_block_stop
message_delta
message_stop
```

## Stream Aggregation

Anthropic extended thinking and tool use are incremental in streaming mode. A complete assistant message may require combining several event types:

```text
thinking_delta
signature_delta
text_delta
input_json_delta
message_delta
message_stop
```

`AnthropicStreamAggregator` is a library helper that can rebuild a completed Anthropic message from Anthropic SSE events. It preserves content block order and can rebuild thinking blocks with signatures when the upstream emitted real signature deltas.

UniGateway provides aggregation helpers, but it does not store conversations. The embedder decides whether and where to save the completed assistant message for future turns.

## Support Matrix

```text
OpenAI client -> OpenAI-compatible upstream
  Supported as the primary OpenAI-compatible path.

Anthropic client -> Anthropic upstream
  Supported with raw Anthropic message preservation, including thinking signatures.

Anthropic client -> OpenAI-compatible upstream
  Supported for the shared message, system, tool, tool_choice, and thinking/reasoning subset.

OpenAI client -> Anthropic upstream
  Supported for system, text, tool_calls, tool results, tools, and tool_choice conversion.

OpenAI Responses API -> OpenAI-compatible upstream
  Supported by the OpenAI-compatible driver.

OpenAI Embeddings API -> OpenAI-compatible upstream
  Supported by the OpenAI-compatible driver.

Responses or embeddings -> Anthropic upstream
  Not implemented.
```

## Loss and Degradation Rules

Cross-protocol conversion must distinguish these categories:

```text
Lossless pass-through fields
Structurally convertible fields
Fields that can only be degraded
Fields that must not be fabricated
```

Anthropic thinking `signature` belongs to the last category. A real signature can only come from Anthropic-native client history or an Anthropic upstream response.

OpenAI has no equivalent field for Anthropic thinking signatures. If an Anthropic response is rendered to an OpenAI client and that client sends the next turn back as OpenAI Chat, the signature will usually be lost unless the embedder stores the original Anthropic assistant content separately.

OpenAI reasoning content can be rendered as Anthropic `thinking` for compatibility, but a placeholder signature is not a real Anthropic continuation signature. Placeholder signatures must not be treated as valid signatures for Anthropic-native multi-turn continuation.

## Design Invariants

For Anthropic-native thinking paths, UniGateway should preserve:

```text
thinking block content
real signature value
content block order
tool_use and tool_result id relationships
```

For cross-protocol paths, UniGateway should:

```text
record the source client protocol explicitly
prefer raw provider-native pass-through for same-protocol paths
perform explicit conversion for cross-protocol paths
surface invalid tool or message shapes as InvalidRequest when strict conversion is required
avoid converting placeholder thinking signatures into real upstream continuation signatures
```

## Boundary

UniGateway is a library workspace. It owns protocol parsing, neutral request and response types, provider driver conversion, response rendering, stream normalization, aggregation helpers, and neutral metadata.

It does not own HTTP routing, authentication, user or tenant management, billing, quota policy, audit persistence, admin APIs, product dashboards, or conversation storage. Those responsibilities stay in the embedder application.
