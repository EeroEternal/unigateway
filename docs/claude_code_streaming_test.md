# Claude Code Streaming 测试报告

## 1. 测试目标
验证 Claude Code (CLI) 能否通过 UniGateway 正常连接到 Moonshot (Kimi) 后端，重点验证流式传输 (Streaming) 的兼容性。

## 2. 背景
- **客户端**: Claude Code (默认强制开启 streaming)
- **网关**: UniGateway
- **服务端**: Moonshot AI (兼容 OpenAI 协议)

由于 Claude Code 期望接收 Anthropic 格式的 SSE 事件（如 `message_start`, `content_block_delta`），而 Moonshot 返回的是 OpenAI 格式的 SSE 数据（`data: {...}`），网关层需要进行协议转换。

## 3. 修改内容
### 3.1 依赖更新
- 升级 `llm-connector` 至 `v1.1.8`，引入了 `AnthropicSseAdapter` 用于流式格式转换。

### 3.2 代码变更
- **`src/gateway/streaming.rs`**: 实现了适配逻辑。当检测到下游需要 Anthropic 格式但上游提供 OpenAI 格式流时，使用 `AnthropicSseAdapter` 将数据流转换为标准的 Anthropic SSE 事件序列。
- **`src/gateway/chat.rs`**: 添加了协议检测逻辑。通过检查响应处理函数（`chat_response_to_anthropic_json`），自动判断下游是否为 Anthropic 客户端，从而触发流式适配器。

## 4. 验证步骤

### 4.1 环境准备
- UniGateway 运行在 `http://127.0.0.1:3210`
- 配置 Moonshot Provider (API Key 已脱敏: `sk-****************`)
- 配置测试用 Gateway Key (API Key 已脱敏: `ugk_****************`)

### 4.2 验证项 1: 网关健康检查
```bash
curl http://127.0.0.1:3210/health
# 返回: {"name":"UniGateway","status":"ok"} -> PASS
```

### 4.3 验证项 2: 手动流式请求测试
使用 `curl` 模拟 Claude 客户端发送流式请求：
```bash
curl -v http://127.0.0.1:3210/v1/messages \
  -H "x-api-key: ugk_****************" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 1024,
    "stream": true
  }'
```
**结果**: 成功接收到 Anthropic 格式的 SSE 事件流：
```
event: message_start
data: {"type":"message_start", ...}

event: content_block_start
data: {"type":"content_block_start", ...}

event: content_block_delta
data: {"type":"content_block_delta", "delta": {"type": "text_delta", "text": "Hello"}, ...}
...
event: message_stop
data: {"type":"message_stop"}
```
**状态**: **PASS**

### 4.4 验证项 3: Claude Code CLI 实测
运行 Claude Code：
```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:3210
export ANTHROPIC_API_KEY=ugk_****************
claude --model claude-3-5-sonnet-latest -p "Hello, who are you?"
```
**结果**:
```
Hello! I'm Claude, an AI assistant created by Anthropic...
```
无报错，交互正常。
**状态**: **PASS**

## 5. 结论
UniGateway 现已成功支持 Claude Code 连接 OpenAI 兼容接口（如 Moonshot），流式传输协议转换功能工作正常。
