# UniGateway 作为本地网关的优化待办

> **2026-04：** 本仓库已收敛为 **SDK / 库 workspace**，不再包含 `ug` 与根目录 `src/` HTTP 产品壳。下文中的 CLI、单进程守护、默认 `3210` 端口等描述，均指**由宿主网关产品实现时的目标形态**；库本身只提供 config / host / core / protocol 能力。

本文档面向贡献者与 AI 代理，汇总「个人本地 LLM 网关」这一**产品形态**下仍可演进的方向，作为自建网关（例如独立管理服务项目）的 backlog 参考。它补充 `memory.md`、`arch.md`，聚焦多工具流量统一托管与路由。

## 定位重申（宿主应用职责）

一个完整的本地网关通常是**无 Web UI 的流量控制面**，由你的二进制提供：

- 进程形态：常驻 HTTP 监听（端口与 bind 由你决定）。
- 交互形态：**CLI（可选）+ HTTP admin/metric**；管理 UI 外置。
- 职责边界：请求级代理与路由；会话记忆、prompt 注入、agent loop、MCP 聚合等仍建议放在上层产品。
- 目标用户：使用 Claude Code、Codex、OpenClaw、Cursor、Zed 等客户端的个人开发者与 power user。

库侧分层为 **config / host / protocol / core**；HTTP、认证、admin 路由与 CLI 属于宿主。

下文表格与小节中若仍出现 `src/...` 路径，指**已删除的旧产品壳**中的实现位置；在自建网关里请自行建立对应模块。

---

## 一、Agent 客户端的适配覆盖度

### 1.1 现状

`unigateway-core/src/protocol/` 下已经有两个 driver：

- `openai/`：chat completions、responses（含 `previous_response_id`，见 `openai/requests.rs`）、embeddings、streaming。
- `anthropic.rs`：messages 原生 driver。

当前由 `unigateway-protocol/src/responses.rs` 负责 Anthropic↔OpenAI SSE 兼容翻译与中立响应格式化。

agent 客户端覆盖的骨架已就位。以下列出需要填平的细节。

### 1.2 Header 与鉴权双轨兼容

Anthropic 系（Claude Code、OpenClaw anthropic 模式）发 `x-api-key`；OpenAI 系发 `Authorization: Bearer`。middleware 需要在两条路径上都把 token 正确识别为 **gateway key**（而非透传给上游）。

相关代码：`src/middleware.rs`、`src/gateway/support/request_flow.rs`。

其他需要关注的头：

- `anthropic-version`：建议透传，默认值兜底。
- `anthropic-beta`：目前代码无匹配，未制定策略。典型取值：
  - `prompt-caching-2024-07-31`
  - `computer-use-2024-10-22`
  - `token-efficient-tools-2025-02-19`
  - `fine-grained-tool-streaming-2025-05-14`

  **待定**：是透传、剥离，还是按目标后端能力选择性剥离。

### 1.3 高级字段保真度（fidelity）

核心隐性风险：driver 在构造上游请求时，未识别字段是否会丢失。

需要核验并补齐：

- **Anthropic `cache_control`**（`{ type: "ephemeral" }` 块级字段）：Claude Code 在长对话下强依赖，丢失会导致 token 成本爆炸。grep 当前 `protocol/anthropic.rs` 对该字段零匹配。
- **OpenAI Responses API** 字段：`reasoning.effort`、`store`、`include`、`metadata`、`instructions`、`parallel_tool_calls`。Codex-cli 依赖 `reasoning` 与 `store`。
- **工具调用的流式字节保真**：
  - Anthropic `content_block_delta` 的 `input_json_delta`
  - OpenAI `tool_calls[*].function.arguments` 增量
  - agent 会在流式期间就开始解析 JSON，任何字节错位都会让 agent 崩。
- **非文本内容块**：Anthropic `image` / `document`、OpenAI `input_image` / `input_file`。Claude Code 粘贴截图时使用。
- **工具结果块**：Anthropic `tool_result`、OpenAI `tool` role。

建议策略：driver 请求体尽量走 **raw `Value` 透传 + 只对必要字段做类型化 merge**，而不是用强类型 struct 重新拼。

### 1.4 长连接与超时

agent 单次请求（reasoning + 多轮工具链）可达十分钟级。必须保证：

- `unigateway-core` 默认 timeout 不会拦截这类请求（参见 engine builder 的 `default_timeout` 与 `transport.rs`）。
- `src/middleware.rs` 的并发队列 / 背压不会因为长时间无下行字节而超时。
- SSE 通道上 keep-alive：Anthropic 原生有 `ping` event，应透传或在 OpenAI→Anthropic 方向合成。

### 1.5 模型名路由

agent 工具普遍**硬编码** model id（`claude-3-5-sonnet-20241022`、`gpt-5-codex` 等）。当 mode 的后端是 Kimi / DeepSeek / 本地模型时，名字对不上。

需要明确并实现：

- `ModelPolicy`（`src/config/core_sync.rs`）的默认行为：透传 vs 重写。
- 支持按客户端**协议面/来源**选择映射：同一个 mode，Anthropic 入口的 `claude-3-5-sonnet` 映为后端 `kimi-k2`，OpenAI 入口的 `gpt-5` 映为 `deepseek-v3`。
- `ug route explain` 输出「进来的 model → 出去的 model」这条映射。

---

## 二、per-tool 运营视图

### 2.1 阻塞性缺口：请求上下文未携带 tool 标识

`unigateway-core/src/response.rs` 的 `RequestReport` 有 `metadata: HashMap<String, String>`，结构可扩展，但**目前 hook 拿到 report 时不知道是哪个 gateway key / 哪个工具发起的请求**。

注入路径（不改 core API，只接线）：

1. `src/gateway/support/request_flow.rs` 鉴权后，把 `gateway_key_id` 与用户可读的 `key_label`（如 `claude-code-laptop`、`codex-ci`）放入 per-request 上下文。
2. `unigateway-host/src/core/targeting.rs` 构造 `ExecutionTarget` 时把两字段塞进 metadata。
3. `RequestReport.metadata` 自然带出。

先做完这一步，后续所有运营视图才有数据源。

### 2.2 统计持久化

当前 `src/telemetry.rs::GatewayTelemetryHooks` 只做 `tracing::info!`，日志即遗忘。

需要新增 `src/stats/` 模块：

- 聚合维度：`key_label`（= tool）× `mode` × `endpoint` × `provider_kind` × 时间窗（分钟/小时/天）。
- 聚合指标：请求数、成功/失败数、p50/p95/p99 latency、input/output tokens、总 attempt 数（反映 retry 率）、按 provider 的故障率。
- 存储介质推荐 **SQLite**（`~/.config/unigateway/stats.db`）：
  - 查询语义天然匹配 `GROUP BY key_label`。
  - 单文件、零运维，符合本地优先原则。
  - 备选：内存环形缓冲 + 周期快照 JSON（更轻量但查询弱）。

实现建议：hook 里只做「入队」，后台一个 `tokio::spawn` 做批量 flush，避免在请求路径里阻塞 I/O。

### 2.3 运营曝光（admin API + CLI）

在 2.1 + 2.2 完成后，新增：

- `GET /api/admin/stats?window=24h&group_by=key_label` 聚合 JSON。
- `GET /api/admin/stats/recent?limit=100` 最近 N 条请求 trace（request_id、key_label、mode、provider、latency、status、tokens）。
- `GET /api/admin/stats/keys` 每把 key 的生命周期累计。
- `GET /api/admin/usage?window=30d` 按 tool / provider 的 token 与成本归因。

这是独立的 `/api/admin/stats/*` 路径，现有 `src/config/admin.rs` 只覆盖配置面，观测面需要新增。

### 2.4 顺带解锁的能力

数据就位后可以开启：

- **per-tool 配额**：把 `concurrency_limit` / quota 从全局/per-key 升级成 per-label。
- **成本归因**：token usage × provider 单价 → 月度账单。
- **故障自动降级**：连续 N 次失败自动用 `PATCH /api/admin/api-keys` 切 mode（API 已就位，仅需策略）。

---

## 三、CLI 强化（本地网关的主要人机界面）

既然 UG 是 headless 本地代理，**CLI 就是产品界面**，所有面向人或 agent 的操作都必须能通过 `ug <subcommand>` 完成，并同时提供机器可解析的结构化输出。

### 3.1 通用原则

- **双模输出**：所有命令同时支持人类表格输出与 `--json` 结构化输出。后者是被 agent / 脚本集成的基础。
- **退出码语义化**：0 = 成功，非 0 有明确含义（见 3.5）。
- **幂等且可组合**：CLI 是薄封装，内部一律调 admin HTTP API，不要有「只能 CLI 做、API 做不了」的操作。
- **stdin / stdout 清洁**：日志走 stderr；stdout 只输出命令结果，方便 `|` 管道。
- **无交互 fallback**：所有 interactive 命令都要有 flag 等价物，支持非交互模式。

### 3.2 现有 CLI 盘点

`src/cli/` 下已有：

- 生命周期：`ug serve`、`ug status`、`ug stop`、`ug logs`
- 模式：`ug mode list` / `show` / `use`
- 接入：`ug launch`、`ug integrations`、`ug guide`
- 诊断：`ug route explain`、`ug doctor`、`ug test`
- MCP：`ug mcp`

**缺口**：观测 / 运营 / 机器集成类命令基本为空。

### 3.3 待补 CLI 命令（观测面）

依赖 §2.1–2.3 的数据：

```
ug stats                      # 按 tool × mode 聚合的 24h 表格
ug stats --window 7d --json   # 机器可读
ug stats --watch              # 实时刷新（TUI，类似 top）
ug trail                      # 最近 N 条请求流水
ug trail --tool claude-code --tail   # 跟随某个 tool
ug usage --window 30d         # token / 成本归因
ug inspect <request_id>       # 单请求全链路详情（attempt、provider、延迟分解）
```

### 3.4 待补 CLI 命令（控制面）

把所有 admin HTTP 端点都配上 CLI 等价物：

```
ug key list                            # 当前 gateway keys + 绑定
ug key create --label claude-code --mode strong
ug key rebind <key> --mode fast        # 包装 PATCH /api/admin/api-keys
ug key revoke <key>
ug provider list / add / disable
ug mode default <mode_id>              # 包装 POST /api/admin/preferences/default-mode
ug config get <path.to.field>          # TOML 字段查询
ug config set <path.to.field> <value>  # 原子 set + 触发 core sync
```

原则：所有配置变更都走 admin API，不要让 CLI 直接写 TOML，避免绕过核心状态机。

### 3.5 机器集成命令（agent / 脚本友好）

这些命令是 UG 被其他工具调用的主要入口：

```
ug endpoint                   # 打印 base url（方便 $(ug endpoint))
ug endpoint --protocol anthropic
ug key get --tool claude-code # 根据 label 查 key，stdout 只有 key
ug env --tool codex           # 输出一组 export 语句，供 eval 使用
ug env --tool codex --json    # {"OPENAI_BASE_URL": "...", ...}
ug health                     # 最小化健康检查，适合放在 shell prompt / supervisor
ug health --json
```

示例使用模式：

```bash
# 在 shell profile 中
eval "$(ug env --tool claude-code)"
claude

# 在 supervisor 中
ug health --json | jq -e '.status == "ok"' || ug serve --restart
```

### 3.6 退出码规范

- `0`：成功
- `1`：通用错误
- `2`：CLI 参数错误
- `3`：网关未运行（`ug status` 等命令）
- `4`：鉴权失败（admin token 错误）
- `5`：资源不存在（key/mode/provider not found）
- `6`：上游不可达 / `ug test` 全部失败

### 3.7 Metrics 接口（给 Prometheus / Datadog / 脚本拉取）

补充 HTTP 侧的 `GET /metrics`（Prometheus 文本格式）与 `GET /api/admin/stats/snapshot`（JSON）。原则：

- `/metrics` 端点**免 admin token**（绑 localhost 即可），降低监控工具集成成本；如果需要保护，用单独的 `UNIGATEWAY_METRICS_TOKEN`。
- 指标命名遵循 Prometheus 惯例：
  - `unigateway_requests_total{tool,mode,provider,status}`
  - `unigateway_request_duration_seconds_bucket{...}`
  - `unigateway_tokens_total{tool,kind=input|output}`
  - `unigateway_attempts_total{endpoint,status}`
  - `unigateway_inflight_requests{mode}`
- `/api/admin/stats/snapshot` 返回同源数据的 JSON 形式，避免外部消费者每个都去 scrape `/metrics`。

这让 UG 成为**可被任意监控栈直接采集**的本地服务，而不需要我们自己写 Dashboard。

---

## 四、实施顺序建议

依赖关系：A → (D₁, C) → D₂ → B。

| 阶段 | 名称 | 触及模块 | 规模 | 风险 |
| --- | --- | --- | --- | --- |
| A | 请求上下文注入 `key_label` | `src/gateway/support/`、`runtime/core/targeting.rs` | 小 | 低 |
| D₁ | CLI 机器集成命令（§3.5）、退出码规范（§3.6） | `src/cli/` | 小 | 低 |
| C | 统计持久化（SQLite + 后台 flush） | 新增 `src/stats/` | 中 | 低 |
| D₂ | 观测 CLI（§3.3）、admin 观测端点（§2.3）、`/metrics`（§3.7） | `src/cli/`、`src/config/admin.rs`、`src/server.rs` | 中 | 低 |
| B | agent 协议保真度补齐（§1.2–1.5） | `unigateway-core/src/protocol/` | 中-大 | 中-高 |

B 放最后的原因：它依赖真实 agent 客户端的端到端冒烟来暴露问题，应该在 A+D₁+D₂ 就位后，用真实流量驱动逐工具攻坚，而不是一把梭。

---

## 五、不在本文档范围（避免范围蔓延）

以下能力**不属于**本地网关的定位，不在此 roadmap 内：

- 对话历史 / session memory
- prompt 模板库 / 提示词注入
- 本地向量检索 / RAG
- MCP server 聚合（UG 自己作为 MCP host）
- agent loop 编排
- 多用户与 RBAC

如果未来要做，应作为独立进程或独立 crate，通过 UG 的 admin API / metrics 接口与其协作，而不是塞进 gateway 本体。
