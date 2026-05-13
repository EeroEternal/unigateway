# 优化范围界定：ParaGateway 需求向 UniGateway 通用原语的映射

> **背景**：ParaGateway 提出了五项优化提案，本文件以 UniGateway 库边界（[`AGENTS.md`](../../AGENTS.md)）为准绳，逐条分析哪些适合作为通用原语进入本仓库，哪些应留在宿主层实现。
>
> 配合阅读：[`memory.md`](./memory.md)（快速心智模型）、[`../design/arch.md`](../design/arch.md)（架构描述）、[`AGENTS.md`](../../AGENTS.md)（库边界约束）。

## 核心判断原则

ParaGateway 的需求是合法输入，但不能把 `enterprise / org / project / least connections / trace header injection` 这类产品语义直接沉入 UniGateway。更合适的模式是：

- **UniGateway 提供**：限流原语、路由信号、生命周期事件、可观测指标、metadata 透传、可注入扩展点
- **宿主负责**：如何计算分数、如何解释 org/project、如何注入企业头、如何实现 Least Connections 等具体控制面策略

---

## 1. 静态并发上限（Endpoint max_concurrency）

### 提案

在 `Endpoint` 上增加 `max_concurrency: Option<usize>`，引擎在派发前检查此硬上限。

### 当前状态

- `Endpoint`（`unigateway-core/src/pool.rs:70`）无并发上限字段
- `AdaptiveConcurrency` 的 `max_concurrency` 是 AIMD 慢启动阈值，非硬上限
- 并发控制仅依赖 AIMD 自适应升降

### 判定：适合纳入

静态并发上限是通用 endpoint capacity 原语。Azure deployment、Bedrock profile、本地模型实例都可能有固定的上游并发限制，这不是 ParaGateway 专属需求。

### 实现方向

**不作为独立的两次检查**（先静态再 AIMD），而是将静态上限融入 AIMD 的 effective limit：

```
effective_limit = min(adaptive_current_limit, endpoint_max_concurrency.unwrap_or(global_max))
```

理由：
- 只维护一套 `active_connections` 计数（`aimd.rs:40`），避免静态检查和 AIMD guard 分叉导致的竞态
- `Endpoint` 字段命名为 `max_concurrency: Option<usize>`，`None` 表示无额外限制
- 不需要 `NonZeroUsize`——`Option<usize>` 配合 `None` 语义更直观，调用摩擦更低

### 涉及位置

| 区域 | 文件 |
|------|------|
| Endpoint 结构体 | `unigateway-core/src/pool.rs` |
| AIMD limit 计算 | `unigateway-core/src/engine/aimd.rs` |
| Engine dispatch（chat/embeddings/responses） | `unigateway-core/src/engine/execution/*.rs` |
| Config 投影（如有 config 端定义） | `unigateway-config/src/` |

---

## 2. ScoreOrdered 路由策略

### 提案

新增 `LoadBalancingStrategy::Priority`（或 `ScoreOrdered`），按 score 严格排序并配平局裁决。

### 当前状态

- 策略枚举：`Fallback` / `Random` / `RoundRobin`（`retry.rs:5`）
- Score 排序是**策略无关的前置步骤**：`apply_routing_feedback()`（`routing.rs:246`）对所有策略先做 score 降序排序 + suppress/cooldown 过滤，然后 `ordered_endpoints()` 中 `Fallback` 保持不变，`Random` 打散，`RoundRobin` 轮转
- 平局裁决已存在：`sort_endpoints_by_signal` 中 `right_score.total_cmp(&left_score).then_with(|| left.endpoint_id.cmp(&right.endpoint_id))`（`routing.rs:306`）

### 判定：适合纳入，命名 `ScoreOrdered`

新增策略的语义是"禁掉 Random/RoundRobin 的再扰动，严格保持 feedback 前置排序结果"。命名为 `ScoreOrdered` 而非 `Priority`：
- `ScoreOrdered` 描述机械行为（按 score 排序），是中立术语
- `Priority` 带有业务/产品语义倾向

**与 Fallback 的关系：不是重复，是不同契约**

`ScoreOrdered` 和 `Fallback` 在 `ordered_endpoints()` 中的实现分支体相同（都是空分支 `{}`），但契约不同：

| 策略 | 契约 |
|------|------|
| `Fallback` | 保持 endpoint 在 pool 中的**配置原始顺序**，逐个尝试。不承诺 score 语义 |
| `ScoreOrdered` | **声明式地**依赖 `RoutingFeedbackProvider` 提供的 score 排序。无 feedback 时退化为配置顺序，但意图明确 |

`ScoreOrdered` 的价值是**让配置意图显式化**——"我要按分值走"比"我用 Fallback（但实际因为有 feedback provider 而按分值走）"更可读。同时也让无 feedback provider 时的 `Fallback` 回归纯粹语义。

**注意：宿主层可能不需要 ScoreOrdered**

score 由宿主通过 `RoutingFeedbackProvider` trait 注入（`feedback.rs:9`），UniGateway 不计算 score，只消费。当宿主（如 ParaRouter）已在宿主层完成核心路由决策（provider 选择、计价、优先级），仅通过 `endpoint_hint` 下发到 UniGateway 做 RoundRobin 分发时，`ScoreOrdered` 不会被用到。它作为一个中立原语存在，供需要它的宿主使用。

### 补充考虑：平局随机化

当前 `endpoint_id` 确定性平局对可复现性和调试更友好；随机打散会增加审计难度。短期保持确定性平局即可，若后续有明确需求再扩展。

### 涉及位置

| 区域 | 文件 |
|------|------|
| 策略枚举 | `unigateway-core/src/retry.rs` |
| 端点排序 | `unigateway-core/src/routing.rs` |
| Config 序列化 | `unigateway-config/src/` |

---

## 3. 请求指标增强

### 提案

- 确保 `ttft_ms` 和 `total_latency_ms` 在所有驱动间一致填充
- 在 `AttemptStartedEvent` 中增加 `active_requests_at_start`

### 当前状态

- `StreamReport` 已有 `ttft_ms: Option<u64>` 和 `latency_ms: u64`（`response.rs:132`），在流式场景下持续填充
- `RequestReport` 有 `latency_ms: u64`（`response.rs:152`），无顶层 `ttft_ms`
- `AttemptStartedEvent` 无 active 计数字段（`hooks.rs:101`）
- `AdaptiveConcurrency` 维护 `active_connections: AtomicUsize`（`aimd.rs:40`），可在 attempt 启动时快照

### 判定

**TTFT/Latency 一致性：已满足，但需注意边界**

`ttft_ms` 仅在流式请求中有意义——非流式请求没有 "first token" 概念，除非将其重定义为 "first byte / first complete response"，这会改变语义。当前设计（`StreamReport` 中有、`RequestReport` 中无顶层字段）是合理的。

**active 计数：适合纳入**

`active_attempts_at_start`（或在 `AttemptStartedEvent` 中命名为 `endpoint_active_attempts_at_start`）是中立观测指标——"本次 attempt 启动时该 endpoint 有多少并发 attempt"。宿主可基于此实现 Least Connections 等策略，但策略本身留在宿主层。

命名建议：
- `active_attempts_at_start` 或 `endpoint_active_attempts_at_start`
- 避免 `active_requests_at_start`（可能与宿主层 "请求数" 混淆）

### 涉及位置

| 区域 | 文件 |
|------|------|
| AttemptStartedEvent 结构体 | `unigateway-core/src/hooks.rs` |
| Attempt 启动处（快照 active_connections） | `unigateway-core/src/engine/execution/chat.rs` 等 |
| AIMD snapshot 方法 | `unigateway-core/src/engine/aimd.rs` |

---

## 4. Metadata 透传审计

### 提案

审计 `emit_attempt_started_hook` 和 `emit_request_finished_hook`，确保完整 metadata map 被传递。

### 当前状态（已验证）

Metadata 合并路径（`engine/mod.rs:269` → `chat.rs:69`）：

```
snapshot_metadata
  → + endpoint.metadata
  → + pool_id entry
  → + request.metadata
  = DriverEndpointContext.metadata
  = AttemptStartedEvent.metadata   ✓
  = RequestReport.metadata         ✓
```

`RequestStartedEvent.metadata`（`chat.rs:36`）包含 `snapshot.metadata + request.metadata`，不含 endpoint metadata——这是合理的，因为 request started 时尚未锁定 endpoint。

### 判定：无缺口，不需要改动

当前代码路径已将请求级 metadata 正确合入所有钩子事件和 Report。提案中关于"部分 metadata 可能丢失"的担忧不适用于当前代码。

### 建议

如果 ParaGateway 仍遇到 metadata 缺失问题，应在其宿主层排查是否在构造 `ProxyChatRequest` 时未正确传入 `metadata` 字段。

---

## 5. 驱动层请求拦截

### 提案

在 `GatewayHooks` 中增强钩子，允许在驱动执行前修改 raw request headers/body。

### 当前状态

- `GatewayHooks::on_request(&mut ProxyChatRequest)` 已存在（`hooks.rs:78`），在路由解析和驱动执行前调用
- `ProxyChatRequest.metadata: HashMap<String, String>` 可携带任意键值对
- 驱动层可将 metadata 映射为 HTTP header

### 判定：现有机制已覆盖核心需求

**企业 trace ID 注入（如 `X-Enterprise-Trace-Id`）无需新机制：**

1. 宿主在构造 `ProxyChatRequest` 时将 trace ID 写入 `metadata`
2. `on_request` 钩子可追加/修改 metadata
3. 驱动层在构建 HTTP 请求时将指定 metadata 键映射为 header

**若要支持 raw HTTP header/body 修改，不建议在 GatewayHooks 中实现：**

- 不同 provider 的 raw body 结构不一致（OpenAI vs Anthropic 格式）
- raw header/body 修改可能绕过协议转换和安全约束
- 将 HTTP 传输关注点泄露到了核心库的 lifecycle hook 中

**若未来多个宿主确实需要 HTTP 层拦截，可考虑独立 transport middleware 抽象：**

```text
trait OutboundRequestMiddleware {
    // 可在出站 HTTP 请求上添加/覆盖 header、读取 metadata、附加 tracing context
    // 默认不建议暴露 raw body mutation
}
```

但短期不需要——现有 `on_request` + `metadata` 通道已满足需求。

---

## 实施优先级

| 优先级 | 条目 | 理由 |
|--------|------|------|
| P0 | #4 metadata 透传审计 | 确认当前已正确（无代码改动，仅确认） |
| P1 | #1 endpoint 静态并发上限 | 通用 capacity 原语，多个宿主场景需要 |
| P1 | #3 指标增强（`active_attempts_at_start`） | 中立可观测指标，投入小收益明确 |
| P2 | #2 `ScoreOrdered` 策略 | 非重复，是 `Fallback` 的显式化替代契约。当前 ParaRouter 在宿主层做路由决策，暂不需要 |
| - | #5 驱动钩子 | 暂不实施，现有机制已覆盖 |

---

## 不改动的边界清单

以下能力**不进入** UniGateway，由 ParaGateway 宿主层自行实现：

- Least Connections 路由算法（UniGateway 只暴露 `active_attempts_at_start` 指标）
- Latency-Based 路由算法（UniGateway 只暴露 `ttft_ms` / `latency_ms` 原始数据）
- 基于 `project_id` / `org_id` 的路由或配额逻辑
- HTTP header 注入的具体规则（通过现有 `metadata` 通道传递）
- 企业专属的评分公式或权重规则
