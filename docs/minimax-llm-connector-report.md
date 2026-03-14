# MiniMax 空响应问题（已结案）

**结论**：根因在 unigateway 侧，已修复。无需在 llm-connector 做修改。

unigateway 此前对 MiniMax 使用 `ConfigurableProtocol` 强制走 `/text/chatcompletion_v2`，该非标准路径的响应格式与 OpenAI 解析不兼容导致空对象。现已改为统一走标准 OpenAI 客户端（`/chat/completions`），MiniMax 支持该端点。

详见 **[llm-connector-issues-summary.md](./llm-connector-issues-summary.md)**。
