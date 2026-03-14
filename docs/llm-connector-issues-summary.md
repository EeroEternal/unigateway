# MiniMax / Moonshot 空响应问题排查总结

## 现象

经 unigateway 转发后，上游返回 HTTP 200，但网关给客户端的响应里 content、id、model、usage 全为空：

```json
{"choices":[{"finish_reason":"stop","index":0,"message":{"content":"","role":"assistant"}}],"created":0,"id":"","model":"","object":"chat.completion","usage":null}
```

## 根因（已定位并修复）

**问题在 unigateway，不在 llm-connector。**

`src/protocol.rs` 中的 `build_openai_client` 函数对 MiniMax 做了特殊处理：当 `family_id == "minimax"` 时，使用 `ConfigurableProtocol` 将请求路径强制改写为 `/text/chatcompletion_v2`（MiniMax 的旧版非标准接口）。该接口的响应格式与 OpenAI 标准格式不完全一致，导致 llm-connector 的 OpenAI parser 解析出空对象。

而 MiniMax 实际上支持标准 OpenAI 兼容的 `/v1/chat/completions` 端点，llm-connector 独立测试使用该标准端点时返回正常。

## 修复内容

移除了 `build_openai_client` 中针对 MiniMax 的 `ConfigurableProtocol` 特殊分支，所有 OpenAI 协议 provider（包括 MiniMax）统一使用标准 OpenAI 客户端，走 `/chat/completions` 路径。

修改文件：`src/protocol.rs`

## 验证

修复后实测（`x-target-vendor: minimax`）：
- 修复前：HTTP 200，`id/model/choices/usage/content` 全空
- 修复后：MiniMax 正常响应（当前因 API key 过期返回 401，但已不再是"静默空响应"）

## 附：unigateway 同时修复的路由问题

原有 provider 选择逻辑是按 service 下所有同类型 provider 轮询，无法显式指定目标厂商。已增加 `x-target-vendor` / `x-unigateway-provider` 请求头支持，可精准路由到指定 provider。
