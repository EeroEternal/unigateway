---
title: Protocol Conversion and Anthropic Thinking Support
---

# Protocol Conversion and Anthropic Thinking Support

## 背景

UniGateway 需要支持客户端协议和上游协议不一致的场景：客户端可能使用 OpenAI Chat Completions 或 Anthropic Messages 协议，上游也可能是 OpenAI-compatible 或 Anthropic provider。当前仓库已经有协议转换的基础结构，但还没有完整覆盖所有组合。

## 当前状态

请求入口侧，`unigateway-protocol` 已提供：

```text
openai_payload_to_chat_request
anthropic_payload_to_chat_request
```

它们会把客户端 JSON 转为 core 内部的 `ProxyChatRequest`。

上游执行侧，`unigateway-core` 已内置：

```text
OpenAiCompatibleDriver
AnthropicDriver
```

响应出口侧，`unigateway-protocol` 已提供：

```text
render_openai_chat_session
render_anthropic_chat_session
```

这些函数已经支持一部分 OpenAI 与 Anthropic 响应格式之间的转换，包括非流式和部分流式场景。

## 主要问题

### OpenAI 客户端到 Anthropic 上游还不完整

当前 `openai_payload_to_chat_request` 会保留 OpenAI 原始 `messages` 到 `raw_messages`，而 `AnthropicDriver::build_chat_request` 会优先把 `raw_messages` 直接放入 Anthropic `/v1/messages` 请求。

这对简单纯文本 `user` / `assistant` 消息可能可用，但对复杂 OpenAI 请求不可靠，例如：

```text
system role
tool role
tool_calls
content: null
OpenAI 多模态 content block
OpenAI 特有参数
```

这些内容需要显式转换为 Anthropic Messages 协议的结构，不能直接透传。

### Anthropic thinking 多轮需要保留 signature

Anthropic extended thinking 的 assistant 历史消息里，`thinking` content block 需要携带 `signature`。多轮对话继续时，客户端应把 assistant 的 thinking block 原样带回：

```json
{
  "role": "assistant",
  "content": [
    {
      "type": "thinking",
      "thinking": "...",
      "signature": "..."
    },
    {
      "type": "text",
      "text": "..."
    }
  ]
}
```

流式返回时，`signature` 通常通过 `signature_delta` 出现，需要由宿主或协议层组装回最终的 thinking block。

当前 UniGateway 在把 OpenAI reasoning 转成 Anthropic thinking 时使用了占位签名：

```text
EXTENDED_THINKING_PLACEHOLDER_SIG
```

这个值只能用于协议形状兼容，不能作为真实 Anthropic thinking signature 用于后续 Anthropic 多轮请求。

### 跨协议时存在信息损失

OpenAI 协议没有 Anthropic thinking signature 的等价字段。如果 Anthropic 上游返回给 OpenAI 客户端，再由 OpenAI 客户端发起下一轮，除非 UniGateway 或宿主层提供额外机制保存 Anthropic 原始 assistant content，否则 signature 会丢失。

## 需要保证的不变量

Anthropic 原生 thinking 多轮链路中，必须保证：

```text
thinking block 原样保留
signature 原样保留
content block 顺序原样保留
tool_use / tool_result 的 id 对应关系不被破坏
```

OpenAI 与 Anthropic 跨协议转换时，必须显式区分：

```text
可无损透传字段
可结构转换字段
只能降级表达字段
不可伪造字段
```

`signature` 属于不可伪造字段。

## 支持矩阵

```text
OpenAI client -> OpenAI-compatible upstream: 已基本支持
Anthropic client -> Anthropic upstream: 应尽量原样透传，thinking signature 需要明确保护
Anthropic client -> OpenAI-compatible upstream: 已有部分转换，thinking 会降级为 OpenAI reasoning/thinking 字段
OpenAI client -> Anthropic upstream: 当前缺口最大，需要补请求转换
```

Responses API 和 embeddings 不属于当前 chat 协议转换闭环。Anthropic driver 当前对 responses 和 embeddings 仍是未实现状态。

## 建议实现方向

### 1. 引入显式协议来源标记

`ProxyChatRequest` 当前通过 `raw_messages` 和 metadata 隐式区分 OpenAI 原始消息。后续应明确记录请求来源协议，例如：

```text
client_protocol = openai_chat | anthropic_messages | neutral
```

这样 driver 构造上游请求时可以根据来源协议选择转换策略，而不是简单判断 `raw_messages` 是否存在。

### 2. 增加 OpenAI messages 到 Anthropic messages 的转换器

需要补齐独立转换逻辑：

```text
OpenAI system message -> Anthropic top-level system
OpenAI assistant tool_calls -> Anthropic assistant content[].tool_use
OpenAI tool message -> Anthropic user content[].tool_result
OpenAI string content -> Anthropic text block
OpenAI content array -> Anthropic content blocks
OpenAI tools -> Anthropic tools
OpenAI tool_choice -> Anthropic tool_choice
```

这部分应放在中立协议转换层或 Anthropic driver 请求构造附近，但不应引入宿主产品语义。

### 3. 保留 Anthropic 原始 content block

Anthropic 客户端入口应保留原始 `messages`，尤其是 assistant thinking block。Anthropic 上游请求构造时，如果来源本身是 Anthropic，应优先无损透传，而不是从扁平 `Message` 重新生成。

### 4. 明确 signature 策略

真实 Anthropic `signature` 只能来自 Anthropic 上游或 Anthropic 客户端历史消息。UniGateway 不应该伪造可续用的 signature。

对于 OpenAI reasoning 转 Anthropic thinking 的场景，可以继续输出占位 signature，但需要将其语义定义为不可续用。更严格的方案是只在测试或兼容渲染中使用占位值，并避免把占位 signature 发送给 Anthropic 上游。

### 5. 流式聚合能力分层

Anthropic streaming 的 `thinking_delta` 和 `signature_delta` 如果要支持自动续轮，需要在协议层或宿主层聚合成最终 assistant message。UniGateway 作为库可以提供中立的聚合 helper，但不应承担会话存储、用户状态或多轮历史管理。

## 初步里程碑

```text
M1: 补齐 OpenAI -> Anthropic chat request 转换，覆盖 system、tool_calls、tool_result、tools、tool_choice。
M2: 为 Anthropic thinking signature 增加测试，确保 Anthropic -> Anthropic 路径原样透传。
M3: 明确 OpenAI reasoning -> Anthropic thinking 的降级语义，禁止占位 signature 被当作真实 signature 续传。
M4: 评估是否提供 streaming 聚合 helper，用于把 Anthropic SSE 组装成可续轮 assistant message。
```

## 边界

UniGateway 只应提供协议转换、原始事件、响应渲染、聚合 helper 和中立元数据，不应内置会话存储、用户管理、租户策略、计费或产品化后台逻辑。多轮历史如何保存和传入，仍由宿主应用负责。

## 对 llm-connector 和 xrouter 的参考分析

本节基于 `/Users/xinference/github/llm-connector` 与 `/Users/xinference/github/xrouter` 的只读分析，用于判断 UniGateway 可以借鉴哪些库层设计，不代表直接照搬实现。

### llm-connector 的可借鉴点

`llm-connector` 的核心做法是先定义一个较丰富的中间 `ChatRequest` / `Message` / `MessageBlock` / `StreamingResponse` 类型，再由 provider protocol 负责转成目标上游格式。这个方向适合 UniGateway 借鉴，但 UniGateway 应保持更窄的库边界。

值得借鉴的点：

```text
1. 中间请求模型显式保留 provider-native 字段。
2. Anthropic-native tools 和 tool_choice 与 OpenAI tools/tool_choice 并存。
3. reasoning / thinking 请求参数按 provider capability 映射。
4. 流式输出区分 OpenAI chunk 与 Anthropic event lifecycle。
5. Anthropic stream adapter 是有状态转换器，不是逐 chunk 无状态替换。
```

`llm-connector` 的 `ChatRequest` 同时包含：

```text
tools
anthropic_tools
tool_choice
anthropic_tool_choice
enable_thinking
reasoning_effort
thinking_budget
```

这避免了把 Anthropic-native 工具强行塞进 OpenAI function tool 形状后再还原。UniGateway 当前 `ProxyChatRequest` 只有 `tools` / `tool_choice` 的 JSON 字段，可以考虑增加协议来源标记和 provider-native 扩展槽，而不是只靠 `raw_messages` 推断。

`llm-connector` 的 `AnthropicProtocol::build_request` 对 OpenAI/中立消息到 Anthropic 的转换覆盖了关键结构：

```text
system message -> top-level system
assistant tool_calls -> content[].tool_use
tool role -> user content[].tool_result
tools -> Anthropic tool definition
tool_choice -> Anthropic tool_choice
thinking_budget -> Anthropic thinking: { type: enabled, budget_tokens }
```

这正好对应 UniGateway 当前 `OpenAI client -> Anthropic upstream` 的缺口。

### llm-connector 的限制

`llm-connector` 的 Anthropic response 解析结构里虽然有 `signature` 字段，但转为中间 `Message` 时没有把真实 `signature` 建模进 assistant content block；它主要保留了 `thinking` 文本，而不是完整 thinking block。

所以对 Anthropic extended thinking 多轮来说，`llm-connector` 的现状不能作为完整无损方案。UniGateway 如果要支持 Claude 原生 thinking 续轮，应优先保留原始 Anthropic content block，而不是只保留扁平 thinking 文本。

### xrouter 的可借鉴点

`xrouter` 建在 `llm-connector` 上，但额外实现了 Anthropic Messages emulator，用于让 Anthropic 客户端请求走内部 OpenAI-style 路由，再把响应转回 Anthropic。

可借鉴点主要在协议适配边界：

```text
Anthropic endpoint 入口负责把 Anthropic messages/tools/tool_choice 转成 OpenAI-style 请求。
内部路由继续使用统一 OpenAI-style payload。
出口再将 OpenAI-style response 转回 Anthropic message 或 Anthropic SSE。
```

它的 `anthropic_messages_to_openai_messages` 覆盖了：

```text
assistant content[].tool_use -> OpenAI assistant.tool_calls
user content[].tool_result -> OpenAI tool message
assistant content[].thinking -> OpenAI thinking 字段
Anthropic tools -> OpenAI function tools
Anthropic tool_choice -> OpenAI tool_choice
```

它的响应转换覆盖了：

```text
OpenAI message.content -> Anthropic text block
OpenAI message.reasoning_content/thinking -> Anthropic thinking block
OpenAI message.tool_calls -> Anthropic tool_use block
OpenAI usage prompt/completion tokens -> Anthropic input/output tokens
OpenAI finish_reason -> Anthropic stop_reason
```

它的流式 Anthropic emulator 明确按 Claude Code 期望的事件生命周期输出：

```text
message_start
ping
content_block_start
content_block_delta
content_block_stop
message_delta
message_stop
```

这点对 UniGateway 的 Anthropic stream renderer 有直接参考价值。当前 UniGateway 已经有类似方向，但 xrouter 对工具调用流式参数做了更强的缓冲与归一化。

### xrouter 对 Claude Code 的经验点

Claude Code 对 Anthropic SSE 事件序列较敏感。xrouter 的实现显示，兼容时至少需要注意：

```text
1. 需要先发 message_start。
2. message_start 后发 ping 有助于兼容。
3. thinking/text/tool_use 必须用 content_block_start/delta/stop 成对表达。
4. 结束时必须发 message_delta 和 message_stop。
5. 工具调用参数可能是增量、累计量、双重 JSON 字符串，或带 {} 前缀，需要归一化。
6. OpenAI finish_reason=tool_calls/function_call/tool_use 应映射为 Anthropic stop_reason=tool_use。
```

这些经验可以进入 UniGateway 的 stream renderer 测试，尤其是 Claude Code 兼容测试。

### xrouter 的限制

`xrouter` 对 OpenAI reasoning 转 Anthropic thinking 也使用了：

```text
EXTENDED_THINKING_PLACEHOLDER_SIG
```

这说明它解决的是“Anthropic 协议形状兼容”和 Claude Code 流式消费问题，不等于解决了 Anthropic 原生 extended thinking 多轮续传问题。这个占位 signature 不能被 UniGateway 当作真实 Anthropic signature。

另外，xrouter 是产品网关，包含认证、限流、路由、HTTP endpoint、日志、请求上下文等产品层逻辑。UniGateway 不应照搬这些上层职责，只应吸收其中的中立协议转换能力。

## 对 UniGateway 的具体借鉴方案

### 1. 中间模型增加协议来源和 provider-native 槽

当前 UniGateway 的 `ProxyChatRequest` 可以演进为显式携带：

```text
client_protocol
raw_messages
raw_system
raw_tools
raw_tool_choice
anthropic_native
openai_native
```

不一定一次性全部公开为强类型字段，但至少要避免“只要有 raw_messages 就直接透传给当前上游”的隐式策略。

### 2. OpenAI -> Anthropic request 转换优先落地

可参考 `llm-connector` 和 `xrouter` 的转换覆盖范围，先补齐：

```text
system role 合并到 top-level system
assistant tool_calls 转 content[].tool_use
tool role 转 user content[].tool_result
content string 转 text block
content array 按 block 类型转换或透传可兼容子集
tools 转 Anthropic tools
tool_choice 转 Anthropic tool_choice
stop -> stop_sequences
max_tokens 默认值与 thinking budget 的关系
```

实现时应放在 UniGateway 的协议转换层或 Anthropic driver 请求构造旁边，保持为库级转换，不引入 xrouter 的 endpoint、限流、认证、路由语义。

### 3. Anthropic 原生链路要无损优先

对于：

```text
Anthropic client -> Anthropic upstream
```

UniGateway 应优先透传 Anthropic 原始 `messages`、`system`、`tools`、`tool_choice`、`thinking`，尤其是 assistant 历史消息中的 `thinking.signature`。

这条链路不应经过 OpenAI-style 中间结构后再还原，否则容易丢失 signature、cache_control、server tool、MCP tool、自定义 tool 字段等 Anthropic-native 信息。

### 4. OpenAI reasoning -> Anthropic thinking 明确为降级

从 xrouter 和 llm-connector 看，OpenAI-style reasoning 字段可以渲染为 Anthropic `thinking` block，但没有真实 signature。UniGateway 应明确：

```text
真实 signature: 只能来自 Anthropic 上游或 Anthropic 客户端历史消息。
占位 signature: 只能用于兼容 Anthropic SSE/JSON 形状，不能发送给 Anthropic 上游作为可续轮历史。
```

可考虑在内部 metadata 中标注：

```text
thinking_signature_status = real | placeholder | absent
```

这样后续转换器可以阻止 placeholder signature 被误当作真实 signature 续传。

### 5. 提供中立 streaming adapter，而不是会话存储

可借鉴 xrouter 的 `AnthropicStreamState`，在 UniGateway 提供库级 helper：

```text
OpenAI-style stream -> Anthropic SSE event stream
Anthropic SSE event stream -> neutral stream events
Anthropic SSE event stream -> completed assistant content blocks
```

但 UniGateway 不保存用户会话，不决定是否把聚合后的 assistant message 写入历史。宿主应用负责会话存储，UniGateway 只提供可复用聚合器。

### 6. 增补测试优先级

建议优先补以下测试：

```text
OpenAI request with system/tool_calls/tool result -> Anthropic request body
Anthropic request with thinking signature -> Anthropic upstream request body 原样保留
OpenAI stream reasoning deltas -> Anthropic SSE lifecycle
OpenAI stream tool_call argument fragments -> Anthropic input_json_delta
placeholder signature 不会进入 Anthropic upstream request
Claude Code 期望的 message_start/ping/block/message_delta/message_stop 顺序
```

## Tools 转换设计

Tools 不是附带能力，而是 OpenAI / Anthropic 协议转换的核心部分。工具定义、工具选择、工具调用历史和流式工具调用需要分别处理，不能只转换顶层 `tools` 字段。

### 工具定义转换

OpenAI function tool 形状：

```json
{
  "type": "function",
  "function": {
    "name": "search",
    "description": "...",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  }
}
```

Anthropic tool 形状：

```json
{
  "name": "search",
  "description": "...",
  "input_schema": {
    "type": "object",
    "properties": {}
  }
}
```

OpenAI -> Anthropic 转换规则：

```text
tool.type=function 才作为 Anthropic tool 转换。
function.name -> name
function.description -> description
function.parameters -> input_schema
未知 OpenAI function 字段默认不进入 Anthropic tool，除非后续明确兼容策略。
```

Anthropic -> OpenAI 转换规则：

```text
name -> function.name
description -> function.description
input_schema -> function.parameters
OpenAI tool.type 固定为 function
```

Anthropic-native tools 可能包含 OpenAI 无法表达的字段：

```text
cache_control
custom_input_schema
input_examples
strict
allowed_callers
defer_loading
eager_input_streaming
server tool / MCP tool 相关字段
其他 Anthropic provider-native 扩展
```

处理策略：

```text
Anthropic client -> Anthropic upstream: 原样透传 tools，不能强制转换为 OpenAI function tool。
Anthropic client -> OpenAI-compatible upstream: 只转换 OpenAI 可表达子集，其他字段属于降级或丢失风险。
OpenAI client -> Anthropic upstream: 按 function tool 转 Anthropic tool。
```

### tool_choice 转换

OpenAI 常见形状：

```json
"auto"
```

```json
{
  "type": "function",
  "function": {
    "name": "search"
  }
}
```

Anthropic 常见形状：

```json
{ "type": "auto" }
```

```json
{ "type": "any" }
```

```json
{ "type": "tool", "name": "search" }
```

OpenAI -> Anthropic 转换规则：

```text
"auto" -> { "type": "auto" }
"none" -> { "type": "none" }
"required" -> { "type": "any" }
{ "type": "function", "function": { "name": N } } -> { "type": "tool", "name": N }
```

Anthropic -> OpenAI 转换规则：

```text
{ "type": "auto" } -> "auto"
{ "type": "none" } -> "none"
{ "type": "any" } -> "required"
{ "type": "tool", "name": N } -> { "type": "function", "function": { "name": N } }
```

无法识别的 `tool_choice` 应返回 `InvalidRequest`，不要静默降级为 `auto`，除非调用方明确选择宽松模式。

### 工具调用历史转换

OpenAI assistant 工具调用历史：

```json
{
  "role": "assistant",
  "content": null,
  "tool_calls": [
    {
      "id": "call_1",
      "type": "function",
      "function": {
        "name": "search",
        "arguments": "{\"query\":\"abc\"}"
      }
    }
  ]
}
```

Anthropic assistant 工具调用历史：

```json
{
  "role": "assistant",
  "content": [
    {
      "type": "tool_use",
      "id": "call_1",
      "name": "search",
      "input": {
        "query": "abc"
      }
    }
  ]
}
```

OpenAI tool result：

```json
{
  "role": "tool",
  "tool_call_id": "call_1",
  "content": "result"
}
```

Anthropic tool result：

```json
{
  "role": "user",
  "content": [
    {
      "type": "tool_result",
      "tool_use_id": "call_1",
      "content": "result"
    }
  ]
}
```

转换规则：

```text
OpenAI assistant.tool_calls -> Anthropic assistant content[].tool_use。
OpenAI tool role -> Anthropic user content[].tool_result。
Anthropic assistant content[].tool_use -> OpenAI assistant.tool_calls。
Anthropic user content[].tool_result -> OpenAI tool role message。
tool_call id / tool_use_id 必须原样保留。
function.arguments 是 JSON 字符串，转 Anthropic input 时需要解析为 JSON value。
Anthropic input 是 JSON value，转 OpenAI arguments 时需要序列化为 JSON 字符串。
```

特殊情况：

```text
OpenAI assistant content=null 且存在 tool_calls 时，应生成只包含 tool_use 的 Anthropic assistant message。
OpenAI function.arguments 解析失败时，应返回 InvalidRequest 或退化为 { "_raw": "..." }，具体策略需要实现时统一。
Anthropic tool_result content 如果是数组，应尽量提取 text；无法提取时可序列化为字符串。
```

### 流式工具调用转换

OpenAI streaming tool call 参数通常按碎片发送：

```json
{
  "choices": [
    {
      "delta": {
        "tool_calls": [
          {
            "index": 0,
            "id": "call_1",
            "type": "function",
            "function": {
              "name": "search",
              "arguments": "{\"query"
            }
          }
        ]
      }
    }
  ]
}
```

后续 chunk 可能继续发送：

```json
{
  "choices": [
    {
      "delta": {
        "tool_calls": [
          {
            "index": 0,
            "function": {
              "arguments": "\":\"abc\"}"
            }
          }
        ]
      }
    }
  ]
}
```

Anthropic streaming 需要表达为：

```text
content_block_start: tool_use
content_block_delta: input_json_delta
content_block_stop
```

因此 OpenAI -> Anthropic streaming 必须是有状态转换：

```text
按 tool_call.index 聚合 id/name/arguments。
arguments 可能是增量片段，也可能是累计字符串。
需要避免重复拼接累计片段。
结束时补齐尚未关闭的 tool_use block。
finish_reason=tool_calls/function_call/tool_use -> stop_reason=tool_use。
```

Anthropic -> OpenAI streaming 也需要状态：

```text
content_block_start(type=tool_use) -> OpenAI delta.tool_calls 初始片段。
input_json_delta.partial_json -> OpenAI delta.tool_calls[].function.arguments 片段。
content_block_stop -> 结束对应工具调用 block。
```

### Tools 转换验收标准

第一轮实现必须覆盖：

```text
OpenAI tools -> Anthropic tools。
Anthropic tools -> OpenAI tools 的可表达子集。
OpenAI tool_choice -> Anthropic tool_choice。
Anthropic tool_choice -> OpenAI tool_choice。
OpenAI assistant.tool_calls -> Anthropic tool_use。
OpenAI tool message -> Anthropic tool_result。
Anthropic tool_use -> OpenAI assistant.tool_calls。
Anthropic tool_result -> OpenAI tool message。
OpenAI streaming tool_call arguments -> Anthropic input_json_delta。
工具调用 id 在双向转换中不丢失。
```

## 开发计划（方案 A：强类型 ContentBlock）

基于 xrouter/llm-connector 反馈分析，决定采用**方案 A**：引入强类型 `ContentBlock` 枚举来表示消息内容块，而非仅用扁平字符串。这是破坏性变更，但能从根本上解决 thinking signature 保留问题。

### 方案对比

**方案 A（采用）**：
```rust
pub enum ContentBlock {
    Text { text: String },
    Thinking { thinking: String, signature: Option<String> },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String },
    Image { source: ImageSource },  // 未来扩展
}

pub struct Message {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,  // 结构化
}
```

优点：thinking signature 可保留；content block 顺序精确；Anthropic 原生字段不丢失。
缺点：破坏性 API 变更；需要更新所有 Message 构造点。

**方案 B（放弃）**：
保留现有 `Message { role, content: String }`，通过 `raw_content_blocks: Option<Vec<Value>>` 保留原始 blocks。
优点：低侵入。
缺点：需要两套路径；容易丢失 block 边界；难以类型安全。

### 迁移策略

1. 新增 `ContentBlock` 和新的 `MessageV2`（或直接用新 `Message`）
2. 保留旧的 `Message` 构造兼容一段时间，标记 deprecated
3. 提供 builder 模式降低构造复杂度
4. 所有 protocol 转换使用新的 content blocks API

### 阶段 0：建立保护性测试

目标是在改实现前先锁住当前期望行为，避免协议转换过程中破坏已有路径。

改动范围：

```text
unigateway-protocol/src/requests.rs
unigateway-core/src/protocol/anthropic.rs
unigateway-core/src/protocol/openai/requests.rs
unigateway-host/src/core/tests.rs
```

测试内容：

```text
OpenAI raw messages 仍能原样发给 OpenAI-compatible upstream。
Anthropic raw messages 发给 Anthropic upstream 时保留 thinking.signature。
Anthropic raw messages 发给 OpenAI-compatible upstream 时仍能转换 tool_use/tool_result。
OpenAI raw messages 不应直接透传给 Anthropic upstream。
```

验收标准：

```text
cargo test -p unigateway-protocol
cargo test -p unigateway-core
cargo test -p unigateway-host
```

### 阶段 1：引入 ContentBlock 和协议来源 metadata

基于方案 A，此阶段引入强类型内容块和协议来源标记。

#### 1.1 新增 ContentBlock 枚举

在 `unigateway-core/src/request.rs`：

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Thinking { thinking: String, signature: Option<String> },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String },
}
```

#### 1.2 更新 Message 结构

```rust
pub struct Message {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,  // 新的结构化内容
    #[deprecated(since = "2.0", note = "使用 content blocks")]
    pub text_content: Option<String>,  // 兼容字段
}
```

提供 backward-compatible 构造器：
- `Message::text(role, text)` - 创建单 text block 消息
- `Message::from_blocks(role, blocks)` - 从 content blocks 创建

#### 1.3 新增协议来源 metadata key

```text
unigateway.client_protocol = openai_chat | anthropic_messages | neutral
unigateway.thinking_signature_status = real | placeholder | absent
```

#### 改动点

```text
unigateway-core/src/request.rs - 添加 ContentBlock 和更新 Message
unigateway-protocol/src/requests.rs - 更新 parser 生成 content blocks
openai_payload_to_chat_request - 解析为 Vec<ContentBlock>
anthropic_payload_to_chat_request - 解析为 Vec<ContentBlock>
```

#### 验收标准

```text
ContentBlock::Thinking { signature: Some(...) } 可序列化/反序列化
Message::text() 构造器可用
OpenAI message 解析为 Vec<ContentBlock> (text + tool_calls)
Anthropic content blocks 解析保留 signature
metadata key 常量定义存在
```

### 阶段 2：ContentBlock 双向转换器

基于 ContentBlock 实现协议转换，而非操作原始 JSON。

#### 2.1 OpenAI -> ContentBlock 解析

```rust
pub fn openai_message_to_content_blocks(msg: &Value) -> Vec<ContentBlock>
```

转换规则：
```text
msg.content (string) -> ContentBlock::Text
msg.tool_calls[] -> ContentBlock::ToolUse { id, name, input }
msg.role="tool" -> ContentBlock::ToolResult { tool_use_id, content }
```

#### 2.2 ContentBlock -> Anthropic 序列化

```rust
pub fn content_blocks_to_anthropic(blocks: &[ContentBlock]) -> Vec<Value>
```

转换规则：
```text
ContentBlock::Text { text } -> { "type": "text", "text": ... }
ContentBlock::Thinking { thinking, signature } -> { "type": "thinking", "thinking": ..., "signature": ... }
ContentBlock::ToolUse { id, name, input } -> { "type": "tool_use", "id": ..., "name": ..., "input": ... }
ContentBlock::ToolResult { tool_use_id, content } -> { "type": "tool_result", "tool_use_id": ..., "content": ... }
```

#### 2.3 Anthropic -> ContentBlock 解析

```rust
pub fn anthropic_content_to_blocks(content: &Value) -> Vec<ContentBlock>
```

关键点：
- 保留 thinking.signature
- 区分 tool_use / tool_result / text / thinking

#### 2.4 特殊处理

```text
OpenAI reasoning/thinking 历史 -> ContentBlock::Thinking { signature: None }
  （降级处理，标记为 placeholder）
OpenAI tool_call.function.arguments JSON 解析失败 -> input: {"_raw": "原始字符串"}
Anthropic cache_control 等额外字段 -> 保留在序列化后的 Value 中（如果需要）
```

#### 验收测试

```text
OpenAI assistant with tool_calls -> Vec<ContentBlock> [ToolUse]
OpenAI tool message -> Vec<ContentBlock> [ToolResult]
Anthropic thinking with signature -> ContentBlock::Thinking with same signature
ContentBlock::Thinking -> Anthropic JSON with signature
ContentBlock::ToolUse -> Anthropic tool_use JSON
双向转换 round-trip 测试
```

### 阶段 3：Driver 层 ContentBlock 集成

改造 driver 使用 ContentBlock 而非原始 JSON 透传。

#### 3.1 ProxyChatRequest 更新

```rust
pub struct ProxyChatRequest {
    pub model: String,
    pub messages: Vec<Message>,  // Message.content 现在是 Vec<ContentBlock>
    // ... 其他字段
    pub client_protocol: ClientProtocol,  // 新增 enum，非 string
    pub signature_policy: SignaturePolicy, // real | placeholder | absent
}

pub enum ClientProtocol {
    OpenAiChat,
    AnthropicMessages,
    Neutral,
}
```

#### 3.2 AnthropicDriver::build_chat_request

根据 client_protocol 选择策略：

```text
client_protocol=AnthropicMessages:
  Message.content blocks 直接序列化为 Anthropic content
  ContentBlock::Thinking { signature: Some(s) } -> 保留真实 signature
  system/tools/tool_choice 原样保留

client_protocol=OpenAiChat:
  先通过阶段 2 的 converter 确保 Message.content 是正确 blocks
  ContentBlock::Thinking { signature: None } 允许（降级 thinking）
  ContentBlock::Thinking { signature: Some(s) } 如果来自 OpenAI 输入应拒绝

client_protocol=Neutral:
  沿用 content blocks fallback
```

#### 3.3 SignaturePolicy 强制

```rust
pub enum SignaturePolicy {
    Real,       // 真实 signature，可续传
    Placeholder, // 占位符，禁止进入 Anthropic upstream
    Absent,     // 无 signature
}

impl ContentBlock {
    pub fn signature_policy(&self) -> SignaturePolicy {
        match self {
            Thinking { signature: Some(s), .. } if !is_placeholder(s) => SignaturePolicy::Real,
            Thinking { signature: Some(_), .. } => SignaturePolicy::Placeholder,
            _ => SignaturePolicy::Absent,
        }
    }
}
```

#### 验收标准

```text
AnthropicMessages 请求 ContentBlock::Thinking 带 signature 原样透传
OpenAiChat 请求转换为 ContentBlock，thinking 无 signature
placeholder signature 被检测并阻止进入 Anthropic upstream
build_chat_request 测试覆盖三种 protocol 分支
```

### 阶段 4：增强 Anthropic SSE renderer 与 Claude Code 兼容测试

目标是把 xrouter 中已经验证过的 Claude Code 兼容经验转为 UniGateway 的库层测试和 renderer 行为。

事件顺序要求：

```text
message_start
ping
content_block_start
content_block_delta
content_block_stop
message_delta
message_stop
```

测试内容：

```text
OpenAI reasoning_content delta -> Anthropic thinking_delta。
thinking block 关闭前输出 signature_delta，但标记为 placeholder。
OpenAI content delta -> Anthropic text_delta。
OpenAI tool_calls delta -> Anthropic tool_use + input_json_delta。
finish_reason=tool_calls -> stop_reason=tool_use。
流式工具参数支持碎片、累计量、双重 JSON 字符串、{} 前缀修复。
```

边界：

```text
renderer 可以输出 placeholder signature 以兼容 Anthropic SSE 形状。
placeholder signature 不得进入 Anthropic upstream request。
renderer 不负责保存会话历史。
```

### 阶段 5：提供可选的 Anthropic stream 聚合 helper

目标是为宿主应用提供“把 Anthropic SSE 聚合成可续轮 assistant message”的工具，但不做会话存储。

候选 API：

```text
AnthropicStreamAggregator
  on_event(event)
  into_assistant_message()
```

输出应包含：

```text
role=assistant
content[].thinking
content[].signature
content[].text
content[].tool_use
```

验收标准：

```text
真实 Anthropic signature 能从 signature_delta 聚合回 thinking block。
聚合结果可由宿主作为下一轮 Anthropic messages 历史传回。
不保存用户、租户、会话状态。
```

### 阶段 6：评估强类型 API 演进

前面阶段稳定后，再评估是否把 metadata 字符串升级为 public 强类型字段。

当前基于仓库代码事实的进一步判断见 [`../dev/public-api-typing.md`](../dev/public-api-typing.md)：更合适的第一步不是立刻修改 `Message`，而是先把 `client_protocol` / `thinking_signature_status` 这类 request semantics 从 public `metadata` map 中抽成 typed view / helper。

候选演进：

```text
ClientProtocol enum
ThinkingSignatureStatus enum
Message { content: Vec<ContentBlock> }
NativeChatPayload struct
ProxyChatRequest builder
```

升级条件：

```text
metadata key 判断开始分散或难维护。
宿主侧需要可靠构造协议来源。
需要更强的编译期保证。
准备做 breaking API release。
```

### 推荐执行顺序

```text
1. 阶段 0：先写保护性测试。
2. 阶段 1：加 metadata 标记。
3. 阶段 2：实现 OpenAI -> Anthropic converter。
4. 阶段 3：接入 AnthropicDriver 分支。
5. 阶段 4：补 Claude Code SSE 兼容测试。
6. 阶段 5：视宿主需求提供聚合 helper。
7. 阶段 6：最后再考虑 public API 强类型化。
```

### 完成定义

第一轮实现可以认为完成的条件：

```text
OpenAI client -> OpenAI-compatible upstream 不回退。
Anthropic client -> Anthropic upstream 保留 thinking.signature。
Anthropic client -> OpenAI-compatible upstream 保持现有转换能力。
OpenAI client -> Anthropic upstream 支持 system/tool_calls/tool_result/tools/tool_choice。
placeholder signature 不会进入 Anthropic upstream request。
Claude Code 关键 SSE 生命周期有测试覆盖。
cargo fmt --all -- --check 通过。
cargo clippy --workspace --all-targets -- -D warnings 通过。
cargo test --workspace 通过。
```
